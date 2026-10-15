#!/usr/bin/env python3
"""Link the production shim to immutable retained libraries; never rebuild an engine."""
import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import stat
import struct
import sys
import zipfile

from darwin_platform import inspect_macho
from verify import HERE, REPO, Rejected, checked_file, digest, document, read_bounded

MAX_ARCHIVE = 16 * 1024 * 1024
MAX_EXPANDED = 128 * 1024 * 1024
MAX_FILES = 1024
TARGETS = (("aarch64-apple-darwin", "arm64"), ("x86_64-apple-darwin", "x86_64"))
ADAPTER_TEST_NAMES = {
    "default_backend_stages_zero_reply_frames_and_abort_preserves_last_publication",
    "linked_engine_publishes_cells_modes_metadata_and_reply_as_one_exact_generation",
    "linked_engine_failure_and_partial_reply_cancellation_keep_published_metadata",
    "linked_engine_exact_history_scroll_and_alternate_remain_bounded",
}

SESSION_TEST_NAMES = {
    "actual_child_receives_engine_replies_raw_input_and_kernel_resize_through_session",
    "blocked_reply_keeps_publication_and_resize_old_then_finishes_current_user_suffix_first",
    "blocked_reply_deadline_or_close_aborts_without_publishing_or_replaying_queued_input",
    "engine_constructor_failure_precedes_child_launch_and_releases_resource_on_worker",
}
# Cargo.lock pins darwin-libproc-sys 0.2.0 (build.rs links proc) and libc 0.2.190
# (src/unix/bsd/apple/mod.rs links iconv outside rustc-dep-of-std). The linker may
# omit unused dylibs; only these exact system paths are justified for Session.
SESSION_OPTIONAL_DYLIBS = ("/usr/lib/libproc.dylib", "/usr/lib/libiconv.2.dylib")


def check_inputs():
    accepted = document(HERE / "linkage-candidates.json")["darwin"]
    recipe = document(HERE / "darwin-recipe.json")
    if (recipe["xcode_version"], recipe["sdk_version"], recipe["rust_version"], recipe["minimum_macos"]) != ("16.4", "15.5", "1.95.0", "14.0"):
        raise Rejected("unsupported linkage toolchain/deployment recipe")
    checked_file(HERE, accepted["independent_review_path"], accepted["independent_review_sha256"])
    if set(accepted["libraries"]) != {target for target, _ in TARGETS}:
        raise Rejected("linkage requires exactly both reviewed Apple slices")
    identity = accepted["download"]
    if (identity["repository"] != "ashutoshpw/zmux" or identity["run"] != 38799601657
            or identity["artifact"] != 12004400055 or not 0 < identity["archive_bytes"] <= MAX_ARCHIVE
            or identity["name"] != "terminal-engine-darwin-thin-" + accepted["source_sha"]):
        raise Rejected("unsupported retained download identity")
    return accepted, recipe


def verify_download_identity(metadata, accepted):
    identity = accepted["download"]
    if (metadata["id"] != identity["artifact"] or metadata["name"] != identity["name"]
            or metadata["expired"] or metadata["size_in_bytes"] != identity["archive_bytes"]
            or metadata["digest"] != "sha256:" + identity["archive_sha256"]
            or metadata["workflow_run"]["id"] != identity["run"]
            or metadata["workflow_run"]["head_sha"] != accepted["source_sha"]):
        raise Rejected("retained artifact identity/size changed or expired; no fallback")


