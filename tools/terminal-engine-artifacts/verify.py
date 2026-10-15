#!/usr/bin/env python3
"""Bounded inspection and exact-input acceptance for the F04 candidate archive.

No extraction, subprocesses, downloads, compilation, or engine selection occur here.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import struct
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
MAX_MEMBERS = 128
MAX_DOCUMENT_BYTES = 1024 * 1024
TARGETS = {
    "linux-x86_64": ("ELF", "x86_64"),
    "linux-arm64": ("ELF", "arm64"),
    "macos-x86_64": ("Mach-O", "x86_64"),
    "macos-arm64": ("Mach-O", "arm64"),
}


class Rejected(ValueError):
    pass


def read_bounded(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
    with os.fdopen(fd, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise Rejected(f"expected a regular file: {path}")
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise Rejected(f"file exceeds {limit} bytes: {path}")
    return data


def digest(data):
    return hashlib.sha256(data).hexdigest()


def document(path):
    result = json.loads(read_bounded(path, MAX_DOCUMENT_BYTES))
    if not isinstance(result, dict) or result.get("schema_version") != 1:
        raise Rejected(f"unsupported document schema: {path}")
    return result


def checked_file(root, name, expected, limit=MAX_DOCUMENT_BYTES):
    path = (root / name).resolve()
    if not path.is_relative_to(root.resolve()):
        raise Rejected(f"file escapes inventory root: {name}")
    data = read_bounded(path, limit)
    if digest(data) != expected:
        raise Rejected(f"sha256 mismatch: {name}")
    return data


def object_target(data):
    if data.startswith(b"\x7fELF"):
        if len(data) < 64 or data[4:7] != b"\x02\x01\x01":
            raise Rejected("only ELF64 little-endian version 1 objects are supported")
        kind, machine = struct.unpack_from("<HH", data, 16)
        if kind != 1 or machine not in (62, 183):
            raise Rejected("unsupported ELF object type or architecture")
        return "ELF", {62: "x86_64", 183: "arm64"}[machine]
    if data.startswith(b"\xcf\xfa\xed\xfe"):
        if len(data) < 32:
            raise Rejected("truncated Mach-O header")
        cpu, _, kind = struct.unpack_from("<III", data, 4)
        if kind != 1 or cpu not in (0x01000007, 0x0100000C):
            raise Rejected("unsupported Mach-O object type or architecture")
        return "Mach-O", {0x01000007: "x86_64", 0x0100000C: "arm64"}[cpu]
    raise Rejected("unsupported object format (thin/fat archives and bitcode are not accepted)")


def inspect_archive(path, target):
    """Read regular GNU/BSD ar in memory, never trusting stored member paths."""
    data = read_bounded(path, MAX_ARCHIVE_BYTES)
    if data[:8] != b"!<arch>\n":
        raise Rejected("expected a regular ar archive; thin/fat archives are not supported")
    offset, count, strings = 8, 0, b""
    members = []
    names = set()
    while offset < len(data):
        count += 1
        if count > MAX_MEMBERS or offset + 60 > len(data):
            raise Rejected("too many members or truncated archive header")
        header = data[offset:offset + 60]
        if header[58:] != b"`\n":
            raise Rejected("invalid archive member trailer")
        try:
            raw_name = header[:16].decode("ascii").rstrip()
            size_text = header[48:58].decode("ascii").strip()
            if not size_text.isdecimal():
                raise ValueError()
            size = int(size_text)
        except (UnicodeError, ValueError) as error:
            raise Rejected("invalid archive member name or size") from error
        start = offset + 60
        end = start + size
        if end + size % 2 > len(data):
            raise Rejected("truncated archive member or padding")
        if size % 2 and data[end:end + 1] != b"\n":
            raise Rejected("invalid archive member padding")
        body = data[start:end]
        offset = end + size % 2
        if raw_name == "//":
            if strings:
                raise Rejected("duplicate GNU archive string table")
            strings = body
            continue
        if raw_name in ("/", "/SYM64/"):
            continue
        if raw_name.startswith("#1/"):
            try:
                length = int(raw_name[3:])
            except ValueError as error:
                raise Rejected("invalid BSD archive name length") from error
            if not 0 < length <= len(body):
                raise Rejected("BSD archive name outside member")
            name = body[:length].rstrip(b"\0").decode("utf-8")
            body = body[length:]
        elif raw_name.startswith("/"):
            try:
                index = int(raw_name[1:])
            except ValueError as error:
                raise Rejected("invalid GNU archive name offset") from error
            if not 0 <= index < len(strings) or (index and strings[index - 1:index] != b"\n"):
                raise Rejected("GNU archive name outside string table")
            stop = strings.find(b"/\n", index)
            if stop < 0:
                raise Rejected("unterminated GNU archive name")
            name = strings[index:stop].decode("utf-8")
        else:
            name = raw_name.removesuffix("/")
        if name in ("__.SYMDEF", "__.SYMDEF SORTED", "__.SYMDEF_64", "__.SYMDEF_64 SORTED"):
            continue
        if not name or name in names:
            raise Rejected("empty or duplicate archive object name")
        names.add(name)
        actual = object_target(body)
        if actual != TARGETS[target]:
            raise Rejected(f"object target mismatch: {name}: {actual}, requested {target}")
        members.append({"name": name, "bytes": len(body), "sha256": digest(body),
                        "format": actual[0], "architecture": actual[1]})
    if not members:
        raise Rejected("archive contains no supported objects")
    return {"sha256": digest(data), "bytes": len(data), "target": target,
            "members": members, "scope": "object format and architecture only; no ABI or deployment claim"}


def verify_notices(root=HERE):
    manifest = document(root / "candidate.json")
    checked_file(root, "notices.json", manifest["notices_sha256"])
    inventory = document(root / "notices.json")
    notices = inventory.get("notices")
    if not isinstance(notices, list) or not notices:
        raise Rejected("empty or invalid notice inventory")
    seen = set()
    for notice in notices:
        name = notice["path"]
        if name in seen or not name.startswith("licenses/"):
            raise Rejected("invalid or duplicate notice path")
        seen.add(name)
        checked_file(root, name, notice["sha256"])
        if not notice.get("source"):
            raise Rejected(f"missing notice provenance: {name}")
    return len(seen)


def verify_candidate(library, producer, target, inventory_root=HERE):
    manifest = document(inventory_root / "candidate.json")
    if target != manifest["artifact"]["target"]:
        raise Rejected("target has no reviewed candidate manifest")
    inspected = inspect_archive(library, target)
    if inspected["sha256"] != manifest["artifact"]["sha256"]:
        raise Rejected("library sha256 does not match the accepted candidate")
    receipt_bytes = read_bounded(producer, MAX_DOCUMENT_BYTES)
    if digest(receipt_bytes) != manifest["producer_receipt"]["sha256"]:
        raise Rejected("producer receipt sha256 does not match the reviewed build")
    receipt = json.loads(receipt_bytes)
    if receipt.get("state") != "complete":
        raise Rejected("producer build was not complete")
    for name, expected in manifest["repository_inputs_sha256"].items():
        checked_file(REPO, name, expected)
    checked_file(inventory_root, "upstream-inputs.json", manifest["upstream_inputs_sha256"])
    checked_file(inventory_root, manifest["witness"]["path"], manifest["witness"]["sha256"])
    for name, expected in manifest["supporting_inventory_sha256"].items():
        checked_file(inventory_root, name, expected)
    count = verify_notices(inventory_root)
    if manifest["unresolved_notice_provenance"]:
        raise Rejected("unresolved runtime notice provenance: " + "; ".join(manifest["unresolved_notice_provenance"]))
    review = manifest["notice_review"]
    reviewed = json.loads(checked_file(inventory_root, review["path"], review["sha256"]))
    if reviewed["artifact_sha256"] != inspected["sha256"] or not reviewed["gates"] or any(
        gate["finding"] != "covered_for_observed_source" for gate in reviewed["gates"]
    ):
        raise Rejected("notice coverage review does not cover this artifact")
    for name in ("notices.json", "upstream-inputs.json"):
        checked_file(inventory_root, name, reviewed["reviewed_input_sha256"][name])
    return {"state": "verified", "library": inspected, "notices": count,
            "claims": {"engine_selected": False, "app_linked": False,
                       "portable_abi_verified": False, "native_execution_verified": False}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("notices", help="check retained notice integrity only")
    for action in ("inspect", "verify"):
        command = sub.add_parser(action)
        command.add_argument("--library", type=Path, required=True)
        command.add_argument("--target", choices=TARGETS, required=True)
        if action == "verify":
            command.add_argument("--producer-receipt", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.action == "notices":
            result = {"state": "notice_integrity_verified", "notices": verify_notices(),
                      "package_accepted": False}
        elif args.action == "inspect":
            result = inspect_archive(args.library, args.target)
            result["package_accepted"] = False
        else:
            result = verify_candidate(args.library, args.producer_receipt, args.target)
        print(json.dumps(result, indent=2))
        return 0
    except (Rejected, OSError, UnicodeError, json.JSONDecodeError, KeyError, TypeError) as error:
        print(f"artifact rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
