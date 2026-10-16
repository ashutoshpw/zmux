"""Exact opt-in retained package admission; no download, build or execution."""
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import stat
import struct
import tarfile
import zipfile

HERE = Path(__file__).resolve().parent
MAX_ARCHIVE = 128 * 1024 * 1024
MAX_PAYLOAD = 512 * 1024 * 1024
MAX_FILE = 64 * 1024 * 1024
MAX_FILES = 10000
TARGET = "aarch64-apple-darwin"


class Rejected(ValueError):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_file(path, limit):
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise Rejected(f"expected regular file: {path}")
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise Rejected(f"file exceeds byte bound: {path}")
    return data


def document(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise Rejected("duplicate JSON key")
            result[key] = value
        return result
    if len(data) > 8 * 1024 * 1024:
        raise Rejected("JSON exceeds byte bound")
    return json.loads(data, object_pairs_hook=unique)


def pin():
    value = document(read_file(HERE / "retained-native-package.json", 65536))
    review = value["independent_review"]
    name = safe_name(review["path"])
    raw = read_file(HERE / name, 65536)
    if value["schema_version"] != 1 or value["target"] != TARGET or digest(raw) != review["sha256"]:
        raise Rejected("retained package review/target differs")
    verified = document(raw)
    host = next(row for row in verified["hosts"] if row["target"] == TARGET)
    if (verified["status"] != "passed" or verified["run_id"] != value["run_id"]
            or verified["source_sha"] != value["source_sha"]
            or verified["notice_policy_sha256"] != value["notice_policy_sha256"]
            or any(host[key] != value[key] for key in ("archive_sha256", "manifest_sha256", "binary_sha256"))):
        raise Rejected("retained tuple differs from independently verified package")
    return value


def native_target(architecture, system, machine):
    if architecture != "host" or system != "Darwin" or machine != "arm64":
        raise Rejected("optional Zellij requires --architecture host on Darwin ARM64; Intel/universal are unverified")
    return TARGET


def safe_name(name):
    path = PurePosixPath(name)
    if (not name or not path.parts or len(name) > 512 or "\\" in name
            or any(ord(char) < 32 or ord(char) == 127 for char in name)
            or path.is_absolute() or ".." in path.parts or str(path) != name):
        raise Rejected("unsafe or aliased package path")
    return name


def archive_files(data):
    if len(data) > MAX_ARCHIVE:
        raise Rejected("compressed package exceeds byte bound")
    with gzip.GzipFile(fileobj=io.BytesIO(data)) as zipped:
        expanded = zipped.read(MAX_PAYLOAD + 1)
    if len(expanded) > MAX_PAYLOAD:
        raise Rejected("expanded package exceeds byte bound")
    files, total = {}, 0
    with tarfile.open(fileobj=io.BytesIO(expanded), mode="r:") as archive:
        for member in archive:
            name = safe_name(member.name)
            total += member.size
            if (not member.isfile() or member.sparse is not None or name in files
                    or len(files) >= MAX_FILES or not 0 <= member.size <= MAX_FILE or total > MAX_PAYLOAD):
                raise Rejected("duplicate, nonregular or oversized package member")
            files[name] = archive.extractfile(member).read()
    for name in files:
        if any(str(parent) in files for parent in PurePosixPath(name).parents if str(parent) != "."):
            raise Rejected("package file conflicts with parent directory")
    return files


def verify_files(files, accepted):
    manifest_raw = files["manifest.json"]
    if digest(manifest_raw) != accepted["manifest_sha256"]:
        raise Rejected("package manifest differs from retained review")
    manifest = document(manifest_raw)
    if (manifest["schema_version"] != 1 or manifest["promotion_ready"] is not True
            or manifest["target"] != accepted["target"]
            or manifest["owner_source_sha"] != accepted["source_sha"]
            or manifest["notice_policy_sha256"] != accepted["notice_policy_sha256"]
            or manifest["asserted_reviewed_policy_sha256"] != accepted["notice_policy_sha256"]):
        raise Rejected("package target/source/reviewed policy differs")
    if (set(files) != set(manifest["files"]) | {"manifest.json"}
            or len(files) != accepted["files"] or sum(map(len, files.values())) != accepted["payload_bytes"]):
        raise Rejected("package file inventory differs")
    for name, expected in manifest["files"].items():
        safe_name(name)
        if len(files[name]) != expected["bytes"] or digest(files[name]) != expected["sha256"]:
            raise Rejected("package member digest/size differs: " + name)
    required = {"THIRD_PARTY_NOTICES.md", "components.json", "platform.json", "SOURCES/zellij-source.tar.gz",
                "SOURCES/upstream-source-provenance.json", "SOURCES/BUILD.md", "PROVENANCE/producer-receipt.json",
                "PROVENANCE/owner-receipt.json", "PROVENANCE/inputs/notice-policy.json", "bin/zellij"}
    if not required <= files.keys():
        raise Rejected("package lost required source/legal/provenance")
    built_raw, owner_raw = files["PROVENANCE/producer-receipt.json"], files["PROVENANCE/owner-receipt.json"]
    built, owner = document(built_raw), document(owner_raw)
    if (built["status"] != "passed" or owner["status"] != "passed"
            or digest(built_raw) != manifest["producer_receipt_sha256"] or digest(owner_raw) != manifest["owner_receipt_sha256"]
            or owner["producer_receipt_sha256"] != digest(built_raw) or owner["source_sha"] != accepted["source_sha"]
            or digest(files["PROVENANCE/inputs/notice-policy.json"]) != accepted["notice_policy_sha256"]):
        raise Rejected("package producer/owner/policy chain differs")
    binary = files["bin/zellij"]
    if (digest(binary) != accepted["binary_sha256"] or manifest["binary_sha256"] != accepted["binary_sha256"]
            or built["binary_sha256"] != accepted["binary_sha256"] or owner["binary_sha256"] != accepted["binary_sha256"]
            or len(binary) < 32 or binary[:4] != b"\xcf\xfa\xed\xfe"
            or struct.unpack_from("<I", binary, 4)[0] != 0x0100000C or struct.unpack_from("<I", binary, 12)[0] != 2):
        raise Rejected("retained native executable digest/architecture differs")
    return {key: accepted[key] for key in ("target", "source_sha", "run_id", "archive_sha256", "manifest_sha256", "binary_sha256")}


def extract_files(files, destination):
    if not destination.is_absolute():
        raise Rejected("package output must be an absolute fresh directory")
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    for name, data in sorted(files.items()):
        path = destination / safe_name(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as output:
            output.write(data)
        path.chmod(0o755 if name == "bin/zellij" else 0o644)


def accept(root, target, output=None):
    accepted = pin()
    if target != accepted["target"]:
        raise Rejected("no retained package accepted for requested target")
    root = root.resolve(strict=True)
    name = safe_name(accepted["archive_name"])
    data = read_file(root / name, MAX_ARCHIVE)
    if len(data) != accepted["archive_bytes"] or digest(data) != accepted["archive_sha256"]:
        raise Rejected("retained package archive digest/size differs")
    if read_file(root / (name + ".sha256"), 512).decode().strip() != accepted["archive_sha256"] + "  " + name:
        raise Rejected("retained package checksum sidecar differs")
    files = archive_files(data)
    result = verify_files(files, accepted)
    if output is not None:
        extract_files(files, output)
        verify_payload(output)
    return result


def verify_payload(root):
    accepted = pin()
    if root.is_symlink() or not root.is_dir():
        raise Rejected("packaged dependency must be a real directory")
    files, total, entries = {}, 0, 0
    for path in root.rglob("*"):
        entries += 1
        if entries > MAX_FILES * 2:
            raise Rejected("packaged dependency exceeds entry bound")
        if path.is_symlink():
            raise Rejected("packaged dependency contains a symlink")
        if path.is_file():
            if len(files) >= MAX_FILES:
                raise Rejected("packaged dependency exceeds file bound")
            raw = read_file(path, min(MAX_FILE, MAX_PAYLOAD - total))
            total += len(raw)
            files[str(path.relative_to(root))] = raw
        elif not path.is_dir():
            raise Rejected("packaged dependency contains a nonregular entry")
    return verify_files(files, accepted)


def verify_download(metadata, accepted):
    expected = accepted["download"]
    if (metadata["id"] != expected["artifact_id"] or metadata["name"] != expected["name"]
            or metadata["expired"] or metadata["expires_at"] != expected["expires_at"]
            or metadata["size_in_bytes"] != expected["zip_bytes"] or metadata["digest"] != "sha256:" + expected["zip_sha256"]
            or metadata["workflow_run"]["id"] != accepted["run_id"] or metadata["workflow_run"]["head_sha"] != accepted["source_sha"]):
        raise Rejected("retained artifact identity changed or expired; no fallback")


def unpack_download(archive, output, accepted):
    raw = read_file(archive, MAX_ARCHIVE)
    expected = accepted["download"]
    if len(raw) != expected["zip_bytes"] or digest(raw) != expected["zip_sha256"]:
        raise Rejected("retained ZIP digest/size differs")
    names = {accepted["archive_name"], accepted["archive_name"] + ".sha256"}
    with zipfile.ZipFile(io.BytesIO(raw)) as zipped:
        entries = zipped.infolist()
        if len(entries) != 2 or {entry.filename for entry in entries} != names:
            raise Rejected("retained ZIP inventory differs")
        for entry in entries:
            if (stat.S_IFMT(entry.external_attr >> 16) not in (0, stat.S_IFREG)
                    or entry.file_size > MAX_ARCHIVE or entry.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)):
                raise Rejected("retained ZIP member type/size differs")
        output.mkdir(mode=0o700, parents=True, exist_ok=False)
        for entry in entries:
            with (output / entry.filename).open("xb") as destination, zipped.open(entry) as source:
                while chunk := source.read(65536):
                    destination.write(chunk)
    return accept(output, accepted["target"])