def extract_retained(archive, destination, expected_digest, expected_bytes):
    data = read_bounded(archive, MAX_ARCHIVE)
    if len(data) != expected_bytes or digest(data) != expected_digest:
        raise Rejected("retained ZIP digest/size differs")
    with zipfile.ZipFile(archive) as zipped:
        entries = zipped.infolist()
        if len(entries) > MAX_FILES or sum(entry.file_size for entry in entries) > MAX_EXPANDED:
            raise Rejected("retained ZIP exceeds extraction bounds")
        names = set()
        for entry in entries:
            name = entry.filename.rstrip("/")
            path = PurePosixPath(name)
            kind = stat.S_IFMT(entry.external_attr >> 16)
            if (not name or "\\" in name or path.is_absolute() or ".." in path.parts
                    or str(path) != name or name in names
                    or kind not in (0, stat.S_IFREG, stat.S_IFDIR)
                    or entry.file_size > 64 * 1024 * 1024
                    or entry.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)):
                raise Rejected("unsafe, duplicate or oversized retained ZIP entry")
            names.add(name)
        # Validate every path before the first write. The destination is newly owned.
        for entry in entries:
            target = destination / entry.filename
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with zipped.open(entry) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, length=65536)
    return {"sha256": digest(data), "bytes": len(data), "entries": len(entries)}


def supplement_bytes(response, expected_path, expected_digest):
    if (response["type"] != "file" or response["path"] != expected_path
            or response["encoding"] != "base64" or not 0 < response["size"] <= 1024 * 1024):
        raise Rejected("unexpected exact-source supplement response")
    data = base64.b64decode(response["content"].replace("\n", ""), validate=True)
    if len(data) != response["size"] or digest(data) != expected_digest:
        raise Rejected("exact-source supplement digest/size differs")
    return data


def cargo_outputs(log, package_id, bridge_id, target_directory, test_name="staged_engine"):
    executables, owners = set(), set()
    for line in log.splitlines():
        if not line.startswith("{"):
            continue  # Retained Cargo stderr diagnostics are not JSON messages.
        record = json.loads(line)
        if (record.get("reason") == "compiler-artifact" and record["package_id"] == package_id
                and record["target"]["name"] == test_name and record["target"]["kind"] == ["test"]
                and record.get("executable")):
            executables.add(record["executable"])
        if record.get("reason") == "build-script-executed" and record["package_id"] == bridge_id:
            owners.add(record["out_dir"])
    if len(executables) != 1 or len(owners) != 1:
        raise Rejected("Cargo did not report one exact staged test and owner output")
    executable, owner = Path(executables.pop()).resolve(), Path(owners.pop()).resolve()
    if not executable.is_relative_to(target_directory.resolve()) or not owner.is_relative_to(target_directory.resolve()):
        raise Rejected("Cargo output escaped the fresh target directory")
    return executable, owner


def dynamic_libraries(data, optional_system_libraries=()):
    # Called only after inspect_macho validates every load-command length/boundary.
    count = struct.unpack_from("<I", data, 16)[0]
    offset, libraries = 32, []
    for _ in range(count):
        command, length = struct.unpack_from("<II", data, offset)
        if command == 0x8000001C:  # LC_RPATH
            raise Rejected("unexpected dynamic runtime search path")
        if command in (0xC, 0x80000018, 0x8000001F, 0x20, 0x80000023):
            if length < 24:
                raise Rejected("truncated dylib load command")
            start = struct.unpack_from("<I", data, offset + 8)[0]
            if not 24 <= start < length:
                raise Rejected("invalid dylib name offset")
            value = data[offset + start:offset + length]
            end = value.find(b"\0")
            if end < 0:
                raise Rejected("unterminated dylib load name")
            libraries.append(value[:end].decode("utf-8", errors="strict"))
        offset += length
    required = "/usr/lib/libSystem.B.dylib"
    allowed = {required, *optional_system_libraries}
    if libraries.count(required) != 1 or len(set(libraries)) != len(libraries) or not set(libraries) <= allowed:
        raise Rejected("unexplained dynamic runtime dependency: " + repr(libraries))
    return libraries


