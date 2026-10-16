#!/usr/bin/env python3
"""Build the explicit host-only bundle and exercise its unchanged retained dependency."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import sys

import native_legacy_owner
from native_bundle_readiness import check_inputs as check_native_inputs, run_relocated

from owner_contract import owner_checks, source_hashes, verify_logs
from package_admission import (VARIANTS, MAX_ARCHIVE, Rejected, accept, digest, native_target,
                               pin, read_file, unpack_download, verify_download, verify_payload)

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def inputs():
    result = source_hashes(ROOT, lambda path: digest(read_file(path, 8 * 1024 * 1024)))
    paths = {ROOT / name for name in ("scripts/build-macos-dev", "scripts/process_group.py", "tools/terminal-feasibility/run.py", "rust-toolchain.toml",
                                     "LICENSE", "THIRD_PARTY_NOTICES.md", ".github/workflows/local-zellij-native-package.yml",
                                     "crates/native-bridge/include/zmux.h",
                                     "apps/cli/contracts/baseline/command-inventory.json",
                                     "apps/cli/contracts/baseline/audit-reconciliation.json",
                                     "apps/cli/src/registry/frozen-coverage.json")}
    for directory in (ROOT / "apps/macos", ROOT / "config", HERE):
        paths.update(path for path in directory.rglob("*") if path.is_file() and "__pycache__" not in path.parts)
    if len(paths) > 3000:
        raise Rejected("native source inventory exceeds bound")
    result.update({str(path.relative_to(ROOT)): digest(read_file(path, 8 * 1024 * 1024)) for path in sorted(paths)})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variant", choices=VARIANTS, default="legacy")
    parser.add_argument("--legacy-owner-root", type=Path, default=ROOT / ".diagnostic-legacy-owner")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check-inputs", action="store_true")
    args = parser.parse_args()
    accepted = pin(args.variant)
    check_native_inputs()
    if args.check_inputs:
        print(json.dumps({"retained_package": accepted, "legacy_owner": native_legacy_owner.retained()[0] if args.variant == "legacy" else None, "native_execution": False}))
        return 0
    if args.output is None or not args.output.is_absolute():
        parser.error("--output must be an absolute fresh directory")
    try:
        native_target("host", platform.system(), platform.machine())
    except Rejected as error:
        parser.error(str(error))
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    receipt = {"schema_version": 1, "status": "failed", "phases": [],
               "claims": {"dependency_rebuilt": False, "default_acquisition_changed": False,
                          "native_start_attach_integrated": False, "intel_or_universal_accepted": False,
                          "minimum_os_runtime_accepted": False}}
    spec = importlib.util.spec_from_file_location("native_package_runner", ROOT / "tools/terminal-feasibility/run.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("DYLD_") and key not in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CFLAGS", "LDFLAGS")}
    environment.update(CARGO_BUILD_JOBS="2", RUSTUP_TOOLCHAIN="1.95.0", CARGO_TARGET_DIR=str(output / "cargo-target"))

    def run(command, label, timeout=30, *, cwd=ROOT, env=None):
        text = runner.run(command, label, output, receipt["phases"], timeout=timeout, cwd=cwd, env=environment if env is None else env)
        phase = receipt["phases"][-1]
        if phase["log_bytes_seen"] != phase["log_bytes_retained"]:
            raise Rejected("native package phase output truncated: " + label)
        return text.strip()

    try:
        frozen = inputs()
        receipt["source_inputs_sha256"] = frozen
        total = 0
        for name, wanted in frozen.items():
            raw = read_file(ROOT / name, 8 * 1024 * 1024)
            total += len(raw)
            if total > 32 * 1024 * 1024 or digest(raw) != wanted:
                raise Rejected("native retained source changed or exceeds byte bound")
            destination = output / "source" / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open("xb") as stream:
                stream.write(raw)
        receipt["source_sha"] = run(["git", "rev-parse", "HEAD"], "source")
        receipt["host"] = platform.platform()
        endpoint = f"repos/{accepted['repository']}/actions/artifacts/{accepted['download']['artifact_id']}"
        metadata = json.loads(run(["gh", "api", endpoint], "download-metadata"))
        verify_download(metadata, accepted)
        receipt["download_identity"] = metadata
        # Retain binary stdout separately; gh warnings remain bounded diagnostic
        # stderr, never ZIP bytes or a large decoded Unicode return value.
        archive = output / "retained-package.zip"
        bound = accepted["download"]["zip_bytes"]
        capture = HERE / "capture-stdout.py"
        receipt["download_capture"] = {"helper_sha256": digest(read_file(capture, 65536)),
                                       "stdout_path": archive.name, "stdout_bound": bound}
        if receipt["download_capture"]["helper_sha256"] != frozen[str(capture.relative_to(ROOT))]:
            raise Rejected("binary capture helper changed before download")
        run([sys.executable, "-B", str(capture), "--output", str(archive), "--max-bytes", str(bound),
             "--", "gh", "api", endpoint + "/zip"], "download-package", 120)
        receipt["download_capture"].update(stdout_bytes=archive.stat().st_size,
                                           stdout_sha256=digest(read_file(archive, MAX_ARCHIVE)))
        if (receipt["download_capture"]["stdout_bytes"] != bound
                or receipt["download_capture"]["stdout_sha256"] != accepted["download"]["zip_sha256"]):
            raise Rejected("binary capture does not match exact retained ZIP accounting")
        package_root = output / "retained-package"
        receipt["admission"] = unpack_download(archive, package_root, accepted)
        run([sys.executable, "-B", "scripts/build-macos-dev", "--architecture", "host", "--with-zellij",
             "--zellij-package-root", str(package_root), "--zellij-variant", args.variant], "build-native-package", 900)
        dependency = ROOT / "dist/zmux.app/Contents/Resources/Dependencies/local-zellij"
        if verify_payload(dependency, args.variant) != accept(package_root, accepted["target"], variant=args.variant):
            raise Rejected("final bundled dependency differs from admitted package")
        binary = dependency / "bin/zellij"
        run(["codesign", "--verify", "--strict", "--verbose=2", str(binary)], "final-dependency-signature")
        if run([str(binary), "--version"], "final-dependency-version") != "zellij 0.43.0":
            raise Rejected("final bundled dependency version differs")
        build_info = json.loads(read_file(ROOT / "dist/build-info.json", 65536))
        if (build_info["source_sha"] != receipt["source_sha"] or build_info["architectures"] != ["arm64"]
                or build_info["optional_zellij_packaged"] is not True
                or build_info["optional_zellij"]["binary_sha256"] != accepted["binary_sha256"]
                or build_info["default_terminal_engine"] != "vt100"):
            raise Rejected("native build metadata differs from explicit package/default contract")
        receipt["build_info"] = build_info
        environment["ZMUX_TEST_ZELLIJ_BIN"] = str(binary)
        # Reuse the actual production-owner suites and their shared semantic
        # verifier. The CLI is Cargo-built; Zellij is the final app-bundled path.
        checks = owner_checks()
        owner_root = ROOT
        verifier = None
        if args.variant == "legacy":
            owner_root = args.legacy_owner_root.resolve(strict=True)
            receipt["legacy_owner"], verifier = native_legacy_owner.prepare(owner_root, output, run)
            # These are the original prior native commands. Its source predates
            # the reexec readiness fixture, so no skip argument is needed.
            checks[1] = checks[1][:checks[1].index("--skip")]
        for index, command in enumerate(checks):
            run(command, f"owner-{index}", 300, cwd=owner_root)
        receipt["executed_owner_contract"] = verifier.verify_logs(output) if verifier else verify_logs(output)
        if verifier:
            native_legacy_owner.verify_unchanged(owner_root, receipt["legacy_owner"])
        if args.variant == "input-readiness-v1":
            receipt["actual_appkit"] = run_relocated(ROOT / "dist/zmux.app", output, accepted,
                                                      receipt["source_sha"], run, environment)
            receipt["claims"]["native_start_attach_integrated"] = True
            receipt["claims"]["actual_relocated_AppKit_accepted"] = True
            dependency = output / "relocated with spaces/zmux.app/Contents/Resources/Dependencies/local-zellij"
        if inputs() != frozen or verify_payload(dependency, args.variant) != receipt["admission"]:
            raise Rejected("native source or bundled dependency changed during execution")
        archive = ROOT / "dist" / ("zmux-dev-arm64-" + receipt["source_sha"][:7] + ".zip")
        raw = read_file(archive, 256 * 1024 * 1024)
        shutil.copy2(archive, output / archive.name)
        receipt["app_archive"] = {"path": archive.name, "bytes": len(raw), "sha256": digest(raw)}
        receipt["status"] = "passed"
    except Exception as error:
        receipt["error"] = f"{type(error).__name__}: {error}"
    finally:
        for phase in receipt["phases"]:
            phase["sha256"] = digest(read_file(output / phase["log"], MAX_ARCHIVE))
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "receipt": str(output / "receipt.json")}))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
