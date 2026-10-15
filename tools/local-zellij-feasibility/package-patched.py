#!/usr/bin/env python3
"""Package only a source-rebuilt, exactly tested Zellij artifact and its notices."""
import argparse
import gzip
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
spec = importlib.util.spec_from_file_location("zmux_package_producer", HERE / "produce-patched.py")
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)
from owner_contract import source_hashes, verify_logs
from package_build import PLUGINS
from package_notices import collect as collect_notices
from package_platform import collect as collect_platform


def read_json(path):
    if path.stat().st_size > 8 * 1024 * 1024:
        raise RuntimeError(f"JSON input exceeded byte bound: {path}")
    return json.loads(path.read_text())


def verify_chain(producer_path, owner_path):
    built = read_json(producer_path)
    owner = read_json(owner_path)
    if built["status"] != "passed" or owner["status"] != "passed" or not built.get("embedded_assets_rebuilt"):
        raise RuntimeError("packaging requires successful source-rebuilt producer and owner gates")
    if owner["producer_receipt_sha256"] != producer.digest(producer_path):
        raise RuntimeError("owner did not test this producer receipt")
    if owner["harness_sha256"] != producer.digest(HERE / "run-patched-owner.py"):
        raise RuntimeError("owner harness no longer matches its execution")
    for name, sha in owner["inputs_sha256"].items():
        if producer.digest(ROOT / name) != sha:
            raise RuntimeError(f"owner input changed since execution: {name}")
    binary = producer_path.parent / "artifacts/zellij"
    if str(binary) != built["binary_path"] or owner["binary_sha256"] != built["binary_sha256"] or producer.digest(binary) != built["binary_sha256"]:
        raise RuntimeError("package executable differs from the exact tested artifact")
    if built["lock"] != read_json(HERE / "patched-producer-lock.json"):
        raise RuntimeError("producer lock differs from package source contract")
    # The reviewed upstream config is the only build config permitted by package
    # mode. Unknown ancestor/HOME configs could select an unrecorded linker.
    source = producer_path.parent / "source" / ("zellij-" + built["lock"]["source_commit"])
    expected_config = source / ".cargo/config.toml"
    if built["cargo_config_sha256"] != {str(expected_config): producer.digest(expected_config)}:
        raise RuntimeError("package requires the reviewed upstream Cargo configuration only")
    for filename, expected in (("dependency-metadata.json", built["dependency_metadata_sha256"]),
                               ("source.tar.gz", built["source_archive_sha256"]),
                               ("prebuilt-assets-manifest.json", built["prebuilt_assets_manifest_sha256"])):
        if producer.digest(producer_path.parent / filename) != expected:
            raise RuntimeError("producer source/metadata evidence changed")
    if producer.digest(source / "Cargo.lock") != built["cargo_lock_sha256"]:
        raise RuntimeError("producer dependency lock changed")
    for filename, capture in (("native-compiler-units.json", built["native_compiler_units"]),
                              ("plugins-compiler-units.json", built["plugins"]["compiler_units"])):
        if producer.digest(producer_path.parent / filename) != capture["sha256"]:
            raise RuntimeError("compiler-unit evidence changed")
    assets_path = producer_path.parent / "embedded-assets-manifest.json"
    if producer.digest(assets_path) != built["plugins"]["manifest_sha256"]:
        raise RuntimeError("rebuilt plugin manifest changed")
    assets = read_json(assets_path)
    if not assets["rebuilt"] or tuple(asset["name"] for asset in assets["assets"]) != PLUGINS:
        raise RuntimeError("rebuilt plugin set differs from reviewed shipped assets")
    if owner["source_inputs_sha256"] != source_hashes(ROOT, producer.digest):
        raise RuntimeError("owner source differs from executed input inventory")
    labels = []
    for phase in owner["phases"]:
        logfile = owner_path.parent / phase["log"]
        if phase["exit_code"] or phase["timed_out"] or phase["log_bytes_seen"] != phase["log_bytes_retained"] or producer.digest(logfile) != phase["log_sha256"]:
            raise RuntimeError("actual owner phase failed or evidence changed")
        labels.append(phase["label"])
    if labels != ["source", "rustc", "owner-0", "owner-1", "owner-2"]:
        raise RuntimeError("actual owner suite phases missing")
    if owner["executed_owner_contract"] != verify_logs(owner_path.parent):
        raise RuntimeError("owner semantic contract evidence changed")
    return built, owner, binary, assets


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer-receipt", required=True, type=Path)
    parser.add_argument("--owner-receipt", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--reviewed-policy-sha256", help="explicit operator assertion of independently reviewed exact notice policy")
    parser.add_argument("--prepare-only", action="store_true", help="local review artifact; never promotable or named as a distributable")
    args = parser.parse_args()
    if not args.output.is_absolute():
        parser.error("output must be an absolute fresh directory")
    output = args.output
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    receipt = {"status": "failed", "promotion_ready": False,
               "scope": "explicit experimental dependency package; no default acquisition/native app selection"}
    try:
        policy = HERE / "notice-policy.json"
        policy_sha = producer.digest(policy)
        if not args.prepare_only and args.reviewed_policy_sha256 != policy_sha:
            raise RuntimeError("promotion requires the exact independently reviewed notice-policy SHA256; use --prepare-only for local review")
        producer_path = args.producer_receipt.resolve()
        owner_path = args.owner_receipt.resolve()
        built, owner, binary, assets = verify_chain(producer_path, owner_path)
        environment = {key: value for key, value in os.environ.items() if key in built["build_environment_keys"] and not key.startswith("CARGO_")}
        environment.update(LANG="C", LC_ALL="C", TZ="UTC")
        for variable in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "TMPDIR"):
            directory = output / variable.lower()
            directory.mkdir(mode=0o700)
            environment[variable] = str(directory)
        # This public setup action extracts the bytes actually embedded in the
        # tested executable, independently of source include paths/cache reuse.
        dumped = output / "dumped"
        producer.run([binary, "setup", "--dump-plugins", dumped], output, environment, output / "embedded-plugin-dump.log", 30)
        files = sorted((dumped / "plugins").iterdir())
        if {path.name for path in files} != {name + ".wasm" for name in PLUGINS}:
            raise RuntimeError("tested executable did not dump exactly the rebuilt shipped plugin set")
        for asset in assets["assets"]:
            if producer.digest(dumped / "plugins" / (asset["name"] + ".wasm")) != asset["rebuilt_sha256"]:
                raise RuntimeError("tested executable embedded stale/unproven plugin bytes")
        producer.run(["rustc", "+1.95.0", "--print", "sysroot"], output, environment, output / "sysroot.log", 30)
        sysroot = Path((output / "sysroot.log").read_text().strip())
        platform = collect_platform(binary, sysroot, built, output, environment, producer)
        payload = output / "payload"
        (payload / "bin").mkdir(parents=True)
        shutil.copy2(binary, payload / "bin/zellij")
        notice = collect_notices(producer_path.parent, payload, sysroot, producer.digest)
        shutil.copy2(output / "platform.json", payload / "platform.json")
        provenance = payload / "PROVENANCE"
        provenance.mkdir()
        platform_logs = provenance / "platform"
        platform_logs.mkdir()
        for path in sorted(output.glob("*.log")):
            shutil.copy2(path, platform_logs / path.name)
        for path in sorted(producer_path.parent.iterdir()):
            if path.is_file() and path.suffix in (".log", ".json"):
                shutil.copy2(path, provenance / ("producer-" + path.name))
        for path in sorted(owner_path.parent.iterdir()):
            if path.is_file() and path.suffix in (".log", ".json"):
                shutil.copy2(path, provenance / ("owner-" + path.name))
        inputs = ["produce-patched.py", "run-patched-owner.py", "package-patched.py", "package_build.py", "package_notices.py", "package_sources.py", "package_platform.py", "notice-policy.json", "patched-producer-lock.json", "stdin-eintr.rs", "validate-wasm.rs", "owner_contract.py", "patches/0001-retry-interrupted-stdin.patch"]
        inputs += [str(path.relative_to(HERE)) for path in sorted((HERE / "licenses/components").iterdir()) if path.is_file()]
        for name in inputs:
            target = provenance / "inputs" / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(HERE / name, target)
        manifest = {"schema_version": 1, "binary": "bin/zellij", "binary_sha256": built["binary_sha256"],
                    "target": platform["target"], "executed_host": platform["executed_host"],
                    "producer_receipt_sha256": producer.digest(producer_path), "owner_receipt_sha256": producer.digest(owner_path),
                    "owner_source_sha": owner["source_sha"], "source_commit": built["lock"]["source_commit"],
                    "notice_policy_sha256": policy_sha, "asserted_reviewed_policy_sha256": args.reviewed_policy_sha256,
                    "promotion_ready": not args.prepare_only, "notices": notice,
                    "metadata_only_text_supplements": read_json(policy)["metadata_only_text_supplements"],
                    "scope": receipt["scope"], "files": {}}
        for path in sorted(payload.rglob("*")):
            if path.is_symlink():
                raise RuntimeError("package payload contains a link")
            if path.is_file():
                manifest["files"][str(path.relative_to(payload))] = {"sha256": producer.digest(path), "bytes": path.stat().st_size}
        if len(manifest["files"]) > 10000 or sum(item["bytes"] for item in manifest["files"].values()) > 512 * 1024 * 1024:
            raise RuntimeError("package exceeded its member/byte bound")
        if producer.digest(binary) != built["binary_sha256"] or producer.digest(payload / "bin/zellij") != built["binary_sha256"]:
            raise RuntimeError("tested executable changed while packaging")
        (payload / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        prefix = "UNREVIEWED-" if args.prepare_only else ""
        name = prefix + "zellij-0.43.0-zmux-" + platform["target"] + "-" + policy_sha[:12] + ".tar.gz"
        archive = output / name
        with archive.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped, tarfile.open(fileobj=zipped, mode="w|") as stream:
            for path in sorted(payload.rglob("*")):
                if not path.is_file():
                    continue
                info = stream.gettarinfo(path, arcname=str(path.relative_to(payload)))
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ""
                info.mode = 0o755 if path == payload / "bin/zellij" else 0o644
                with path.open("rb") as data:
                    stream.addfile(info, data)
        receipt.update(status="passed", promotion_ready=not args.prepare_only, archive=str(archive),
                       archive_sha256=producer.digest(archive), binary_sha256=built["binary_sha256"],
                       manifest_sha256=producer.digest(payload / "manifest.json"), notices=notice,
                       metadata_only_text_supplements=manifest["metadata_only_text_supplements"],
                       notice_policy_sha256=policy_sha, asserted_reviewed_policy_sha256=args.reviewed_policy_sha256,
                       host=platform["executed_host"], target=platform["target"])
        (output / (name + ".sha256")).write_text(receipt["archive_sha256"] + "  " + name + "\n")
    except Exception as error:
        receipt["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
