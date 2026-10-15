#!/usr/bin/env python3
"""Adversarial package admission checks using real successful execution evidence."""
import argparse
import importlib.util
import json
import io
import tarfile
from pathlib import Path
import shutil
import sys

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("zmux_package", HERE / "package-patched.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
from owner_contract import verify_logs
from package_notices import registry_component
from package_sources import collect_source_bundle, verify_source_bundle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer-receipt", required=True, type=Path)
    parser.add_argument("--owner-receipt", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    original_producer = args.producer_receipt.resolve()
    original_owner = args.owner_receipt.resolve()
    # Establish the positive control before any negative input mutations.
    built, owner, _, _ = package.verify_chain(original_producer, original_owner)
    results = []

    def rejected(label, function, reason):
        try:
            function()
        except RuntimeError as error:
            if reason not in str(error):
                raise RuntimeError(f"{label} rejected for unrelated reason: {error}") from error
            results.append({"case": label, "rejected": True, "reason": str(error)})
        else:
            raise RuntimeError(f"{label} admitted invalid package evidence")

    owner_copy = output / "owner"
    owner_copy.mkdir()
    for path in original_owner.parent.glob("*.log"):
        shutil.copy2(path, owner_copy / path.name)
    baseline = (owner_copy / "owner-0.log").read_text()
    (owner_copy / "owner-0.log").write_text(baseline.replace("test input_and_resize_reach_the_real_terminal ... ok", "test replacement_unrelated_behavior ... ok"))
    rejected("same-count missing baseline behavior", lambda: verify_logs(owner_copy), "required owner behavior")
    (owner_copy / "owner-0.log").write_text(baseline)
    journey = (owner_copy / "owner-2.log").read_text()
    (owner_copy / "owner-2.log").write_text(journey.replace('"actual_input":true', '"actual_input":false'))
    rejected("missing subsequent input journey", lambda: verify_logs(owner_copy), "actual owner journey")

    def policy_rejected(label, value):
        directory = output / label
        command = [sys.executable, "-B", HERE / "package-patched.py", "--producer-receipt", original_producer,
                   "--owner-receipt", original_owner, "--output", directory]
        if value is not None:
            command += ["--reviewed-policy-sha256", value]
        phases = []
        try:
            package.producer.bounded_runner.run(command, label, output, phases, timeout=30)
        except RuntimeError:
            receipt = json.loads((directory / "receipt.json").read_text())
            if receipt["status"] != "failed" or receipt["promotion_ready"] or "exact independently reviewed" not in receipt["error"] or list(directory.glob("*.tar.gz")):
                raise RuntimeError("policy failure did not prevent a promotable archive")
            results.append({"case": label, "rejected": True, "reason": receipt["error"]})
        else:
            raise RuntimeError("unreviewed policy admitted")

    policy_rejected("missing-policy-review", None)
    policy_rejected("mismatched-policy-review", "0" * 64)

    # Clone small evidence, symlinking large immutable source/binary products.
    # The positive control is re-established after path relocation, so later
    # mutations must reach their intended gate rather than fail on fixture paths.
    clone = output / "producer"
    clone.mkdir()
    for path in original_producer.parent.iterdir():
        target = clone / path.name
        if path.is_file() and path.suffix in (".json", ".log"):
            shutil.copy2(path, target)
        else:
            target.symlink_to(path, target_is_directory=path.is_dir())
    built["binary_path"] = str(clone / "artifacts/zellij")
    source = clone / "source" / ("zellij-" + built["lock"]["source_commit"])
    built["cargo_config_sha256"] = {str(source / ".cargo/config.toml"): package.producer.digest(source / ".cargo/config.toml")}
    clone_receipt = clone / "receipt.json"
    clone_receipt.write_text(json.dumps(built))
    owner["producer_receipt_sha256"] = package.producer.digest(clone_receipt)
    # Restore authentic logs after independent contract mutations above.
    shutil.copy2(original_owner.parent / "owner-2.log", owner_copy / "owner-2.log")
    copied_owner = owner_copy / "receipt.json"
    copied_owner.write_text(json.dumps(owner))
    package.verify_chain(clone_receipt, copied_owner)
    original = json.loads(clone_receipt.read_text())
    built["status"] = "failed"
    clone_receipt.write_text(json.dumps(built))
    rejected("failed producer", lambda: package.verify_chain(clone_receipt, copied_owner), "successful source-rebuilt")
    built = original
    clone_receipt.write_text(json.dumps(built))
    with (clone / "native-compiler-units.json").open("a") as stream:
        stream.write("\n")
    rejected("altered compiler evidence", lambda: package.verify_chain(clone_receipt, copied_owner), "compiler-unit evidence changed")
    shutil.copy2(original_producer.parent / "native-compiler-units.json", clone / "native-compiler-units.json")
    with (owner_copy / "owner-0.log").open("a") as stream:
        stream.write("altered\n")
    rejected("altered owner evidence", lambda: package.verify_chain(clone_receipt, copied_owner), "actual owner phase failed or evidence changed")
    shutil.copy2(original_owner.parent / "owner-0.log", owner_copy / "owner-0.log")

    metadata = json.loads((original_producer.parent / "dependency-metadata.json").read_text())
    component = next(row for row in metadata["packages"] if row["name"] == "pretty-bytes" and row["version"] == "0.2.2")
    policy = json.loads((HERE / "notice-policy.json").read_text())
    checksum = policy["missing_text_overrides"].pop("pretty-bytes@0.2.2")["archive_sha256"]
    rejected("missing notice supplement", lambda: registry_component(component, checksum, output / "missing-notice", policy, package.producer.digest), "no reviewed notice text")

    assets_path = clone / "embedded-assets-manifest.json"
    assets = json.loads(assets_path.read_text())
    assets["assets"][0]["rebuilt_sha256"] = "0" * 64
    assets_path.write_text(json.dumps(assets))
    built["plugins"]["manifest_sha256"] = package.producer.digest(assets_path)
    clone_receipt.write_text(json.dumps(built))
    owner["producer_receipt_sha256"] = package.producer.digest(clone_receipt)
    copied_owner.write_text(json.dumps(owner))
    phases = []
    try:
        package.producer.bounded_runner.run([sys.executable, "-B", HERE / "package-patched.py", "--producer-receipt", clone_receipt,
            "--owner-receipt", copied_owner, "--output", output / "stale-embedded", "--prepare-only"], "stale-embedded", output, phases, timeout=30)
    except RuntimeError:
        receipt = json.loads((output / "stale-embedded/receipt.json").read_text())
        if "embedded stale/unproven plugin bytes" not in receipt["error"]:
            raise RuntimeError(f"stale embedded bytes rejected for unrelated reason: {receipt}")
        results.append({"case": "stale embedded plugin bytes", "rejected": True, "reason": receipt["error"]})
    else:
        raise RuntimeError("stale embedded plugin hash admitted")
    fixture_component = next(row for row in metadata["packages"] if row["name"] == "wit-parser" and row["version"] == "0.221.3")
    fixture_policy = json.loads((HERE / "notice-policy.json").read_text())
    fixture_checksum = fixture_policy["registry_source_compiled_exclusions"].pop("wit-parser@0.221.3")["archive_sha256"]
    rejected("unreviewed registry WASM fixture", lambda: registry_component(fixture_component, fixture_checksum, output / "unreviewed-registry-wasm", fixture_policy, package.producer.digest), "unreviewed prebuilt binary in registry source archive")
    source_destination = output / "source-gates"
    source_bundle = collect_source_bundle(original_producer.parent, source_destination, built["lock"], package.producer.digest)
    expected_assets = json.loads((original_producer.parent / "prebuilt-assets-manifest.json").read_text())["assets"]
    sources = source_destination / "SOURCES"
    bundle = sources / "zellij-source.tar.gz"
    proof_path = sources / "upstream-source-provenance.json"
    proof = json.loads(proof_path.read_text())
    backup = output / "filtered-source-control.tar.gz"
    shutil.copy2(bundle, backup)

    def replace_source(skip_lock=False, add_old_wasm=False):
        with tarfile.open(backup) as original, tarfile.open(bundle, "w:gz") as changed:
            for member in original.getmembers():
                if skip_lock and member.name.endswith("/Cargo.lock"):
                    continue
                changed.addfile(member, original.extractfile(member))
            if add_old_wasm:
                with tarfile.open(original_producer.parent / "source.tar.gz") as upstream:
                    member = next(item for item in upstream.getmembers() if item.isfile() and item.name.endswith(".wasm"))
                    raw = upstream.extractfile(member).read()
                # A disguised filename must not evade the byte-based WASM guard.
                info = tarfile.TarInfo("zellij-" + built["lock"]["source_commit"] + "/old-plugin.bin")
                info.size = len(raw)
                changed.addfile(info, io.BytesIO(raw))
        proof["filtered_archive_sha256"] = package.producer.digest(bundle)
        proof_path.write_text(json.dumps(proof))

    replace_source(add_old_wasm=True)
    rejected("old WASM hidden in source archive", lambda: verify_source_bundle(source_destination, built["lock"], expected_assets, package.producer.digest), "forbidden prebuilt component bytes")
    replace_source(skip_lock=True)
    rejected("filtered source lost Cargo.lock", lambda: verify_source_bundle(source_destination, built["lock"], expected_assets, package.producer.digest), "lost or changed retained source members")
    shutil.copy2(backup, bundle)
    proof["filtered_archive_sha256"] = package.producer.digest(bundle)
    proof["excluded_prebuilt_wasm"][0]["sha256"] = "0" * 64
    proof_path.write_text(json.dumps(proof))
    rejected("altered excluded-member provenance", lambda: verify_source_bundle(source_destination, built["lock"], expected_assets, package.producer.digest), "excluded-member inventory differs")
    receipt = {"status": "passed", "positive_control": "real exact producer/owner chain", "negative_controls": results}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
