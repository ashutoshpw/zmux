#!/usr/bin/env python3
"""Run production Rust owner gates with the existing checksum-pinned download."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    output = args.output.resolve()
    spec = importlib.util.spec_from_file_location("zellij_public_peer", HERE / "run.py")
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    pins = json.loads((HERE / "pins.json").read_text())
    key = platform.system() + "-" + platform.machine()
    fixture.require(key in pins["assets"], "Unsupported owner host: " + key)
    receipt = {"host": platform.platform(), "machine": platform.machine(),
               "macos": platform.mac_ver()[0] or None,
               "source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
               "rustc": subprocess.check_output(["rustc", "--version", "--verbose"], cwd=ROOT, text=True),
               "zellij_source_commit": pins["source_commit"], "checks": [], "status": "failed"}
    if platform.system() == "Darwin":
        receipt["sdk"] = subprocess.check_output(["xcrun", "--show-sdk-version"], text=True).strip()
    began = time.monotonic()
    try:
        binary, receipt["download"] = fixture.download_binary(pins["assets"][key], output)
        environment = dict(os.environ, ZMUX_TEST_ZELLIJ_BIN=str(binary))
        checks = [
            ["cargo", "test", "--locked", "-p", "zmux-session-runtime", "--test", "child", "--test", "lifecycle", "--", "--nocapture"],
            ["cargo", "test", "--locked", "-p", "zmux-remote-runtime", "--lib", "local_zellij", "--", "--nocapture"],
            ["cargo", "test", "--locked", "-p", "zmux", "--features", "local-zellij-conformance", "--test", "local_zellij", "--test", "local_zellij_owner", "--", "--nocapture"],
        ]
        for index, command in enumerate(checks):
            log = output / f"owner-{index}.log"
            with log.open("wb") as stream:
                result = subprocess.run(command, cwd=ROOT, env=environment, stdout=stream, stderr=subprocess.STDOUT, timeout=300)
            data = log.read_bytes()
            receipt["checks"].append({"command": command, "returncode": result.returncode,
                                      "log": log.name, "log_sha256": hashlib.sha256(data).hexdigest()})
            print(data.decode(errors="replace"), end="", flush=True)
            fixture.require(result.returncode == 0, "Production owner gate failed: " + log.name)
        receipt["status"] = "passed"
    finally:
        receipt["elapsed_seconds"] = round(time.monotonic() - began, 3)
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
