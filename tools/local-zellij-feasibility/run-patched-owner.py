#!/usr/bin/env python3
"""Bounded actual-owner gates for a verified experimental producer artifact."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
from owner_contract import source_hashes, verify_logs

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
spec = importlib.util.spec_from_file_location("zmux_patched_producer", HERE / "produce-patched.py")
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer-receipt", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--cargo-target", type=Path, help="explicit absolute owned Cargo cache")
    args = parser.parse_args()
    if args.cargo_target is not None and not args.cargo_target.is_absolute():
        parser.error("cargo-target must be an absolute owned cache path")
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    target = args.cargo_target or output / "cargo-target"
    phases = []
    receipt = {"status": "failed", "host": platform.platform(), "phases": phases,
               "scope": "actual foreground CLI owner only; no native integration or acquisition change",
               "cargo_target": str(target)}
    try:
        artifact_receipt = args.producer_receipt.resolve()
        artifact = json.loads(artifact_receipt.read_text())
        if artifact["status"] != "passed" or artifact["lock"] != json.loads((HERE / "patched-producer-lock.json").read_text()):
            raise RuntimeError("producer did not pass with the current pinned source contract")
        inputs = {"producer_sha256": HERE / "produce-patched.py",
                  "patch_sha256": HERE / "patches/0001-retry-interrupted-stdin.patch",
                  "fixture_sha256": HERE / "stdin-eintr.rs",
                  "shared_runner_sha256": ROOT / "tools/terminal-feasibility/run.py",
                  "process_group_sha256": ROOT / "scripts/process_group.py",
                  "package_build_sha256": HERE / "package_build.py",
                  "wasm_validator_sha256": HERE / "validate-wasm.rs"}
        for key, path in inputs.items():
            if producer.digest(path) != artifact[key]:
                raise RuntimeError(f"producer input no longer matches: {path}")
        binary = artifact_receipt.parent / "artifacts/zellij"
        if Path(artifact["binary_path"]).resolve() != binary or producer.digest(binary) != artifact["binary_sha256"]:
            raise RuntimeError("retained producer executable does not match its receipt")
        if artifact["binary_version"] != "zellij 0.43.0":
            raise RuntimeError("retained producer version changed")
        inputs["owner_contract_sha256"] = HERE / "owner_contract.py"
        sources = source_hashes(ROOT, producer.digest)
        receipt["source_inputs_sha256"] = sources
        receipt.update(producer_receipt_sha256=producer.digest(artifact_receipt),
                       binary_sha256=artifact["binary_sha256"], binary_path=str(binary),
                       harness_sha256=producer.digest(Path(__file__)),
                       inputs_sha256={str(path.relative_to(ROOT)): producer.digest(path) for path in inputs.values()})
        runner = producer.bounded_runner
        runner.run(["git", "rev-parse", "HEAD"], "source", output, phases, timeout=10, cwd=ROOT)
        receipt["source_sha"] = (output / "source.log").read_text().strip()
        runner.run(["rustc", "+1.95.0", "--verbose", "--version"], "rustc", output, phases, timeout=30, cwd=ROOT)
        environment = dict(os.environ, ZMUX_TEST_ZELLIJ_BIN=str(binary), CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS="2")
        checks = [
            ["cargo", "+1.95.0", "test", "--locked", "-p", "zmux-session-runtime", "--test", "child", "--test", "lifecycle", "--test", "shutdown", "--", "--nocapture"],
            ["cargo", "+1.95.0", "test", "--locked", "-p", "zmux-remote-runtime", "--lib", "local_zellij", "--", "--nocapture"],
            ["cargo", "+1.95.0", "test", "--locked", "-p", "zmux", "--features", "local-zellij-conformance", "--test", "local_zellij", "--test", "local_zellij_owner", "--", "--nocapture"],
        ]
        for index, command in enumerate(checks):
            runner.run(command, f"owner-{index}", output, phases, timeout=300, cwd=ROOT, env=environment)
            if any(phase["log_bytes_seen"] != phase["log_bytes_retained"] for phase in phases):
                raise RuntimeError("owner gate output exceeded its retained byte bound")
        if source_hashes(ROOT, producer.digest) != sources:
            raise RuntimeError("owner source changed during execution")
        receipt["executed_owner_contract"] = verify_logs(output)
        receipt["status"] = "passed"
    except Exception as error:
        receipt["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        for phase in phases:
            phase["log_sha256"] = producer.digest(output / phase["log"])
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"status": receipt["status"], "receipt": str(output / "receipt.json")}, indent=2))


if __name__ == "__main__":
    main()
