"""Retain rebuildable source without redistributing prebuilt component bytes."""
import gzip
import hashlib
import json
from pathlib import PurePosixPath
import posixpath
import tarfile

from package_build import PLUGINS


def compiled_kind(prefix):
    if prefix[:4] == b"\0asm":
        return "WASM"
    if prefix in (b"!<arch>\n", b"!<thin>\n"):
        return "static archive"
    if prefix[:4] == b"\x7fELF" or prefix[:2] == b"MZ" or prefix[:4] in (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xce", b"BC\xc0\xde", b"\xde\xc0\x17\x0b"):
        return "native object/executable"
    return None


def checked_members(stream, top):
    members = stream.getmembers()
    if len(members) > 20000 or sum(member.size for member in members) > 256 * 1024 * 1024:
        raise RuntimeError("upstream source bundle exceeded member/byte bounds")
    names = set()
    for member in members:
        path = PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0] != top or member.name in names:
            raise RuntimeError("unsafe or duplicate upstream source member")
        names.add(member.name)
    return members


def hash_member(stream, member):
    data = stream.extractfile(member)
    checksum = hashlib.sha256()
    while chunk := data.read(1024 * 1024):
        checksum.update(chunk)
    return checksum.hexdigest()


def verify_source_bundle(destination, lock, expected_assets, digest):
    sources = destination / "SOURCES"
    provenance = json.loads((sources / "upstream-source-provenance.json").read_text())
    top = "zellij-" + lock["source_commit"]
    if (provenance["original_archive_url"] != lock["source_url"] or
            provenance["original_archive_sha256"] != lock["source_archive_sha256"] or
            provenance["original_archive_included"] is not False):
        raise RuntimeError("source bundle lost its pinned original archive provenance")
    excluded = [{"path": item["path"], "bytes": item["bytes"], "sha256": item["sha256"]} for item in expected_assets]
    if len(excluded) != 15 or provenance["excluded_prebuilt_wasm"] != excluded:
        raise RuntimeError("source bundle excluded-member inventory differs from pinned producer evidence")
    bundle = sources / "zellij-source.tar.gz"
    if digest(bundle) != provenance["filtered_archive_sha256"]:
        raise RuntimeError("filtered source archive changed")
    actual = {}
    with tarfile.open(bundle) as stream:
        for member in checked_members(stream, top):
            if not member.isfile():
                raise RuntimeError("filtered source bundle contains an unexpected member type")
            data = stream.extractfile(member)
            if member.name.endswith(".wasm") or compiled_kind(data.read(8)):
                raise RuntimeError("filtered source bundle contains forbidden prebuilt component bytes")
            actual[member.name] = {"bytes": member.size, "sha256": hash_member(stream, member)}
    if actual != provenance["retained_members"]:
        raise RuntimeError("filtered source bundle lost or changed retained source members")
    for required in ("Cargo.toml", "Cargo.lock", "LICENSE.md", *["default-plugins/" + name + "/Cargo.toml" for name in PLUGINS]):
        if top + "/" + required not in actual:
            raise RuntimeError("filtered source bundle lacks required rebuild source/lock/license")
    if (sources / "zellij-upstream.tar.gz").exists():
        raise RuntimeError("unfiltered upstream source archive entered package payload")
    return {"path": "SOURCES/zellij-source.tar.gz", "sha256": digest(bundle),
            "provenance_path": "SOURCES/upstream-source-provenance.json",
            "provenance_sha256": digest(sources / "upstream-source-provenance.json"),
            "retained_file_count": len(actual), "excluded_prebuilt_wasm_count": len(excluded)}


