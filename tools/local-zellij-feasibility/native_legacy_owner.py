"""Pinned compatible legacy consumer harness; never substitute current observer4."""
import importlib.util

from owner_contract import source_hashes
from package_admission import HERE, Rejected, digest, document, pin, read_file, safe_name


ADDITIONAL_INPUTS = ("apps/cli/contracts/baseline/command-inventory.json",
                     "apps/cli/contracts/baseline/audit-reconciliation.json",
                     "apps/cli/src/registry/frozen-coverage.json")


def retained():
    value = document(read_file(HERE / "retained-legacy-native-owner.json", 65536))
    raw = read_file(HERE / safe_name(value["receipt_path"]), 65536)
    prior = document(raw)
    accepted = pin("legacy")
    if (value["schema_version"] != 1 or digest(raw) != value["receipt_sha256"]
            or prior["status"] != "passed" or prior["source_sha"] != value["source_sha"]
            or any(prior["admission"][key] != accepted[key] for key in
                   ("run_id", "source_sha", "target", "archive_sha256", "manifest_sha256", "binary_sha256"))
            or prior["source_inputs_sha256"]["tools/local-zellij-feasibility/owner_contract.py"] != value["verifier_sha256"]
            or set(value["additional_inputs_sha256"]) != set(ADDITIONAL_INPUTS)):
        raise Rejected("legacy owner harness differs from its original passing native receipt")
    return value, prior


def prepare(root, output, execute):
    value, prior = retained()
    root = root.resolve(strict=True)
    if (execute(["git", "rev-parse", "HEAD"], "legacy-owner-head", cwd=root) != value["source_sha"]
            or execute(["git", "rev-parse", "HEAD^{tree}"], "legacy-owner-tree", cwd=root) != value["source_tree"]
            or execute(["git", "status", "--porcelain", "--untracked-files=all"], "legacy-owner-clean", cwd=root)):
        raise Rejected("legacy consumer requires its clean exact prior accepted source; no fallback")
    frozen = source_hashes(root, lambda path: digest(read_file(path, 8 * 1024 * 1024)))
    verifier = root / "tools/local-zellij-feasibility/owner_contract.py"
    frozen[str(verifier.relative_to(root))] = digest(read_file(verifier, 65536))
    if any(prior["source_inputs_sha256"].get(name) != wanted for name, wanted in frozen.items()):
        raise Rejected("legacy consumer input differs from original passing native receipt")
    original_count = len(frozen)
    # The old source is authenticated by exact clean Git HEAD/tree as well as
    # its retained receipt. Its three include_str JSON files were not listed in
    # that receipt, so retain them under separately reviewed Git-bound hashes.
    additional = {name: digest(read_file(root / name, 8 * 1024 * 1024)) for name in ADDITIONAL_INPUTS}
    if additional != value["additional_inputs_sha256"]:
        raise Rejected("legacy included JSON differs from its exact authenticated Git source")
    frozen.update(additional)
    total = 0
    for name, wanted in frozen.items():
        raw = read_file(root / name, 8 * 1024 * 1024)
        total += len(raw)
        if total > 16 * 1024 * 1024 or digest(raw) != wanted:
            raise Rejected("legacy retained harness source changed or exceeds bound")
        destination = output / "legacy-owner-source" / safe_name(name)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("xb") as stream:
            stream.write(raw)
    spec = importlib.util.spec_from_file_location("pinned_legacy_owner_contract", verifier)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return {"source_sha": value["source_sha"], "source_tree": value["source_tree"],
            "original_native_run_id": value["run_id"], "original_receipt_sha256": value["receipt_sha256"],
            "source_inputs_sha256": frozen, "original_receipt_matching_inputs": original_count,
            "additional_git_bound_inputs_sha256": additional, "no_native_tests_omitted": True}, module


def verify_unchanged(root, receipt):
    frozen = source_hashes(root, lambda path: digest(read_file(path, 8 * 1024 * 1024)))
    name = "tools/local-zellij-feasibility/owner_contract.py"
    frozen[name] = digest(read_file(root / name, 65536))
    frozen.update({name: digest(read_file(root / name, 8 * 1024 * 1024)) for name in ADDITIONAL_INPUTS})
    if frozen != receipt["source_inputs_sha256"]:
        raise Rejected("legacy consumer source changed during execution")
