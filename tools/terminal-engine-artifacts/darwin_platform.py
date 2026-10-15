"""Inspect actual thin Mach-O objects and macOS deployment load commands."""

import struct

from verify import MAX_ARCHIVE_BYTES, Rejected, archive_objects, digest, object_target, read_bounded

MAX_LOAD_COMMANDS = 4096


def version_tuple(value):
    return value >> 16, (value >> 8) & 255, value & 255


def inspect_macho(data, architecture, minimum=(14, 0, 0), sdk=(15, 5, 0)):
    actual = object_target(data)
    if actual != ("Mach-O", architecture):
        raise Rejected(f"Mach-O architecture mismatch: {actual}, requested {architecture}")
    subtype = struct.unpack_from("<I", data, 8)[0]
    if subtype != {"arm64": 0, "x86_64": 3}[architecture]:
        raise Rejected(f"specialized or unsupported Mach-O CPU subtype: {subtype:#x}")
    ncmds, size = struct.unpack_from("<II", data, 16)
    if ncmds > MAX_LOAD_COMMANDS or 32 + size > len(data):
        raise Rejected("Mach-O load commands exceed object bounds")
    offset, versions = 32, []
    for _ in range(ncmds):
        if offset + 8 > 32 + size:
            raise Rejected("truncated Mach-O load command")
        command, length = struct.unpack_from("<II", data, offset)
        if length < 8 or length % 8 or offset + length > 32 + size:
            raise Rejected("invalid Mach-O load command length")
        if command == 0x32:  # LC_BUILD_VERSION
            if length < 24:
                raise Rejected("truncated Mach-O build version")
            platform, minos, sdk_value, tools = struct.unpack_from("<IIII", data, offset + 8)
            if platform != 1 or length != 24 + tools * 8:
                raise Rejected("unsupported Mach-O platform or build tools size")
            versions.append((version_tuple(minos), version_tuple(sdk_value)))
        elif command == 0x24:  # LC_VERSION_MIN_MACOSX
            if length != 16:
                raise Rejected("invalid legacy macOS version command")
            minos, sdk_value = struct.unpack_from("<II", data, offset + 8)
            versions.append((version_tuple(minos), version_tuple(sdk_value)))
        elif command in (0x25, 0x2F, 0x30):  # iOS/tvOS/watchOS deployment commands
            raise Rejected("non-macOS deployment command")
        offset += length
    if offset != 32 + size or len(versions) != 1:
        raise Rejected("missing, duplicate or inconsistent macOS deployment command")
    minos, actual_sdk = versions[0]
    if minos != minimum:
        raise Rejected(f"macOS minimum mismatch: {minos}, expected {minimum}")
    # Relocatable objects can leave SDK unspecified. The driver separately pins
    # the actual Xcode/SDK used; zero here never proves a particular SDK.
    if actual_sdk not in ((0, 0, 0), sdk):
        raise Rejected(f"Mach-O SDK mismatch: {actual_sdk}, expected {sdk} or unspecified")
    return {"format": actual[0], "architecture": architecture, "cpu_subtype": subtype,
            "minimum_macos": list(minos), "object_sdk": list(actual_sdk),
            "sdk_unspecified_in_object": actual_sdk == (0, 0, 0)}


def inspect_darwin_archive(path, architecture):
    data = read_bounded(path, MAX_ARCHIVE_BYTES)
    members = []
    for name, body in archive_objects(data):
        checked = inspect_macho(body, architecture)
        members.append({"name": name, "bytes": len(body), "sha256": digest(body), **checked})
    if not members:
        raise Rejected("archive contains no supported Mach-O objects")
    return {"sha256": digest(data), "bytes": len(data), "target": "macos-" + architecture,
            "members": members, "minimum_macos": [14, 0, 0],
            "scope": "Mach-O object architecture and declared deployment only; no app or runtime claim"}
