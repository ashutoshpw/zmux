"""Explicit historical and versioned Ghostty artifact candidate routes."""

import json
from pathlib import PurePosixPath

from verify import (HERE, REPO, Rejected, checked_file, document, digest,
                    read_bounded)


VERSIONED_7501 = "ghostty-7501-v1"
HISTORICAL = "historical"

SOURCE_MANIFESTS = {
    HISTORICAL: HERE / "candidate.json",
    VERSIONED_7501: HERE / "candidate-sets" / VERSIONED_7501 / "source.json",
}
LINKAGE_MANIFESTS = {
    HISTORICAL: HERE / "linkage-candidates.json",
    VERSIONED_7501: HERE / "candidate-sets" / VERSIONED_7501 / "linkage.json",
}


def _path(registry, candidate_set, label):
    try:
        return registry[candidate_set]
    except (KeyError, TypeError) as error:
        raise Rejected(f"unsupported {label} candidate set: {candidate_set!r}") from error


def source_manifest_path(candidate_set=HISTORICAL):
    return _path(SOURCE_MANIFESTS, candidate_set, "source")


def linkage_manifest_path(candidate_set=HISTORICAL):
    return _path(LINKAGE_MANIFESTS, candidate_set, "linkage")


def load_source_manifest(candidate_set=HISTORICAL):
    path = source_manifest_path(candidate_set)
    try:
        manifest = document(path)
    except FileNotFoundError as error:
        if candidate_set != HISTORICAL:
            raise Rejected("versioned source candidate manifest is not available") from error
        raise
    if candidate_set != HISTORICAL and (
        manifest.get("candidate_set") != candidate_set
        or manifest.get("candidate_kind") != "source"
    ):
        raise Rejected("source candidate manifest identity differs")
    return path, manifest


def source_manifest_repo_path(candidate_set):
    try:
        return source_manifest_path(candidate_set).relative_to(REPO).as_posix()
    except ValueError as error:
        raise Rejected("source candidate manifest escaped the repository") from error


def load_linkage_manifest(candidate_set=HISTORICAL):
    path = linkage_manifest_path(candidate_set)
    try:
        manifest = document(path)
    except FileNotFoundError as error:
        if candidate_set != HISTORICAL:
            raise Rejected("versioned linkage candidate is not accepted yet") from error
        raise
    if candidate_set != HISTORICAL and (
        manifest.get("candidate_set") != candidate_set
        or manifest.get("candidate_kind") != "accepted_linkage"
    ):
        raise Rejected("linkage candidate manifest identity/state differs")
    return path, manifest


def validate_source_manifest(candidate_set, manifest):
    """Validate the versioned source-only manifest's explicit input contract."""
    if candidate_set == HISTORICAL:
        return
    if candidate_set != VERSIONED_7501 or manifest.get("candidate_set") != candidate_set:
        raise Rejected("unsupported source candidate identity")
    if not is_schema_version_one(manifest.get("schema_version")):
        raise Rejected("unsupported versioned source candidate schema")
    if manifest.get("candidate_kind") != "source":
        raise Rejected("versioned candidate is not a source manifest")
    if not _is_sha256(manifest.get("base_candidate_sha256")):
        raise Rejected("versioned source candidate lacks its historical baseline pin")
    if not _is_sha256(manifest.get("upstream_inputs_sha256")):
        raise Rejected("versioned source candidate lacks its exact upstream input pin")
    if not _is_sha256(manifest.get("recipe_sha256")):
        raise Rejected("versioned source candidate lacks its exact Darwin recipe pin")
    inputs = manifest.get("source_inputs_sha256")
    patches = manifest.get("patches")
    headers = manifest.get("post_patch_headers_sha256")
    if not isinstance(inputs, dict) or not inputs or not isinstance(patches, list) or not patches:
        raise Rejected("versioned source candidate input inventory is incomplete")
    if not isinstance(headers, dict) or not headers:
        raise Rejected("versioned source candidate lacks patched header pins")
    for name, expected in inputs.items():
        if not isinstance(name, str) or not name or not _is_sha256(expected):
            raise Rejected("invalid versioned source input pin")
    seen = set()
    expected_patch_order = [
        "tools/terminal-feasibility/resource.patch",
        "tools/terminal-feasibility/metadata.patch",
        "tools/terminal-feasibility/program-status-v0.4.patch",
    ]
    for patch in patches:
        if (not isinstance(patch, dict) or not isinstance(patch.get("path"), str)
                or not _is_sha256(patch.get("sha256"))
                or patch["path"] in seen):
            raise Rejected("invalid or duplicate versioned source patch pin")
        seen.add(patch["path"])
        if inputs.get(patch["path"]) != patch["sha256"]:
            raise Rejected("versioned patch pin differs from its source input pin")
    if [patch["path"] for patch in patches] != expected_patch_order:
        raise Rejected("versioned source candidate patch order differs")
    for name, expected in headers.items():
        if not _is_canonical_header_path(name) or not _is_sha256(expected):
            raise Rejected("invalid versioned post-patch header pin")