def source_hashes(include_session_runtime=False):
    paths = {REPO / "Cargo.toml", REPO / "Cargo.lock", REPO / "scripts/process_group.py",
             REPO / ".github/workflows/terminal-engine-linkage.yml"}
    directories = [REPO / "crates/ghostty-vt", REPO / "crates/terminal-adapter", HERE]
    if include_session_runtime:
        directories += [REPO / "crates/session-runtime", REPO / "vendor/darwin-libproc"]
    for directory in directories:
        paths.update(path for path in directory.rglob("*") if path.is_file() and "__pycache__" not in path.parts)
    paths.update(REPO / name for name in document(HERE / "candidate.json")["repository_inputs_sha256"])
    return {str(path.relative_to(REPO)): digest(read_bounded(path, 2 * 1024 * 1024)) for path in sorted(paths)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check-inputs", action="store_true")
    parser.add_argument("--session-runtime", action="store_true",
                        help="Also compile both optional Session targets and execute ARM PTY tests")
    args = parser.parse_args()
    if args.check_inputs:
        accepted, _ = check_inputs()
        print(json.dumps({"state": "linkage_inputs_verified", "download": accepted["download"],
                          "sources": len(source_hashes(args.session_runtime)), "session_runtime_requested": args.session_runtime,
                          "engine_rebuilt": False,
                          "native_linkage_verified": False}, indent=2))
        return 0
    if args.output is None:
        parser.error("--output is required for native linkage")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    receipt = {"schema_version": 1, "state": "starting", "phases": [], "targets": [],
               "claims": {"engine_rebuilt": False, "default_engine_changed": False,
                          "optional_session_arm64_executed": False, "appkit_linked": False,
                          "intel_executed": False, "minimum_os_executed": False,
                          "performance_measured": False}}
    try:
        accepted, recipe = check_inputs()
        frozen = source_hashes(args.session_runtime)
        receipt["source_sha256"] = frozen
        if platform.system() != "Darwin" or platform.machine() != "arm64":
            raise Rejected("new linkage execution requires a Darwin ARM64 host")
        spec = importlib.util.spec_from_file_location("retained_linkage_runner", REPO / "tools/terminal-feasibility/run.py")
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("DYLD_") and key not in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CFLAGS", "LDFLAGS", "CC", "AR")}
        environment.update(DEVELOPER_DIR=recipe["developer_dir"], MACOSX_DEPLOYMENT_TARGET="14.0")

        def run(command, label, timeout=60, cwd=None):
            result = runner.run(command, label, output, receipt["phases"], timeout=timeout, cwd=cwd, env=environment)
            if receipt["phases"][-1]["log_bytes_seen"] != receipt["phases"][-1]["log_bytes_retained"]:
                raise Rejected("required linkage output was truncated: " + label)
            return result.strip()

        receipt["source_sha"] = run(["git", "rev-parse", "HEAD"], "source", cwd=REPO)
        xcode = run(["xcodebuild", "-version"], "xcode")
        sdk = Path(run(["xcrun", "--sdk", "macosx", "--show-sdk-path"], "sdk-path"))
        sdk_version = run(["xcrun", "--sdk", "macosx", "--show-sdk-version"], "sdk-version")
        clang = run(["xcrun", "--find", "clang"], "clang-path")
        archive_tool = run(["xcrun", "--find", "ar"], "ar-path")
        rust = run(["rustc", "--version"], "rust-version")
        if xcode != "Xcode 16.4\nBuild version 16F6" or sdk_version != "15.5" or not rust.startswith("rustc 1.95.0 "):
            raise Rejected("host Xcode/SDK/Rust differs from the reviewed linkage recipe")
        receipt["developer"] = {"xcode": xcode, "sdk_version": sdk_version, "sdk_path": str(sdk),
                                "rust": rust, "host_macos": platform.mac_ver()[0],
                                "sdk_settings_sha256": digest(read_bounded(sdk / "SDKSettings.json", 1024 * 1024))}
        environment.update(CC=clang, AR=archive_tool, SDKROOT=str(sdk))
        work, bundle = output / "work", output / "bundle"
        work.mkdir()
        bundle.mkdir()
        root = bundle / "accepted-inputs"
        original = root / "original-upload"
        original.mkdir(parents=True)
        identity = accepted["download"]
        endpoint = f"repos/{identity['repository']}/actions/artifacts/{identity['artifact']}"
        metadata = json.loads(run(["gh", "api", endpoint], "download-metadata"))
        verify_download_identity(metadata, accepted)
        receipt["download_identity"] = metadata
        # Use gh's authenticated redirect handling; credentials stay out of argv/logs.
        # Reuse the owned process-group runner with a bounded binary capture size.
        old_log_limit = runner.MAX_LOG_BYTES
        try:
            runner.MAX_LOG_BYTES = MAX_ARCHIVE
            run(["gh", "api", endpoint + "/zip"], "retained-archive", timeout=120)
        finally:
            runner.MAX_LOG_BYTES = old_log_limit
        archive = output / "retained-archive.log"
        receipt["archive"] = extract_retained(archive, original, identity["archive_sha256"], identity["archive_bytes"])
        archive.rename(work / "retained-artifact.zip")
        receipt["phases"][-1]["log"] = "work/retained-artifact.zip"
        supplemental = root / "supplemental-source"
        for name, expected in accepted["supplement"].items():
            response = json.loads(run(["gh", "api", f"repos/{identity['repository']}/contents/{name}?ref={accepted['source_sha']}"], "source-supplement"))
            data = supplement_bytes(response, name, expected)
            destination = supplemental / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        checked_file(HERE, accepted["independent_review_path"], accepted["independent_review_sha256"])
        shutil.copy2(HERE / accepted["independent_review_path"], root / "independent-review.json")
        environment["ZMUX_GHOSTTY_ARTIFACT_ROOT"] = str(root)
        target_directory = work / "cargo"
        environment["CARGO_TARGET_DIR"] = str(target_directory)
        flags = ["-C", "linker=" + clang, "-C", "link-arg=-isysroot", "-C", "link-arg=" + str(sdk),
                 "-C", "link-arg=-mmacosx-version-min=14.0"]
        environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
        metadata = json.loads(run(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], "cargo-metadata", cwd=REPO))
        packages = {package["name"]: package["id"] for package in metadata["packages"]}
        for target, architecture in TARGETS:
            result = {"target": target, "architecture": architecture,
                      "engine_sha256": accepted["libraries"][target], "execution": "not_executed"}
            receipt["targets"].append(result)
            acceptance = json.loads(run([sys.executable, str(HERE / "accept-linkage.py"), "--root", str(root), "--target", target], "accept-" + architecture))
            result["acceptance"] = acceptance
            log = run(["cargo", "test", "--locked", "-p", "zmux-terminal-adapter", "--features", "ghostty",
                       "--test", "staged_engine", "--target", target, "--no-run", "--message-format=json"],
                      "compile-link-" + architecture, timeout=600, cwd=REPO)
            executable, owner = cargo_outputs(log, packages["zmux-terminal-adapter"], packages["zmux-ghostty-vt"], target_directory)
            retained = bundle / architecture
            retained.mkdir()
            for source, name, is_executable in ((owner / "zmux-vt.o", "zmux-vt.o", False), (executable, "staged-engine", True)):
                data = read_bounded(source, 64 * 1024 * 1024)
                destination = retained / name
                shutil.copy2(source, destination)
                record = {"sha256": digest(data), "bytes": len(data)}
                result.setdefault("outputs", {})[name] = record
                inventory = inspect_macho(data, architecture, executable=is_executable)
                if is_executable:
                    inventory["dynamic_libraries"] = dynamic_libraries(data)
                record.update(inventory)
            if architecture == "arm64":
                stdout = run([str(retained / "staged-engine"), "--test-threads=1", "--nocapture"], "execute-arm64", timeout=30)
                if "test result: ok. 4 passed; 0 failed; 0 ignored;" not in stdout or any("test " + name + " ... ok" not in stdout for name in ADAPTER_TEST_NAMES):
                    raise Rejected("ARM64 adapter did not execute all four required behavioral cases")
                result["execution"] = "passed_on_arm64_host"
            if args.session_runtime:
                session = {"execution": "not_executed", "dynamic_library_policy": {
                    "required": ["/usr/lib/libSystem.B.dylib"], "allowed_optional": list(SESSION_OPTIONAL_DYLIBS),
                    "sources": {
                        "/usr/lib/libproc.dylib": "Cargo.lock pinned darwin-libproc-sys 0.2.0 build.rs links dylib=proc",
                        "/usr/lib/libiconv.2.dylib": "Cargo.lock pinned libc 0.2.190 src/unix/bsd/apple/mod.rs links iconv outside rustc-dep-of-std"}}}
                result["session_runtime"] = session
                log = run(["cargo", "test", "--locked", "-p", "zmux-session-runtime", "--features", "ghostty",
                           "--test", "engine", "--target", target, "--no-run", "--message-format=json"],
                          "compile-session-" + architecture, timeout=600, cwd=REPO)
                executable, owner = cargo_outputs(log, packages["zmux-session-runtime"], packages["zmux-ghostty-vt"], target_directory, "engine")
                object_data = read_bounded(owner / "zmux-vt.o", 16 * 1024 * 1024)
                session["owner_object"] = {"sha256": digest(object_data), **inspect_macho(object_data, architecture)}
                if session["owner_object"]["sha256"] != result["outputs"]["zmux-vt.o"]["sha256"]:
                    raise Rejected("Session linked a different host shim object")
                data = read_bounded(executable, 64 * 1024 * 1024)
                # Preserve candidate bytes/hash even if native inspection refuses.
                # Retention is diagnostic and does not authorize execution.
                shutil.copy2(executable, retained / "session-engine")
                session["executable"] = {"sha256": digest(data), "bytes": len(data)}
                inventory = inspect_macho(data, architecture, executable=True)
                inventory["dynamic_libraries"] = dynamic_libraries(data, SESSION_OPTIONAL_DYLIBS)
                session["executable"].update(inventory)
                if architecture == "arm64":
                    session["raw_pty_capacity"] = json.loads(run(
                        [sys.executable, str(HERE / "probe-pty-capacity.py")], "probe-pty-capacity", timeout=10))
                    session["execution"] = "attempted_on_arm64_host"
                    stdout = run([str(retained / "session-engine"), "--test-threads=1", "--nocapture"], "execute-session-arm64", timeout=60)
                    if "test result: ok. 4 passed; 0 failed; 0 ignored;" not in stdout or any("test " + name + " ... ok" not in stdout for name in SESSION_TEST_NAMES):
                        raise Rejected("ARM64 Session did not execute all four required PTY cases")
                    session["execution"] = "passed_on_arm64_host"
                    receipt["claims"]["optional_session_arm64_executed"] = True
                session["state"] = "compiled_linked_inspected"
            result["state"] = "compiled_linked_inspected"
        if source_hashes(args.session_runtime) != frozen:
            raise Rejected("linkage source inputs changed during validation")
        for target, architecture in TARGETS:
            checked_file(root / "original-upload/terminal-engine-artifacts/bundle", architecture + "/libghostty-vt.a", accepted["libraries"][target], 64 * 1024 * 1024)
        receipt["bundle_bytes"] = sum(path.stat().st_size for path in bundle.rglob("*") if path.is_file())
        if receipt["bundle_bytes"] > MAX_EXPANDED:
            raise Rejected("retained linkage evidence exceeds upload bound")
        receipt["state"] = "complete"
        receipt["scope"] = "production shim/adapter and requested optional Session compile/link/inspection for both Apple targets; ARM64 executed, Intel not executed; original engines unchanged; no native app/default/typed-input/performance acceptance"
    except Exception as error:
        receipt.update(state="failed", error=f"{type(error).__name__}: {error}")
    finally:
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"state": receipt["state"], "receipt": str(output / "receipt.json")}))
    return 0 if receipt["state"] == "complete" else 1


if __name__ == "__main__":
    sys.exit(main())
