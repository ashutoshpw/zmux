"""Strict v1 metadata validation for packaged remote scheduler helpers."""
from __future__ import annotations

import hashlib
import json
import re
import stat
import struct
from pathlib import Path

PROTOCOL_VERSION = 1
HELPERS = {
    "linux-x86_64": {
        "os": "linux",
        "arch": "x86_64",
        "format": "elf",
        "linkage": "static-musl",
    },
    "linux-aarch64": {
        "os": "linux",
        "arch": "aarch64",
        "format": "elf",
        "linkage": "static-musl",
    },
    "darwin-universal": {
        "os": "darwin",
        "arch": "universal",
        "format": "mach-o-universal",
        "linkage": "system",
    },
}
MANIFEST_KEYS = {"schema_version", "source_sha", "scheduler_protocol_version", "helpers"}
ENTRY_KEYS = {"path", "sha256", "os", "arch", "format", "linkage"}


class ManifestError(ValueError):
    pass


def validate_linux_helper(path: Path, key: str) -> None:
    """Require a 64-bit static Linux ELF for the architecture named by key.

    Static PIE binaries may have a PT_DYNAMIC segment for relocations. The
    properties that rule out a dynamic loader and shared-library dependencies
    are the absence of PT_INTERP and DT_NEEDED entries, respectively.
    """
    expected_machines = {"linux-x86_64": 62, "linux-aarch64": 183}
    if key not in expected_machines:
        raise ManifestError(f"unsupported Linux scheduler helper key: {key}")
    try:
        data = path.read_bytes()
    except OSError as error:
        raise ManifestError(f"{key} helper cannot be read: {error}") from error
    if len(data) < 64 or data[:4] != b"\x7fELF":
        raise ManifestError(f"{key} helper is not an ELF binary")
    if data[4] != 2 or data[5] != 1 or data[6] != 1:
        raise ManifestError(f"{key} helper must be a little-endian ELF64 binary")

    elf_type, machine = struct.unpack_from("<HH", data, 16)
    version = struct.unpack_from("<I", data, 20)[0]
    entry = struct.unpack_from("<Q", data, 24)[0]
    if machine != expected_machines[key]:
        raise ManifestError(f"{key} helper has the wrong ELF architecture")
    if version != 1 or entry == 0:
        raise ManifestError(f"{key} helper has a malformed ELF executable header")
    # ET_DYN is valid for Rust's static-PIE output; ET_EXEC is valid too.
    if elf_type not in (2, 3):
        raise ManifestError(f"{key} helper is not an ELF executable")

    header_size, program_header_size, program_header_count = struct.unpack_from("<HHH", data, 52)
    program_header_offset = struct.unpack_from("<Q", data, 32)[0]
    if header_size < 64 or (program_header_count and program_header_size < 56):
        raise ManifestError(f"{key} helper has a malformed ELF header")
    if program_header_count == 0xFFFF:
        raise ManifestError(f"{key} helper uses unsupported extended ELF program headers")
    table_end = program_header_offset + program_header_size * program_header_count
    if table_end > len(data):
        raise ManifestError(f"{key} helper has a truncated ELF program header table")

    saw_load_segment = False
    for index in range(program_header_count):
        offset = program_header_offset + index * program_header_size
        (
            segment_type,
            _flags,
            file_offset,
            _virtual_address,
            _physical_address,
            file_size,
            _memory_size,
            _alignment,
        ) = struct.unpack_from("<IIQQQQQQ", data, offset)
        if file_offset + file_size > len(data):
            raise ManifestError(f"{key} helper has a truncated ELF segment")
        if segment_type == 1:  # PT_LOAD
            saw_load_segment = True
        elif segment_type == 3:  # PT_INTERP
            raise ManifestError(f"{key} helper requires a dynamic ELF interpreter")
        elif segment_type == 2:  # PT_DYNAMIC; allowed when it carries only static-PIE metadata
            if file_size == 0 or file_size % 16:
                raise ManifestError(f"{key} helper has a malformed ELF dynamic section")
            saw_null = False
            for dynamic_offset in range(file_offset, file_offset + file_size, 16):
                tag, _value = struct.unpack_from("<qQ", data, dynamic_offset)
                if tag == 0:  # DT_NULL
                    saw_null = True
                    break
                if tag == 1:  # DT_NEEDED
                    raise ManifestError(f"{key} helper depends on a dynamically linked library")
            if not saw_null:
                raise ManifestError(f"{key} helper has an unterminated ELF dynamic section")
    if not saw_load_segment:
        raise ManifestError(f"{key} helper has no ELF loadable segment")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as binary:
        for chunk in iter(lambda: binary.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def helper_path(key: str) -> str:
    if key not in HELPERS:
        raise ManifestError(f"unsupported scheduler helper key: {key}")
    return f"{key}/zmux-scheduler"


def make_manifest(source_sha: str, digests: dict[str, str]) -> dict:
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ManifestError("source_sha must be a lowercase 40-character Git SHA")
    entries = {}
    for key, digest in digests.items():
        if key not in HELPERS:
            raise ManifestError(f"unsupported scheduler helper key: {key}")
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ManifestError(f"invalid SHA-256 for {key}")
        entries[key] = {"path": helper_path(key), "sha256": digest, **HELPERS[key]}
    if not entries:
        raise ManifestError("manifest must contain at least one helper")
    return {
        "schema_version": 1,
        "source_sha": source_sha,
        "scheduler_protocol_version": PROTOCOL_VERSION,
        "helpers": entries,
    }


def validate_manifest(
    manifest: object,
    root: Path,
    *,
    expected_source_sha: str | None = None,
    required_keys: set[str] | None = None,
) -> dict:
    if not isinstance(manifest, dict) or set(manifest) != MANIFEST_KEYS:
        raise ManifestError("manifest has an invalid top-level shape")
    if type(manifest["schema_version"]) is not int or manifest["schema_version"] != 1:
        raise ManifestError("unsupported scheduler manifest schema")
    if type(manifest["scheduler_protocol_version"]) is not int or manifest["scheduler_protocol_version"] != PROTOCOL_VERSION:
        raise ManifestError("scheduler protocol version mismatch")
    source_sha = manifest["source_sha"]
    if not isinstance(source_sha, str) or not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ManifestError("manifest source_sha is invalid")
    if expected_source_sha is not None and source_sha != expected_source_sha:
        raise ManifestError("scheduler helper source SHA does not match the app")
    helpers = manifest["helpers"]
    if not isinstance(helpers, dict) or not helpers:
        raise ManifestError("manifest helpers must be a nonempty object")
    if required_keys is not None and not required_keys.issubset(helpers):
        missing = ", ".join(sorted(required_keys - set(helpers)))
        raise ManifestError(f"manifest is missing required helpers: {missing}")

    root = root.resolve(strict=True)
    for key, entry in helpers.items():
        if key not in HELPERS:
            raise ManifestError(f"unsupported scheduler helper key: {key}")
        if not isinstance(entry, dict) or set(entry) != ENTRY_KEYS:
            raise ManifestError(f"{key} has an invalid helper entry shape")
        expected = {"path": helper_path(key), **HELPERS[key]}
        if any(entry.get(name) != value for name, value in expected.items()):
            raise ManifestError(f"{key} helper metadata does not match the v1 contract")
        digest = entry.get("sha256")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ManifestError(f"{key} has an invalid SHA-256")
        path = root / entry["path"]
        try:
            metadata = path.lstat()
        except OSError as error:
            raise ManifestError(f"{key} helper is missing: {error}") from error
        if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
            raise ManifestError(f"{key} helper must be a regular executable file")
        try:
            resolved = path.resolve(strict=True)
        except OSError as error:
            raise ManifestError(f"{key} helper cannot be resolved: {error}") from error
        if not resolved.is_relative_to(root):
            raise ManifestError(f"{key} helper escapes the scheduler resource directory")
        if sha256(path) != digest:
            raise ManifestError(f"{key} helper SHA-256 does not match the manifest")
        if HELPERS[key]["format"] == "elf":
            validate_linux_helper(path, key)
    return manifest


def read_manifest(path: Path) -> dict:
    try:
        if not stat.S_ISREG(path.lstat().st_mode):
            raise ManifestError("manifest must be a regular file")
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ManifestError(f"cannot read scheduler manifest: {error}") from error
    if not isinstance(value, dict):
        raise ManifestError("manifest must be a JSON object")
    return value