def validate_source_candidate_files(candidate_set, manifest, repository_root=REPO):
    """Verify the source manifest and all repository files it pins."""
    validate_source_manifest(candidate_set, manifest)
    checked_file(repository_root, "tools/terminal-engine-artifacts/candidate.json",
                 manifest["base_candidate_sha256"])
    checked_file(repository_root, "tools/terminal-engine-artifacts/upstream-inputs.json",
                 manifest["upstream_inputs_sha256"])
    checked_file(repository_root, "tools/terminal-engine-artifacts/darwin-recipe.json",
                 manifest["recipe_sha256"])
    for name, expected in manifest["source_inputs_sha256"].items():
        checked_file(repository_root, name, expected)


def validate_versioned_linkage(candidate_set, manifest, producer_receipt, *,
                               source_manifest, source_manifest_sha256, target=None,
                               library_sha256=None, producer_sha256=None):
    """Bind a reviewed output record to its source manifest and producer recipe."""
    if candidate_set == HISTORICAL:
        return
    if candidate_set != VERSIONED_7501 or manifest.get("candidate_set") != candidate_set:
        raise Rejected("unsupported linkage candidate identity")
    if not is_schema_version_one(manifest.get("schema_version")):
        raise Rejected("unsupported versioned linkage candidate schema")
    if manifest.get("candidate_kind") != "accepted_linkage":
        raise Rejected("versioned linkage candidate is not an accepted linkage manifest")
    source_pin = manifest.get("source_candidate")
    if not isinstance(source_pin, dict):
        raise Rejected("versioned linkage candidate lacks its source candidate pin")
    if (source_pin.get("path") != source_manifest_repo_path(candidate_set)
            or source_pin.get("candidate_set") != candidate_set
            or source_pin.get("sha256") != source_manifest_sha256):
        raise Rejected("versioned linkage source candidate pin differs")
    produced_source = producer_receipt.get("source_candidate")
    if not isinstance(produced_source, dict) or produced_source != source_pin:
        raise Rejected("producer receipt does not bind the reviewed source candidate")
    accepted = manifest.get("darwin")
    if not isinstance(accepted, dict):
        raise Rejected("versioned linkage candidate lacks Darwin output pins")
    recipe_hash = accepted.get("library_recipe_sha256")
    if not _is_sha256(recipe_hash):
        raise Rejected("versioned linkage candidate lacks its exact library recipe pin")
    library_recipe = producer_receipt.get("library_recipe")
    if not isinstance(library_recipe, dict):
        raise Rejected("producer receipt lacks its full library recipe")
    computed_recipe_hash = canonical_digest(library_recipe)
    if (producer_receipt.get("library_recipe_sha256") != computed_recipe_hash
            or recipe_hash != computed_recipe_hash):
        raise Rejected("producer library recipe differs from reviewed recipe pin")
    validate_source_manifest(candidate_set, source_manifest)
    receipt_headers = producer_receipt.get("headers_sha256")
    source_headers = source_manifest.get("post_patch_headers_sha256")
    if (not isinstance(receipt_headers, dict) or receipt_headers != source_headers
            or library_recipe.get("headers_sha256") != receipt_headers):
        raise Rejected("producer receipt header inventory differs from source candidate pins")
    if producer_sha256 is not None and accepted.get("producer_sha256") != producer_sha256:
        raise Rejected("producer receipt differs from reviewed receipt pin")
    if target is not None:
        expected_library = accepted.get("libraries", {}).get(target)
        if not isinstance(expected_library, str) or library_sha256 != expected_library:
            raise Rejected("library digest differs from reviewed candidate")


def source_manifest_file_sha256(candidate_set):
    path = source_manifest_path(candidate_set)
    return digest(read_bounded(path, 1024 * 1024))


def _is_sha256(value):
    return (isinstance(value, str) and len(value) == 64
            and all(character in "0123456789abcdef" for character in value))


def _is_canonical_header_path(value):
    if not isinstance(value, str):
        return False
    path = PurePosixPath(value)
    return (value == path.as_posix() and value.startswith("include/")
            and not path.is_absolute() and ".." not in path.parts
            and "." not in path.parts and path.suffix == ".h")


def is_schema_version_one(value):
    """Reject booleans, which compare equal to integer 1 in Python."""
    return type(value) is int and value == 1


def canonical_digest(value):
    """Hash a canonical JSON value using the producer's receipt encoding."""
    encoded = (json.dumps(value, sort_keys=True, separators=(",", ":"),
                          ensure_ascii=False) + "\n").encode()
    return digest(encoded)
