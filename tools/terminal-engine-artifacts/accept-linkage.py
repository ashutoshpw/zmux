#!/usr/bin/env python3
"""Verify exact retained linkage bytes; never download, rebuild, or run an engine."""
import argparse
import json
from pathlib import Path
import sys

from verify import (HERE, REPO, Rejected, checked_file, digest, document, read_bounded,
                    verify_candidate)
from darwin_platform import inspect_darwin_archive
from candidate_sets import (HISTORICAL, VERSIONED_7501, load_linkage_manifest,
                            load_source_manifest, source_manifest_repo_path,
                            validate_source_candidate_files,
                            validate_versioned_linkage)


def accept(root, target, candidate_set=HISTORICAL):
    root = root.resolve(strict=True)
    linkage_manifest, manifest = load_linkage_manifest(candidate_set)
    inputs = [str(linkage_manifest.resolve())]

    def check(base, name, expected, limit=1024 * 1024):
        data = checked_file(base, name, expected, limit)
        inputs.append(str((base / name).resolve()))
        return data

    for name, expected in manifest["shim_inputs_sha256"].items():
        check(REPO, name, expected)
    if target == "x86_64-unknown-linux-gnu":
        if candidate_set != HISTORICAL:
            raise Rejected("this candidate set has no accepted Linux library")
        library = root / "install/lib/libghostty-vt.a"
        producer = root / "receipt.json"
        verify_candidate(library, producer, "linux-x86_64")
        upstream = document(HERE / "upstream-inputs.json")
        include_root = root / "inputs/ghostty-22fa134a07f5749edd9e92c6bc17d8aaf61843be"
        for name, expected in upstream["patched_header_sha256"].items():
            check(include_root, name, expected)
        # verify_candidate checks all source, receipt, notice and witness inputs.
        inputs.extend(str((REPO / name).resolve()) for name in document(HERE / "candidate.json")["repository_inputs_sha256"])
        inputs.extend(str(path.resolve()) for path in HERE.rglob("*") if path.is_file() and path.suffix == ".json")
        inputs.extend(str((HERE / notice["path"]).resolve()) for notice in document(HERE / "notices.json")["notices"])
        inputs.extend([str(library), str(producer)])
    elif target in manifest["darwin"]["libraries"]:
        accepted = manifest["darwin"]
        original = root / "original-upload/terminal-engine-artifacts"
        bundle = original / "bundle"
        receipt_bytes = check(original, "receipt.json", accepted["producer_sha256"])
        receipt = json.loads(receipt_bytes)
        check(root, "independent-review.json", accepted["independent_review_sha256"])
        if receipt["state"] != "complete" or receipt["source_sha"] != accepted["source_sha"]:
            raise Rejected("Darwin producer identity/state differs")
        source = json.loads(read_bounded(bundle / "sources/manifest.json", 1024 * 1024))
        inputs.append(str(bundle / "sources/manifest.json"))
        if source["source_sha"] != accepted["source_sha"] or source["repository_inputs_sha256"] != receipt["source_sha256"]:
            raise Rejected("retained source manifest differs from pinned producer")
        for name, expected in receipt["source_sha256"].items():
            if name in accepted["supplement"]:
                if expected != accepted["supplement"][name]:
                    raise Rejected("supplement hash differs")
                check(root / "supplemental-source", name, expected)
            else:
                check(bundle / "sources/repository", name, expected)
        source_manifest_sha256 = None
        if candidate_set != HISTORICAL:
            source_pin = manifest.get("source_candidate")
            source_name = source_manifest_repo_path(candidate_set)
            if not isinstance(source_pin, dict) or source_pin.get("path") != source_name:
                raise Rejected("versioned linkage candidate source path differs")
            source_manifest, source_document = load_source_manifest(candidate_set)
            fixture_source_sha256 = validate_source_candidate_files(
                candidate_set, source_document)
            inputs.extend(str(path.resolve()) for path in (
                source_manifest,
                REPO / "tools/terminal-engine-artifacts/candidate.json",
                REPO / "tools/terminal-engine-artifacts/upstream-inputs.json",
                REPO / "tools/terminal-engine-artifacts/darwin-recipe.json",
                *(REPO / name for name in source_document["source_inputs_sha256"]),
            ))
            source_bytes = checked_file(bundle / "sources/repository", source_name,
                                        source_pin["sha256"])
            validate_versioned_linkage(
                candidate_set, manifest, receipt,
                source_manifest=source_document,
                source_manifest_sha256=digest(source_bytes),
                producer_sha256=digest(receipt_bytes),
                fixture_source_sha256=fixture_source_sha256)
            source_manifest_sha256 = digest(source_bytes)
        for name, expected in receipt["headers_sha256"].items():
            check(bundle, name, expected)
        architecture = "arm64" if target.startswith("aarch64") else "x86_64"
        library = bundle / architecture / "libghostty-vt.a"
        library_bytes = check(bundle, f"{architecture}/libghostty-vt.a",
                              accepted["libraries"][target], 64 * 1024 * 1024)
        if candidate_set != HISTORICAL:
            validate_versioned_linkage(
                candidate_set, manifest, receipt,
                source_manifest=source_document,
                source_manifest_sha256=source_manifest_sha256,
                target=target, library_sha256=digest(library_bytes),
                producer_sha256=digest(receipt_bytes),
                fixture_source_sha256=fixture_source_sha256)
        inspect_darwin_archive(library, architecture)
        include_root = bundle
    else:
        raise Rejected("no accepted linkage candidate for target")
    return {"target": target, "include": str(include_root / "include"),
            "library_directory": str(library.parent), "inputs": sorted(set(inputs)),
            "scope": manifest["scope"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--candidate-set", choices=(HISTORICAL, VERSIONED_7501),
                        default=HISTORICAL)
    args = parser.parse_args()
    try:
        print(json.dumps(accept(args.root, args.target, args.candidate_set)))
    except (Rejected, OSError, ValueError, KeyError, TypeError) as error:
        print(f"linkage artifact rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