def collect_source_bundle(producer_directory, destination, lock, digest):
    archive = producer_directory / "source.tar.gz"
    if digest(archive) != lock["source_archive_sha256"]:
        raise RuntimeError("original source archive no longer matches pinned provenance")
    inventory = producer_directory / "prebuilt-assets-manifest.json"
    receipt = json.loads((producer_directory / "receipt.json").read_text())
    if digest(inventory) != receipt["prebuilt_assets_manifest_sha256"]:
        raise RuntimeError("pinned prebuilt-asset inventory changed")
    expected = json.loads(inventory.read_text())["assets"]
    sources = destination / "SOURCES"
    sources.mkdir(parents=True, exist_ok=True)
    bundle = sources / "zellij-source.tar.gz"
    top = "zellij-" + lock["source_commit"]
    excluded = []
    retained = {}
    links = []
    with tarfile.open(archive) as original, bundle.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped, tarfile.open(fileobj=zipped, mode="w|") as filtered:
        members = checked_members(original, top)
        for member in sorted(members, key=lambda item: item.name):
            if member.isdir():
                continue
            relative = str(PurePosixPath(member.name).relative_to(top))
            if member.isfile() and member.name.endswith(".wasm"):
                excluded.append({"path": relative, "bytes": member.size, "sha256": hash_member(original, member)})
                continue
            data_member = member
            if member.issym():
                resolved = posixpath.normpath(posixpath.join(posixpath.dirname(member.name), member.linkname))
                if PurePosixPath(member.name).name != "LICENSE.md" or resolved != top + "/LICENSE.md":
                    raise RuntimeError("unreviewed upstream source link")
                data_member = original.getmember(resolved)
                links.append({"path": relative, "original_target": member.linkname, "retained_as": "regular file with exact upstream root license bytes"})
            if not data_member.isfile():
                raise RuntimeError("unreviewed upstream source member type")
            if compiled_kind(original.extractfile(data_member).read(8)):
                raise RuntimeError("unlisted prebuilt WASM magic in upstream source")
            info = tarfile.TarInfo(member.name)
            info.size = data_member.size
            info.mode = 0o755 if member.mode & 0o111 and member.isfile() else 0o644
            filtered.addfile(info, original.extractfile(data_member))
            retained[member.name] = {"bytes": data_member.size, "sha256": hash_member(original, data_member)}
    if excluded != expected or len(excluded) != 15:
        raise RuntimeError("actual upstream prebuilt WASM exclusions differ from pinned inventory")
    provenance = {"schema_version": 1, "source_commit": lock["source_commit"],
                  "original_archive_url": lock["source_url"], "original_archive_sha256": lock["source_archive_sha256"],
                  "original_archive_included": False, "filtered_archive_sha256": digest(bundle),
                  "excluded_prebuilt_wasm": excluded, "retained_members": retained,
                  "materialized_license_links": links,
                  "transform": "omit exactly fifteen original prebuilt WASM members; retain every other source member byte-for-byte; materialize twelve root-license links and normalize archive metadata",
                  "source_availability": "Complete original source, manifests, lock and legal text retained; original EINTR patch and exact native/plugin build instructions alongside. Rebuild ten production plugins from source before building native executable."}
    (sources / "upstream-source-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    commands = ["# Source rebuild instructions", "", "This source bundle excludes all fifteen upstream prebuilt WASM files. Rebuild the ten production plugins before compiling the native executable. The original archive URL/SHA and exact excluded hashes are in upstream-source-provenance.json; that unfiltered archive is deliberately not included.", "", "Use Bash, a native C toolchain and the host prerequisites required by pinned upstream Zellij. These commands reproduce the producer's source/configuration choices; they do not promise byte-for-byte reproducibility or another host's ABI.", "", "```sh", "rustup toolchain install 1.95.0 --profile minimal", "rustup target add --toolchain 1.95.0 wasm32-wasip1", "tar -xzf zellij-source.tar.gz", "cd " + top, 'export CARGO_TARGET_DIR="$PWD/target"', "export CARGO_BUILD_JOBS=2", "export CARGO_PROFILE_RELEASE_LTO=false", "export CARGO_PROFILE_RELEASE_OPT_LEVEL=1", "export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16", "cargo +1.95.0 build --locked --release --target wasm32-wasip1 " + " ".join("-p " + name for name in PLUGINS), "mkdir -p zellij-utils/assets/plugins", "for plugin in " + " ".join(PLUGINS) + "; do", '  cp "$CARGO_TARGET_DIR/wasm32-wasip1/release/$plugin.wasm" "zellij-utils/assets/plugins/$plugin.wasm"', "done", "patch --batch --forward -p1 -i ../stdin-eintr.patch", "cargo +1.95.0 build --locked --release -p zellij --no-default-features --features " + ",".join(lock["features"]), "```", "", "Cargo.lock is unchanged. Registry source archives are supplied in registry/ with exact checksums and component notices. Follow the producer/owner evidence in PROVENANCE for the independent actual EINTR, pinned Wasmtime and owner acceptance gates; rebuilding alone does not establish those behaviors.", ""]
    if receipt.get("input_readiness", {}).get("requested") is True:
        native_command = "cargo +1.95.0 build --locked --release -p zellij --no-default-features --features " + ",".join(lock["features"])
        position = commands.index(native_command)
        commands[position:position] = [
            "# Original MIT optional readiness integration; corresponding source is supplied beside this archive.",
            "patch --batch --forward -p1 -i ../input-readiness-v1/input-readiness.patch",
            "cp ../input-readiness-v1/reporter.rs zellij-client/src/input_readiness.rs",
        ]
        commands += ["The separate input-readiness-v1 directory retains the exact original reporter, patch, protocol and producing source inventory. Apply both patches and copy the reporter before the final native build above; source inventory pins all four resulting compiled files. The capability is `--zmux-input-readiness-protocol`. Package provenance retains the actual readiness and frozen observer4 consumer owners; a source rebuild alone does not establish admission.", ""]
    (sources / "BUILD.md").write_text("\n".join(commands))
    return verify_source_bundle(destination, lock, expected, digest)


def filter_registry_bundle(archive, output, excluded):
    """Preserve non-WASM registry source bytes after explicit fixture admission."""
    omitted = {item["path"] for item in excluded}
    with tarfile.open(archive) as original, output.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped, tarfile.open(fileobj=zipped, mode="w|") as filtered:
        for member in sorted(original.getmembers(), key=lambda item: item.name):
            if member.isdir():
                continue
            relative = str(PurePosixPath(*PurePosixPath(member.name).parts[1:]))
            if relative in omitted:
                continue
            if not member.isfile():
                raise RuntimeError("unexpected registry source member type during filtering")
            info = tarfile.TarInfo(member.name)
            info.size = member.size
            info.mode = 0o755 if member.mode & 0o111 else 0o644
            filtered.addfile(info, original.extractfile(member))
