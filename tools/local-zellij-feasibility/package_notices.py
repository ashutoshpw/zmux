"""Retain exact compiler-component sources and reviewed distribution notices."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import tarfile
import tomllib
from package_sources import collect_source_bundle, filter_registry_bundle, compiled_kind

HERE = Path(__file__).resolve().parent
LEGAL = re.compile(r"^(licen[cs]e|copying|copyright|notice)([._-].*)?$", re.IGNORECASE)


def legal_path(path):
    return ((LEGAL.match(path.name) and path.suffix.lower() not in (".rs", ".c", ".h", ".pl", ".pm", ".py"))
            or any(part.lower() in ("licenses", "license", "legal") for part in path.parts[:-1]))


def registry_component(package, expected, destination, policy, digest):
    """Archive checksum and every cached source file must agree before retaining."""
    directory = Path(package["manifest_path"]).parent
    label = package["name"] + "-" + package["version"]
    registry = directory.parent.name
    archive = directory.parent.parent.parent / "cache" / registry / (label + ".crate")
    if digest(archive, 32 * 1024 * 1024) != expected:
        raise RuntimeError(f"registry archive differs from pinned Cargo.lock: {label}")
    source_path = destination / "SOURCES/registry" / archive.name
    source_path.parent.mkdir(parents=True, exist_ok=True)
    notices = []
    extra = set(policy["native_extra_notice_paths"].get(package["name"], []))
    seen_extra = set()
    archive_paths = set()
    prebuilt = []
    with tarfile.open(archive) as stream:
        members = stream.getmembers()
        if len(members) > 30000 or sum(item.size for item in members) > 256 * 1024 * 1024:
            raise RuntimeError(f"registry source archive exceeds its inspection bound: {label}")
        for member in members:
            if member.isdir():
                continue
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or name.parts[0] != label:
                raise RuntimeError(f"registry source path escaped component: {label}")
            relative = PurePosixPath(*name.parts[1:])
            if not member.isfile():
                raise RuntimeError(f"unreviewed registry source link/type: {label}/{relative}")
            archive_paths.add(str(relative))
            cached = directory / Path(str(relative))
            raw = stream.extractfile(member)
            checksum = hashlib.sha256()
            prefix = raw.read(8)
            checksum.update(prefix)
            while chunk := raw.read(1024 * 1024):
                checksum.update(chunk)
            if str(relative).endswith(".wasm") or compiled_kind(prefix):
                prebuilt.append({"path": str(relative), "bytes": member.size, "sha256": checksum.hexdigest()})
            if not cached.is_file() or cached.is_symlink() or digest(cached) != checksum.hexdigest():
                raise RuntimeError(f"compiled registry source no longer matches its archive: {label}/{relative}")
            if legal_path(relative) or str(relative) in extra:
                if member.size > 2 * 1024 * 1024:
                    raise RuntimeError(f"notice exceeds its byte bound: {label}/{relative}")
                output = destination / "NOTICES/components" / label / Path(str(relative))
                output.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(cached, output)
                notices.append(str(output.relative_to(destination)))
                seen_extra.add(str(relative))
    cached_paths = {str(path.relative_to(directory)) for path in directory.rglob("*") if path.is_file() or path.is_symlink()}
    if cached_paths - archive_paths - {".cargo-ok", ".cargo-checksum.json"}:
        raise RuntimeError(f"compiled registry source contains unarchived files: {label}")
    if seen_extra & extra != extra:
        raise RuntimeError(f"reviewed native notice disappeared: {label}: {sorted(extra - seen_extra)}")
    exclusion = policy.get("registry_source_compiled_exclusions", {}).get(package["name"] + "@" + package["version"])
    if prebuilt:
        if not exclusion or exclusion["archive_sha256"] != expected or sorted(prebuilt, key=lambda item: item["path"]) != exclusion["excluded_prebuilt_files"]:
            raise RuntimeError(f"unreviewed prebuilt binary in registry source archive: {label}")
        source_path = source_path.with_name(label + "-source.tar.gz")
        filter_registry_bundle(archive, source_path, prebuilt)
    else:
        if exclusion:
            raise RuntimeError(f"reviewed registry prebuilt exclusion disappeared: {label}")
        shutil.copy2(archive, source_path)
    override = policy["missing_text_overrides"].get(package["name"] + "@" + package["version"])
    decision = None
    if not notices:
        if override is None or override["archive_sha256"] != expected:
            raise RuntimeError(f"no reviewed notice text for compiler component: {label}")
        for text in override["texts"]:
            data = HERE / text["path"]
            if digest(data) != text["sha256"]:
                raise RuntimeError(f"reviewed notice override changed: {label}")
            output = destination / "NOTICES/components" / label / data.name
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(data, output)
            notices.append(str(output.relative_to(destination)))
        decision = override
    return {"source_archive": str(source_path.relative_to(destination)), "archive_sha256": expected,
            "original_archive_url": f"https://static.crates.io/crates/{package['name']}/{label}.crate",
            "retained_archive_sha256": digest(source_path), "source_archive_is_pristine": not prebuilt,
            "excluded_prebuilt_files": prebuilt, "source_filter_policy": exclusion,
            "notice_files": sorted(notices), "reviewed_override": decision,
            "corresponding_source_included": True}


def collect(producer_directory, destination, sysroot, digest):
    policy_path = HERE / "notice-policy.json"
    policy = json.loads(policy_path.read_text())
    metadata = json.loads((producer_directory / "dependency-metadata.json").read_text())
    packages = {package["id"]: package for package in metadata["packages"]}
    producer_lock = json.loads((producer_directory / "receipt.json").read_text())["lock"]
    source = producer_directory / "source" / ("zellij-" + producer_lock["source_commit"])
    readiness = json.loads((producer_directory / "receipt.json").read_text()).get("input_readiness", {}).get("requested") is True
    lock = tomllib.loads((source / "Cargo.lock").read_text())
    checksums = {(entry["name"], entry["version"], entry.get("source")): entry.get("checksum") for entry in lock["package"]}
    components = {}
    for filename in ("native-compiler-units.json", "plugins-compiler-units.json"):
        document = json.loads((producer_directory / filename).read_text())
        for unit in document["units"]:
            component = components.setdefault(unit["package_id"], {"units": []})
            component["units"].append({key: unit[key] for key in ("target", "features", "scope", "role")})
    rows = []
    for identifier, component in sorted(components.items()):
        package = packages[identifier]
        declared = package["license"]
        if package["source"]:
            if not package["source"].startswith("registry+") or declared not in policy["allowed_license_expressions"]:
                raise RuntimeError(f"unreviewed compiler-component source/license: {identifier}: {declared}")
            expected = checksums.get((package["name"], package["version"], package["source"]))
            if expected is None:
                raise RuntimeError(f"registry component has no pinned archive checksum: {identifier}")
            retained = registry_component(package, expected, destination, policy, digest)
        else:
            directory = Path(package["manifest_path"]).parent
            relative = str(directory.relative_to(source))
            if relative not in policy["local_root_mit_paths"] or declared not in (None, "MIT"):
                raise RuntimeError(f"unreviewed local component license: {identifier}")
            declared = "MIT (verified upstream repository root coverage)"
            retained = {"source_archive": "SOURCES/zellij-source.tar.gz", "notice_files": ["NOTICES/Zellij-LICENSE.md"],
                        "corresponding_source_included": True, "reviewed_override": None}
        rows.append({"name": package["name"], "version": package["version"], "declared_license": declared,
                     "authors": package["authors"], "repository": package["repository"],
                     "package_id": identifier, **component, **retained})
    notice_directory = destination / "NOTICES"
    notice_directory.mkdir(exist_ok=True)
    shutil.copy2(source / "LICENSE.md", notice_directory / "Zellij-LICENSE.md")
    shutil.copy2(HERE.parent.parent / "LICENSE", notice_directory / "Zmux-LICENSE")
    source_bundle = collect_source_bundle(producer_directory, destination, producer_lock, digest)
    shutil.copy2(HERE / "patches/0001-retry-interrupted-stdin.patch", destination / "SOURCES/stdin-eintr.patch")
    rust_notices = sysroot / "share/doc/rust"
    if not (rust_notices / "COPYRIGHT-library.html").is_file() or not (rust_notices / "licenses/MIT.txt").is_file():
        raise RuntimeError("exact Rust standard-library copyright inventory unavailable")
    shutil.copy2(rust_notices / "COPYRIGHT-library.html", notice_directory / "Rust-COPYRIGHT-library.html")
    shutil.copytree(rust_notices / "licenses", notice_directory / "licenses")
    document = {"schema_version": 1, "policy_sha256": digest(policy_path), "components": rows,
                "source_scope": "actual final native/plugin compiler units including marked host build support; no validator dev closure",
                "source_availability": "All listed registry sources, including pristine MPL-2.0 colored and option-ext, are included under SOURCES/registry; explicit wit-parser WASM and rustix outline-archive exclusions retain exact original/exclusion provenance; complete Zellij source/lock/license with all fifteen prebuilt WASM omitted, original patch and build instructions under SOURCES",
                "upstream_source_bundle": source_bundle,
                "metadata_only_text_supplements": policy["metadata_only_text_supplements"],
                "supplement_review_scope": policy["supplement_review_scope"],
                "rust_std_scope": "exact installed toolchain aggregate standard-library copyright inventory and referenced license directory; native plus wasm32-wasip1 target use"}
    if readiness:
        document["source_availability"] += "; original MIT readiness reporter/patch/protocol/source inventory retained under SOURCES/input-readiness-v1 with exact reconstruction instructions"
    path = destination / "components.json"
    path.write_text(json.dumps(document, indent=2) + "\n")
    lines = ["# Third-party component notices", "", document["source_availability"], "",
             "The executable contains source-rebuilt upstream plugins. Components below include conservatively retained host build support; they are not an assertion that every compiler unit is linked runtime code.", "",
             "| Component | Declared license | Retained notice files | Corresponding source |", "| --- | --- | --- | --- |"]
    for row in rows:
        links = "; ".join(f"[{Path(name).name}]({name})" for name in row["notice_files"])
        lines.append(f"| {row['name']} {row['version']} | {row['declared_license']} | {links} | [{Path(row['source_archive']).name}]({row['source_archive']}) |")
    lines += ["", "## Explicit legacy text supplementation", ""]
    for row in rows:
        if row["reviewed_override"]:
            lines.append(f"- {row['name']} {row['version']}: {row['reviewed_override']['review_basis']}. Selected/retained license: {row['reviewed_override']['selected_license']}. Manifest authors: {', '.join(row['authors'])}.")
    lines += ["", document["supplement_review_scope"], ""]
    lines += ["", "The original Zellij MIT and Zmux MIT texts are retained under NOTICES. Rust's exact standard-library copyright inventory and referenced licenses are also retained there. External system libraries identified in platform.json are not bundled by this package.", ""]
    (destination / "THIRD_PARTY_NOTICES.md").write_text("\n".join(lines))
    return {"path": str(path.relative_to(destination)), "sha256": digest(path), "component_count": len(rows),
            "source_archives": len(list((destination / "SOURCES/registry").iterdir())), "upstream_source_bundle": source_bundle}
