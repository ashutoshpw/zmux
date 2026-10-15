#!/usr/bin/env python3
"""Isolated pinned Zellij producer; never changes product acquisition or pins."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import selectors
import shutil
import signal
import subprocess
import sys
import tarfile
import time
import urllib.request

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(HERE))
from package_build import compiler_units, rebuild_plugins, validate_plugins
from process_group import ProcessGroup
_spec = importlib.util.spec_from_file_location("zmux_bounded_runner", REPO / "tools/terminal-feasibility/run.py")
bounded_runner = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bounded_runner)
bounded_runner.MAX_LOG_BYTES = 8 * 1024 * 1024


def digest(path, max_bytes=None):
    path = Path(path)
    if max_bytes is not None and path.stat().st_size > max_bytes:
        raise RuntimeError(f"hash input exceeded byte bound: {path}")
    result = hashlib.sha256()
    count = 0
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            count += len(chunk)
            if max_bytes is not None and count > max_bytes:
                raise RuntimeError(f"hash input exceeded byte bound: {path}")
            result.update(chunk)
    return result.hexdigest()


def run(command, cwd, environment, log, timeout=1800):
    phases = []
    print(f"producer phase {log.stem} started", flush=True)
    bounded_runner.run(command, log.stem, log.parent, phases, timeout=timeout,
                       cwd=cwd, env=environment)
    actual = log.parent / (log.stem + ".log")
    if actual != log:
        actual.replace(log)
    phase = phases[0]
    if phase["log_bytes_seen"] != phase["log_bytes_retained"]:
        raise RuntimeError(f"phase output exceeded 8 MiB: {log}")
    print(f"producer phase {log.stem} passed ({phase['wall_seconds']}s)", flush=True)
    return {**phase, "log": str(log), "sha256": digest(log)}


def stdin_case(binary, non_eintr=False):
    """Real public owner; retain group identity through EOF and bounded capture."""
    command = [str(binary)] + (["would-block"] if non_eintr else [])
    group = ProcessGroup(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, bufsize=0)
    child = group.process
    output = bytearray()
    selector = None
    input_write_error = None
    try:
        selector = selectors.DefaultSelector()
        os.set_blocking(child.stdout.fileno(), False)
        selector.register(child.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + 5
        ready = False
        sent = False
        while selector.get_map() or not group.exited():
            if time.monotonic() >= deadline:
                raise RuntimeError("actual stdin owner readiness/read/EOF deadline")
            for key, _ in selector.select(0.01):
                chunk = os.read(key.fileobj.fileno(), 16384)
                if not chunk:
                    selector.unregister(key.fileobj)
                if len(output) + len(chunk) > 65536:
                    raise RuntimeError("stdin regression output exceeds 64 KiB")
                output.extend(chunk)
            if not ready and b"READY\n" in output:
                ready = True
                if not non_eintr:
                    for _ in range(8):
                        if group.exited():
                            break
                        os.kill(child.pid, signal.SIGUSR1)
                        time.sleep(0.01)
                    if not group.exited():
                        try:
                            payload = b"kernel-stdin-after-eintr\n"
                            sent = os.write(child.stdin.fileno(), payload) == len(payload)
                        except BrokenPipeError as error:
                            # Baseline can exit between nonreaping observation
                            # and the write. Retain this race in the receipt.
                            input_write_error = str(error)
                child.stdin.close()
            if not ready and group.exited() and not selector.get_map():
                raise RuntimeError("actual stdin owner ended before readiness")
        # close() stops any pipe-closed descendants before the sole final reap.
        code = group.close(timeout=5, retry=True)
        return {"returncode": code, "output": output.decode(errors="replace"),
                "input_sent": sent, "input_write_error": input_write_error,
                "confirmed_zombie_group": group.confirmed_zombie_group}
    finally:
        try:
            if selector is not None:
                selector.close()
        finally:
            group.close(timeout=5, retry=True)


def download_source(url, archive):
    """Worker byte/read limits; the retained parent group enforces total time."""
    with urllib.request.urlopen(url, timeout=20) as response, Path(archive).open("wb") as stream:
        count = 0
        while chunk := response.read(1024 * 1024):
            count += len(chunk)
            if count > 64 * 1024 * 1024:
                raise RuntimeError("source archive exceeded 64 MiB")
            stream.write(chunk)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--cargo-target", type=Path, help="explicit absolute reusable Cargo cache")
    parser.add_argument("--test-only", action="store_true")
    parser.add_argument("--rebuild-plugins", action="store_true", help="rebuild ten embedded WASM plugins and capture package provenance")
    args = parser.parse_args()
    if not args.output.is_absolute() or (args.cargo_target and not args.cargo_target.is_absolute()) or not 1 <= args.jobs <= 4:
        parser.error("output must be absolute; jobs must be 1..4")
    if args.test_only and args.rebuild_plugins:
        parser.error("plugin package evidence requires the complete native build")
    output = args.output
    output.mkdir(parents=True, exist_ok=True)
    lock = json.loads((HERE / "patched-producer-lock.json").read_text())
    archive = output / "source.tar.gz"
    if not archive.exists():
        run([sys.executable, "-B", Path(__file__).resolve(), "--download-source",
             lock["source_url"], archive], REPO, os.environ, output / "download.log", 120)
    archive_sha256 = digest(archive, 64 * 1024 * 1024)
    if archive_sha256 != lock["source_archive_sha256"]:
        raise RuntimeError("source archive checksum mismatch")
    source = output / "source"
    if source.exists():
        raise RuntimeError("source already exists; retain previous evidence and select a fresh output directory")
    source.mkdir()
    with tarfile.open(archive) as tar:
        members = tar.getmembers()
        if len(members) > 20000 or sum(m.size for m in members) > 256 * 1024 * 1024:
            raise RuntimeError("source extraction budget exceeded")
        top = members[0].name.split("/")[0]
        tar.extractall(source, filter="data")
    source = source / top
    assets = [{"path": str(path.relative_to(source)), "bytes": path.stat().st_size,
               "sha256": digest(path)} for path in sorted(source.rglob("*.wasm"))]
    assets_path = output / "prebuilt-assets-manifest.json"
    assets_path.write_text(json.dumps({"rebuilt": False, "assets": assets}, indent=2) + "\n")
    example = source / "zellij-client/examples/zmux-stdin-eintr.rs"
    example.parent.mkdir(exist_ok=True)
    shutil.copy2(HERE / "stdin-eintr.rs", example)
    target = args.cargo_target or output / "cargo-target"
    # Build scripts receive only OS/tool lookup and network trust/proxy inputs;
    # arbitrary compiler/linker/Rust/Cargo overrides are deliberately excluded.
    allowed = {"HOME", "PATH", "USER", "LOGNAME", "TMPDIR", "TMP", "TEMP",
               "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
               "http_proxy", "https_proxy", "all_proxy", "no_proxy",
               "SSL_CERT_FILE", "SSL_CERT_DIR"}
    environment = {key: value for key, value in os.environ.items() if key in allowed}
    environment.update(LANG="C", LC_ALL="C", TZ="UTC", CARGO_TARGET_DIR=str(target),
                       CARGO_BUILD_JOBS=str(args.jobs), CARGO_PROFILE_RELEASE_LTO="false",
                       CARGO_PROFILE_RELEASE_OPT_LEVEL="1", CARGO_PROFILE_RELEASE_CODEGEN_UNITS="16")
    config_paths = {directory / ".cargo" / name for directory in (source, *source.parents)
                    for name in ("config", "config.toml")}
    config_paths.update(Path(environment["HOME"]) / ".cargo" / name
                        for name in ("config", "config.toml"))
    cargo_configs = {str(path): digest(path) for path in sorted(config_paths) if path.is_file()}
    cargo = ["cargo", "+" + lock["rust_toolchain"]]
    command = cargo + ["build", "--locked", "--release", "--no-default-features",
                       "-p", "zellij-client", "--example", "zmux-stdin-eintr"]
    binary = target / "release/examples/zmux-stdin-eintr"
    patched_file = source / "zellij-client/src/os_input_output.rs"
    receipt = {"status": "failed", "lock": lock, "host": platform.platform(),
               "source_archive_sha256": archive_sha256, "source_file_before": digest(patched_file),
               "patch_sha256": digest(HERE / "patches/0001-retry-interrupted-stdin.patch"),
               "fixture_sha256": digest(example), "cargo_lock_sha256": digest(source / "Cargo.lock"),
               "producer_sha256": digest(Path(__file__)), "cargo_target": str(target),
               "package_build_sha256": digest(HERE / "package_build.py"),
               "wasm_validator_sha256": digest(HERE / "validate-wasm.rs"),
               "metadata_capture_sha256": digest(HERE / "capture-stdout.py"),
               "embedded_assets_rebuilt": args.rebuild_plugins,
               "shared_runner_sha256": digest(REPO / "tools/terminal-feasibility/run.py"),
               "process_group_sha256": digest(REPO / "scripts/process_group.py"),
               "prebuilt_assets_manifest_sha256": digest(assets_path),
               "prebuilt_assets_count": len(assets), "prebuilt_assets_rebuilt": False,
               "build_environment_policy": "OS/tool lookup and proxy/trust allowlist; no inherited compiler/linker/Rust/Cargo overrides",
               "build_environment_keys": sorted(environment),
               "cargo_config_sha256": cargo_configs,
               "steps": [],
               "scope": "experimental producer; no acquisition swap, app redistribution or native feature closure"}
    try:
        run(["rustc", "+" + lock["rust_toolchain"], "--verbose", "--version"], source, environment, output / "rustc.log", 30)
        receipt["rustc"] = (output / "rustc.log").read_text()
        # Cargo fingerprints can accept archived old mtimes in a different
        # extracted tree. Invalidate our two source products in this explicit
        # cache; registry dependencies remain reusable. Red must compile the
        # original client, never a previously patched example.
        clean = cargo + ["clean", "--release", "-p", "zellij-client", "-p", "zellij"]
        if args.rebuild_plugins:
            clean += ["-p", "zellij-utils"]
        receipt["steps"].append(run(clean, source, environment, output / "source-products-clean.log", 60))
        if args.rebuild_plugins:
            receipt["steps"].append(run(["rustup", "target", "add", "--toolchain", lock["rust_toolchain"], "wasm32-wasip1"],
                                        source, environment, output / "wasm-target.log", 300))
            receipt["plugins"] = rebuild_plugins(source, target, cargo, environment, output, run, digest)
            receipt["wasm_validation"] = validate_plugins(source, target, cargo, environment, output,
                                                          HERE / "validate-wasm.rs", run, digest)
        receipt["steps"].append(run(command, source, environment, output / "baseline-build.log"))
        red = stdin_case(binary)
        receipt["real_eintr_red"] = red
        if red["returncode"] == 0 or "Interrupted" not in red["output"]:
            raise RuntimeError("unpatched actual stdin owner did not fail at real EINTR")
        bad_red = stdin_case(binary, True)
        receipt["non_eintr_before"] = bad_red
        if bad_red["returncode"] or "NON_EINTR_FAILED" not in bad_red["output"]:
            raise RuntimeError("unpatched non-EINTR control did not observe failure")
        run(["patch", "--batch", "--forward", "-p1", "-i", str(HERE / "patches/0001-retry-interrupted-stdin.patch")], source, environment, output / "patch.log", 30)
        receipt["source_file_after"] = digest(patched_file)
        receipt["steps"].append(run(command, source, environment, output / "patched-build.log"))
        green = stdin_case(binary)
        receipt["real_eintr_green"] = green
        if green["returncode"] or not green["input_sent"] or "READ_EXACT_ONCE" not in green["output"]:
            raise RuntimeError("patched actual stdin owner did not deliver exact bytes after EINTR")
        bad_green = stdin_case(binary, True)
        receipt["non_eintr_after"] = bad_green
        if bad_green["returncode"] or "NON_EINTR_FAILED" not in bad_green["output"]:
            raise RuntimeError("patched non-EINTR failure was swallowed")
        if not args.test_only:
            build = cargo + ["build", "--locked", "--release", "-p", "zellij",
                             "--no-default-features", "--features", ",".join(lock["features"])]
            if args.rebuild_plugins:
                build += ["--message-format=json-render-diagnostics"]
            receipt["steps"].append(run(build, source, environment, output / "zellij-build.log"))
            if args.rebuild_plugins:
                receipt["native_compiler_units"] = compiler_units(output / "zellij-build.log", output / "native-compiler-units.json",
                                                                  target, "native_and_build_support", digest)
            artifact_directory = output / "artifacts"
            artifact_directory.mkdir()
            product = artifact_directory / "zellij"
            shutil.copy2(target / "release/zellij", product)
            receipt["binary_path"] = str(product)
            receipt["binary_sha256"] = digest(product)
            run([product, "--version"], source, environment, output / "version.log", 30)
            receipt["binary_version"] = (output / "version.log").read_text().strip()
            if receipt["binary_version"] != "zellij 0.43.0":
                raise RuntimeError("patched producer version changed")
            shutil.copy2(source / "LICENSE.md", output / "Zellij-LICENSE.md")
            receipt["license_sha256"] = digest(output / "Zellij-LICENSE.md")
            metadata_command = cargo + ["metadata", "--locked", "--format-version", "1", "--no-default-features", "--features", ",".join(lock["features"])]
            metadata_path = output / "dependency-metadata.json"
            receipt["steps"].append(run([sys.executable, "-B", HERE / "capture-stdout.py", "--output", metadata_path, "--", *metadata_command],
                                         source, environment, output / "dependency-metadata.log"))
            metadata = json.loads(metadata_path.read_text())
            if not isinstance(metadata, dict) or metadata.get("version") != 1 or not isinstance(metadata.get("packages"), list) or not isinstance(metadata.get("resolve"), dict):
                raise RuntimeError("Cargo metadata stdout is not one complete dependency graph")
            receipt["dependency_metadata_sha256"] = digest(metadata_path)
        if digest(source / "Cargo.lock") != receipt["cargo_lock_sha256"]:
            raise RuntimeError("upstream dependency lock changed")
        if any(digest(path) != sha for path, sha in cargo_configs.items()):
            raise RuntimeError("effective Cargo configuration changed during producer run")
        receipt["status"] = "passed"
    finally:
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "receipt": str(output / "receipt.json")}, indent=2))


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "--download-source":
        download_source(sys.argv[2], sys.argv[3])
    else:
        main()
