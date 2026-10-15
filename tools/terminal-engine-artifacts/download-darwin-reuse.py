#!/usr/bin/env python3
"""Download one immutable failed-run artifact; preserve its original receipt."""

import argparse
import json
from pathlib import Path
import subprocess

from verify import Rejected, checked_file, document

REPOSITORY = "ashutoshpw/zmux"
RUN = 38797303343
ARTIFACT = 12002284112
SOURCE = "adf51320a80e10db85bd8c9b2c01e1e42d268dbd"
NAME = "terminal-engine-darwin-thin-" + SOURCE
ARCHIVE_DIGEST = "sha256:5597a041c9b1bb3fef7fa32122ea26304a0bff10c4e4baf5cb86b0822ec4d4b7"
RECEIPT_SHA256 = "75bd83078d904c5d91582b0d65f6e213bee48f9c528cf4670d170239f64d2354"
LIBRARY_SHA256 = "df2aa776d0637f13e6641ac443695c9ea4cd592416b9b1a35bf2e73be9c7e658"


def verify_retained(root):
    checked_file(root, "receipt.json", RECEIPT_SHA256)
    receipt = document(root / "receipt.json")
    expected = {"path": "bundle/arm64/libghostty-vt.a", "sha256": LIBRARY_SHA256}
    if (receipt["source_sha"] != SOURCE or len(receipt["targets"]) != 1
            or receipt["targets"][0]["architecture"] != "arm64"
            or receipt["targets"][0]["library"] != expected):
        raise Rejected("pinned failed-run receipt has an unexpected source or slice")
    checked_file(root, expected["path"], LIBRARY_SHA256, 64 * 1024 * 1024)
    return {"run": RUN, "artifact": ARTIFACT, "source_sha": SOURCE,
            "receipt_sha256": RECEIPT_SHA256, "arm64_library_sha256": LIBRARY_SHA256,
            "scope": "download identity only; driver must independently match every compiler recipe, toolchain, patch and header input"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify-only", type=Path)
    args = parser.parse_args()
    if args.verify_only:
        print(json.dumps(verify_retained(args.verify_only.resolve()), indent=2))
        return
    if args.output is None:
        parser.error("--output or --verify-only is required")
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    result = subprocess.run(["gh", "api", f"repos/{REPOSITORY}/actions/artifacts/{ARTIFACT}"],
                            check=True, capture_output=True, timeout=30)
    if len(result.stdout) > 64 * 1024:
        raise Rejected("artifact metadata exceeds 64 KiB")
    metadata = json.loads(result.stdout)
    if (metadata["id"] != ARTIFACT or metadata["name"] != NAME or metadata["expired"]
            or metadata["digest"] != ARCHIVE_DIGEST
            or metadata["workflow_run"]["id"] != RUN
            or metadata["workflow_run"]["head_sha"] != SOURCE
            or not 0 < metadata["size_in_bytes"] <= 8 * 1024 * 1024):
        raise Rejected("immutable prior artifact identity/size changed or expired; no rebuild fallback")
    subprocess.run(["gh", "run", "download", str(RUN), "--repo", REPOSITORY,
                    "--name", NAME, "--dir", str(root)], check=True, timeout=120)
    files = [p for p in root.rglob("*") if p.is_file()]
    if len(files) > 512 or sum(p.stat().st_size for p in files) > 256 * 1024 * 1024:
        raise Rejected("downloaded bundle exceeds retained file/byte bounds")
    evidence = verify_retained(root)
    (root / "download-identity.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()
