#!/usr/bin/env python3
"""Focused archive/owner controls using one real readiness package."""
import argparse
import copy
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("readiness_package_emitter", HERE / "package-patched.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
from readiness_package import verify_archive
from package_admission import archive_files
from owner_contract import verify_logs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer-receipt", type=Path, required=True)
    parser.add_argument("--owner-receipt", type=Path, required=True)
    parser.add_argument("--readiness-owner-receipt", type=Path, required=True)
    parser.add_argument("--package-output", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--promotion-ready", action="store_true", help="require the finite diagnostic retained variant instead of prepare-only")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    built, _, _, _ = package.verify_chain(args.producer_receipt.resolve(), args.owner_receipt.resolve(),
                                         args.readiness_owner_receipt.resolve())
    emitted = package.read_json(args.package_output / "receipt.json")
    manifest = package.read_json(args.package_output / "payload/manifest.json")
    archive = Path(emitted["archive"])
    if emitted["status"] != "passed" or emitted["promotion_ready"] is not args.promotion_ready or package.producer.digest(archive) != emitted["archive_sha256"]:
        raise RuntimeError("real readiness package with explicit expected promotion state required")
    verify_archive(archive, manifest, built, package.producer.digest, promotion_ready=args.promotion_ready)
    results = []

    def rejected(label, function, reason):
        try:
            function()
        except RuntimeError as error:
            if reason not in str(error):
                raise RuntimeError(f"{label} failed at an unrelated gate: {error}") from error
            results.append({"case": label, "rejected": True, "reason": str(error)})
        else:
            raise RuntimeError(label + " admitted invalid readiness package evidence")

    rejected("missing actual readiness owner", lambda: package.verify_chain(
        args.producer_receipt.resolve(), args.owner_receipt.resolve()), "actual readiness owner receipt required")
    ready_directory = output / "readiness-owner"
    ready_directory.mkdir()
    for path in args.readiness_owner_receipt.parent.glob("*.log"):
        shutil.copy2(path, ready_directory / path.name)
    original_ready = package.read_json(args.readiness_owner_receipt)
    ready_receipt = ready_directory / "receipt.json"
    ready_receipt.write_text(json.dumps(original_ready))
    package.verify_chain(args.producer_receipt.resolve(), args.owner_receipt.resolve(), ready_receipt)
    changed = copy.deepcopy(original_ready)
    changed["producer_receipt_sha256"] = "0" * 64
    ready_receipt.write_text(json.dumps(changed))
    rejected("different readiness executable producer", lambda: package.verify_chain(
        args.producer_receipt.resolve(), args.owner_receipt.resolve(), ready_receipt), "did not test this exact producer executable")
    changed = copy.deepcopy(original_ready)
    changed["attachments"][0]["reporter_eof"] = False
    ready_receipt.write_text(json.dumps(changed))
    rejected("missing accepted EOF semantic proof", lambda: package.verify_chain(
        args.producer_receipt.resolve(), args.owner_receipt.resolve(), ready_receipt), "actual readiness/EOF/one-write/cleanup evidence differs")

    owner_directory = output / "public-owner"
    owner_directory.mkdir()
    for path in args.owner_receipt.parent.glob("*.log"):
        shutil.copy2(path, owner_directory / path.name)
    original_owner = package.read_json(args.owner_receipt)
    owner_receipt = owner_directory / "receipt.json"
    baseline = (owner_directory / "owner-2.log").read_text()
    for case in ("missing-named-observer4", "transport-progress-instead-of-ready"):
        if case == "missing-named-observer4":
            changed_log = baseline.replace("native::actual_typed_input_ready_delivers_one_silent_first_line_per_attachment",
                                          "native::unrelated_same_count_behavior")
            expected = "named observer4 consumer behaviors did not execute"
        else:
            changed_log = baseline.replace('"input_admission":"typed_input_ready"', '"input_admission":"transport_connected"')
            expected = "actual silent observer4 admission evidence differs"
        if changed_log == baseline:
            raise RuntimeError("real observer4 positive control marker missing")
        (owner_directory / "owner-2.log").write_text(changed_log)
        changed_owner = copy.deepcopy(original_owner)
        for phase in changed_owner["phases"]:
            if phase["label"] == "owner-2":
                phase["log_sha256"] = package.producer.digest(owner_directory / phase["log"])
                phase["log_bytes_seen"] = phase["log_bytes_retained"] = (owner_directory / phase["log"]).stat().st_size
        changed_owner["executed_owner_contract"] = verify_logs(owner_directory)
        owner_receipt.write_text(json.dumps(changed_owner))
        rejected(case, lambda: package.verify_chain(args.producer_receipt.resolve(), owner_receipt,
                 args.readiness_owner_receipt.resolve()), expected)

    # Exercise the real public packager refusal using the same actual producer,
    # before any platform/package work. It must not infer review from the input.
    result = subprocess.run([sys.executable, "-B", str(HERE / "package-patched.py"),
        "--producer-receipt", str(args.producer_receipt.resolve()),
        "--owner-receipt", str(args.owner_receipt.resolve()),
        "--readiness-owner-receipt", str(args.readiness_owner_receipt.resolve()),
        "--reviewed-policy-sha256", package.producer.digest(HERE / "notice-policy.json"),
        "--reviewed-readiness-source-sha256", "0" * 64,
        "--output", str(output / "unreviewed-promotion")], capture_output=True, text=True, timeout=60)
    refusal_log = output / "unreviewed-retention.log"
    refusal_log.write_text(result.stdout + result.stderr)
    if result.returncode == 0 or "requires the exact reviewed source inventory SHA256" not in result.stderr:
        raise RuntimeError(f"public packager did not refuse unreviewed readiness retention: status {result.returncode}: {result.stderr[-4096:]}")
    results.append({"case": "unreviewed readiness retention", "rejected": True,
                    "reason": "public packager rejected different reviewed source inventory",
                    "exit_code": result.returncode, "log": refusal_log.name,
                    "log_sha256": package.producer.digest(refusal_log)})

    original_files = archive_files(archive.read_bytes())
    for case in ("missing-reporter", "changed-protocol", "wrong-variant", "wrong-promotion-state"):
        files, changed_manifest = dict(original_files), copy.deepcopy(manifest)
        if case == "missing-reporter":
            name = "SOURCES/input-readiness-v1/reporter.rs"
            del files[name]
            del changed_manifest["files"][name]
            expected = "corresponding source missing"
        elif case == "changed-protocol":
            name = "SOURCES/input-readiness-v1/protocol.json"
            files[name] += b"\n"
            changed_manifest["files"][name] = {"bytes": len(files[name]), "sha256": hashlib.sha256(files[name]).hexdigest()}
            expected = "corresponding source differs"
        elif case == "wrong-promotion-state":
            changed_manifest["promotion_ready"] = not args.promotion_ready
            expected = "archive manifest or promotion state differs"
        else:
            changed_manifest["variant"] = "legacy"
            expected = "archive variant differs"
        files["manifest.json"] = (json.dumps(changed_manifest, indent=2) + "\n").encode()
        path = output / (case + ".tar.gz")
        with path.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped, tarfile.open(fileobj=zipped, mode="w|") as stream:
            for name, data in sorted(files.items()):
                member = tarfile.TarInfo(name)
                member.size = len(data)
                stream.addfile(member, io.BytesIO(data))
        rejected(case, lambda: verify_archive(path, changed_manifest, built, package.producer.digest, promotion_ready=args.promotion_ready), expected)
    (output / "receipt.json").write_text(json.dumps({"status": "passed", "positive_control": "real readiness archive and both actual owners", "promotion_ready": args.promotion_ready, "negative_controls": results}, indent=2) + "\n")


if __name__ == "__main__":
    main()
