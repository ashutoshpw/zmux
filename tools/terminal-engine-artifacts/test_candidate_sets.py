"""Selection and integrity checks for versioned Ghostty artifact candidates."""

import copy
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from candidate_sets import (HISTORICAL, VERSIONED_7501, LINKAGE_MANIFESTS,
                            METADATA_FIXTURE_ENTRYPOINT, METADATA_FIXTURE_INTEGRATED,
                            METADATA_FIXTURE_OWNER,
                            SOURCE_MANIFESTS, canonical_digest,
                            load_source_manifest,
                            source_manifest_repo_path,
                            validate_source_candidate_files,
                            validate_source_manifest,
                            validate_versioned_linkage)
from verify import HERE, Rejected, digest, document

BUILD_SPEC = importlib.util.spec_from_file_location(
    "terminal_engine_build", HERE / "build-darwin.py")
build_darwin = importlib.util.module_from_spec(BUILD_SPEC)
BUILD_SPEC.loader.exec_module(build_darwin)


class CandidateSetChecks(unittest.TestCase):
    def source_manifest(self, root):
        patches = [
            "tools/terminal-feasibility/resource.patch",
            "tools/terminal-feasibility/metadata.patch",
            "tools/terminal-feasibility/program-status-v0.4.patch",
        ]
        inputs = {}
        for index, name in enumerate(patches):
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            data = f"patch-{index}".encode()
            path.write_bytes(data)
            inputs[name] = digest(data)
        fixture_patch = "tools/terminal-feasibility/program-status-v0.4-owner-fixture.patch"
        fixture_patch_data = b"versioned metadata owner fixture patch"
        fixture_patch_path = root / fixture_patch
        fixture_patch_path.parent.mkdir(parents=True, exist_ok=True)
        fixture_patch_path.write_bytes(fixture_patch_data)
        inputs[fixture_patch] = digest(fixture_patch_data)

        fixture_source_data = {
            METADATA_FIXTURE_ENTRYPOINT: b"metadata entrypoint",
            METADATA_FIXTURE_INTEGRATED: b"integrated fixture source",
            METADATA_FIXTURE_OWNER: b"legacy owner source",
        }
        baseline = {
            "schema_version": 1,
            "repository_inputs_sha256": {
                name: digest(data) for name, data in fixture_source_data.items()
            },
        }
        baseline_bytes = (json.dumps(baseline, indent=2) + "\n").encode()
        for name, data in fixture_source_data.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        for name, data in (
            ("tools/terminal-engine-artifacts/candidate.json", baseline_bytes),
            ("tools/terminal-engine-artifacts/upstream-inputs.json", b"upstream"),
            ("tools/terminal-engine-artifacts/darwin-recipe.json", b"recipe"),
        ):
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        return {
            "schema_version": 1,
            "candidate_kind": "source",
            "candidate_set": VERSIONED_7501,
            "base_candidate_sha256": digest(baseline_bytes),
            "upstream_inputs_sha256": digest(b"upstream"),
            "recipe_sha256": digest(b"recipe"),
            "source_inputs_sha256": inputs,
            "patches": [{"path": name, "sha256": inputs[name]} for name in patches],
            "fixture_patches": [{
                "path": fixture_patch,
                "sha256": inputs[fixture_patch],
                "target": METADATA_FIXTURE_OWNER,
                "base_sha256": baseline["repository_inputs_sha256"][METADATA_FIXTURE_OWNER],
            }],
            "post_patch_headers_sha256": {
                "include/ghostty/vt/zmux_metadata.h": digest(b"header"),
            },
        }

    def test_candidate_set_defaults_and_known_paths_are_explicit(self):
        self.assertEqual(SOURCE_MANIFESTS[HISTORICAL], HERE / "candidate.json")
        self.assertEqual(LINKAGE_MANIFESTS[HISTORICAL], HERE / "linkage-candidates.json")
        self.assertEqual(
            SOURCE_MANIFESTS[VERSIONED_7501],
            HERE / "candidate-sets" / VERSIONED_7501 / "source.json",
        )
        self.assertEqual(
            LINKAGE_MANIFESTS[VERSIONED_7501],
            HERE / "candidate-sets" / VERSIONED_7501 / "linkage.json",
        )
        self.assertEqual(
            source_manifest_repo_path(VERSIONED_7501),
            "tools/terminal-engine-artifacts/candidate-sets/ghostty-7501-v1/source.json",
        )

    def test_unknown_candidate_set_is_rejected_without_path_fallback(self):
        from candidate_sets import source_manifest_path

        with self.assertRaisesRegex(Rejected, "unsupported source candidate set"):
            source_manifest_path("../../candidate.json")

    def test_versioned_producer_refuses_any_retained_slice_reuse(self):
        build_darwin.ensure_reuse_policy(HISTORICAL, Path("old-output"))
        with self.assertRaisesRegex(Rejected, "without reuse"):
            build_darwin.ensure_reuse_policy(VERSIONED_7501, Path("old-output"))

    def test_source_manifest_pins_base_recipe_patches_and_post_patch_headers(self):
        with tempfile.TemporaryDirectory(prefix="zmux-candidate-source-") as temporary:
            root = Path(temporary)
            manifest = self.source_manifest(root)
            validate_source_manifest(VERSIONED_7501, manifest)
            validate_source_candidate_files(VERSIONED_7501, manifest, root)

            changed = root / "tools/terminal-feasibility/metadata.patch"
            original_patch = changed.read_bytes()
            changed.write_bytes(b"different patch")
            with self.assertRaisesRegex(Rejected, "sha256 mismatch"):
                validate_source_candidate_files(VERSIONED_7501, manifest, root)
            changed.write_bytes(original_patch)

            integrated = root / METADATA_FIXTURE_INTEGRATED
            original_integrated = integrated.read_bytes()
            integrated.write_bytes(original_integrated + b"drift")
            with self.assertRaisesRegex(Rejected, "sha256 mismatch"):
                validate_source_candidate_files(VERSIONED_7501, manifest, root)

    def test_fixture_patch_order_base_and_source_digest_are_pinned(self):
        with tempfile.TemporaryDirectory(prefix="zmux-candidate-fixture-") as temporary:
            root = Path(temporary)
            manifest = self.source_manifest(root)
            validate_source_candidate_files(VERSIONED_7501, manifest, root)

            extra = copy.deepcopy(manifest)
            extra_path = "tools/terminal-feasibility/extra-owner-fixture.patch"
            extra_sha = digest(b"unexpected fixture patch")
            extra["source_inputs_sha256"][extra_path] = extra_sha
            extra["fixture_patches"].append({
                "path": extra_path,
                "sha256": extra_sha,
                "target": METADATA_FIXTURE_OWNER,
                "base_sha256": extra["fixture_patches"][0]["base_sha256"],
            })
            with self.assertRaisesRegex(Rejected, "fixture patch order"):
                validate_source_manifest(VERSIONED_7501, extra)

            wrong_base = copy.deepcopy(manifest)
            wrong_base["fixture_patches"][0]["base_sha256"] = "f" * 64
            with self.assertRaisesRegex(Rejected, "historical owner pin"):
                validate_source_candidate_files(VERSIONED_7501, wrong_base, root)

            patch_path = root / manifest["fixture_patches"][0]["path"]
            patch_path.write_bytes(b"modified fixture patch")
            with self.assertRaisesRegex(Rejected, "sha256 mismatch"):
                validate_source_candidate_files(VERSIONED_7501, manifest, root)

    def test_versioned_fixture_staging_applies_pinned_patch_and_records_exact_output(self):
        _, manifest = load_source_manifest(VERSIONED_7501)
        candidate = document(HERE / "candidate.json")

        def run(command, _label, timeout=60, cwd=None):
            return subprocess.run(command, cwd=cwd, check=True, text=True,
                                  capture_output=True, timeout=timeout).stdout

        with tempfile.TemporaryDirectory(prefix="zmux-candidate-fixture-stage-") as temporary:
            entrypoint, receipt = build_darwin.stage_metadata_fixture(
                Path(temporary), candidate, manifest, run)
            staged = entrypoint.parent
            owner_path = staged / Path(METADATA_FIXTURE_OWNER).name
            self.assertEqual(entrypoint.name, Path(METADATA_FIXTURE_ENTRYPOINT).name)
            self.assertTrue((staged / Path(METADATA_FIXTURE_INTEGRATED).name).is_file())
            self.assertTrue(owner_path.is_file())
            self.assertEqual(receipt["base_owner_sha256"],
                             manifest["fixture_patches"][0]["base_sha256"])
            self.assertEqual(receipt["patches"], [
                {"path": patch["path"], "sha256": patch["sha256"]}
                for patch in manifest["fixture_patches"]
            ])
            self.assertEqual(receipt["entrypoint_sha256"],
                             candidate["repository_inputs_sha256"][METADATA_FIXTURE_ENTRYPOINT])
            self.assertEqual(receipt["integrated_sha256"],
                             candidate["repository_inputs_sha256"][METADATA_FIXTURE_INTEGRATED])
            self.assertEqual(receipt["patched_owner_sha256"], digest(owner_path.read_bytes()))
            self.assertNotEqual(receipt["patched_owner_sha256"], receipt["base_owner_sha256"])

    def test_versioned_source_schema_one_is_exact_and_headers_are_canonical_h_paths(self):
        with tempfile.TemporaryDirectory(prefix="zmux-candidate-source-") as temporary:
            root = Path(temporary)
            manifest = self.source_manifest(root)
            validate_source_manifest(VERSIONED_7501, manifest)
            for version in (True, 2, 3):
                with self.subTest(version=version):
                    invalid_version = copy.deepcopy(manifest)
                    invalid_version["schema_version"] = version
                    with self.assertRaisesRegex(Rejected, "schema"):
                        validate_source_manifest(VERSIONED_7501, invalid_version)

            for path in (
                "include/../outside.h",
                "include/ghostty/program_status.txt",
                "include//ghostty/program_status.h",
                "include/./ghostty/program_status.h",
                "include\\ghostty\\program_status.h",
            ):
                with self.subTest(path=path):
                    invalid = copy.deepcopy(self.source_manifest(root))
                    invalid["post_patch_headers_sha256"] = {path: digest(b"header")}
                    with self.assertRaisesRegex(Rejected, "header pin"):
                        validate_source_manifest(VERSIONED_7501, invalid)

    def test_wrong_source_identity_and_patch_order_are_rejected(self):
        with tempfile.TemporaryDirectory(prefix="zmux-candidate-source-") as temporary:
            manifest = self.source_manifest(Path(temporary))
            with self.assertRaisesRegex(Rejected, "identity"):
                validate_source_manifest("wrong-set", manifest)
            wrong_order = copy.deepcopy(manifest)
            wrong_order["patches"].reverse()
            with self.assertRaisesRegex(Rejected, "patch order"):
                validate_source_manifest(VERSIONED_7501, wrong_order)

    def linkage_fixture(self):
        with tempfile.TemporaryDirectory(prefix="zmux-candidate-source-") as temporary:
            source_manifest = CandidateSetChecks().source_manifest(Path(temporary))
        headers = source_manifest["post_patch_headers_sha256"]
        library_recipe = {
            "recipe": {"targets": ["arm64", "x86_64"]},
            "ghostty": {"commit": "1" * 40, "sha256": digest(b"ghostty source")},
            "zig": {"version": "0.16.0", "sha256": digest(b"zig toolchain")},
            "patches_sha256": {
                patch["path"].rsplit("/", 1)[-1]: patch["sha256"]
                for patch in source_manifest["patches"]
            },
            "developer": {
                "xcode": "Xcode 16.4",
                "sdk_path": "/fixture/MacOSX15.5.sdk",
                "sdk_version": "15.5",
                "sdk_settings_sha256": digest(b"SDK settings"),
                "clang": "fixture Apple clang",
                "rust": "rustc 1.95.0",
                "host_macos": "15.5",
            },
            "headers_sha256": headers,
        }
        recipe_hash = canonical_digest(library_recipe)
        fixture_source_sha256 = {
            METADATA_FIXTURE_ENTRYPOINT: digest(b"metadata entrypoint"),
            METADATA_FIXTURE_INTEGRATED: digest(b"integrated fixture source"),
            METADATA_FIXTURE_OWNER: source_manifest["fixture_patches"][0]["base_sha256"],
        }
        source_pin = {
            "candidate_set": VERSIONED_7501,
            "path": source_manifest_repo_path(VERSIONED_7501),
            "sha256": "a" * 64,
        }
        receipt_hash = "c" * 64
        library_hash = "d" * 64
        manifest = {
            "schema_version": 1,
            "candidate_kind": "accepted_linkage",
            "candidate_set": VERSIONED_7501,
            "source_candidate": source_pin,
            "darwin": {
                "producer_sha256": receipt_hash,
                "library_recipe_sha256": recipe_hash,
                "libraries": {"aarch64-apple-darwin": library_hash},
            },
        }
        receipt = {
            "source_candidate": copy.deepcopy(source_pin),
            "library_recipe": library_recipe,
            "library_recipe_sha256": recipe_hash,
            "headers_sha256": headers,
            "source_sha256": copy.deepcopy(fixture_source_sha256),
            "metadata_fixture": {
                "entrypoint_path": METADATA_FIXTURE_ENTRYPOINT,
                "entrypoint_sha256": fixture_source_sha256[METADATA_FIXTURE_ENTRYPOINT],
                "integrated_sha256": fixture_source_sha256[METADATA_FIXTURE_INTEGRATED],
                "owner_path": METADATA_FIXTURE_OWNER,
                "base_owner_sha256": source_manifest["fixture_patches"][0]["base_sha256"],
                "patches": [{
                    "path": patch["path"],
                    "sha256": patch["sha256"],
                } for patch in source_manifest["fixture_patches"]],
                "patched_owner_sha256": digest(b"patched owner fixture"),
            },
        }
        return (manifest, receipt, source_manifest, source_pin["sha256"], receipt_hash,
                library_hash, fixture_source_sha256)

    def test_canonical_recipe_digest_matches_producer_json_encoding(self):
        self.assertEqual(
            canonical_digest({"z": 2, "a": 1}),
            digest(b'{"a":1,"z":2}\n'),
        )

    def test_linkage_candidate_rejects_wrong_source_recipe_receipt_and_library(self):
        target = "aarch64-apple-darwin"
        manifest, receipt, source_manifest, source_sha, receipt_sha, library_sha, _ = self.linkage_fixture()
        validate_versioned_linkage(
            VERSIONED_7501, manifest, receipt,
            source_manifest=source_manifest,
            source_manifest_sha256=source_sha,
            target=target, library_sha256=library_sha,
            producer_sha256=receipt_sha,
        )

        cases = [
            ("wrong candidate", dict(candidate_set="wrong-set")),
            ("wrong source", dict(source_manifest_sha256="e" * 64)),
            ("wrong recipe", dict(receipt= dict(receipt, library_recipe_sha256="f" * 64))),
            ("wrong receipt", dict(producer_sha256="0" * 64)),
            ("wrong library", dict(library_sha256="1" * 64)),
            ("wrong target", dict(target="x86_64-apple-darwin")),
        ]
        for label, changes in cases:
            with self.subTest(label=label), self.assertRaises(Rejected):
                candidate_set = changes.get("candidate_set", VERSIONED_7501)
                changed_manifest = copy.deepcopy(manifest)
                changed_receipt = changes.get("receipt", receipt)
                validate_versioned_linkage(
                    candidate_set, changed_manifest, changed_receipt,
                    source_manifest=source_manifest,
                    source_manifest_sha256=changes.get("source_manifest_sha256", source_sha),
                    target=changes.get("target", target),
                    library_sha256=changes.get("library_sha256", library_sha),
                    producer_sha256=changes.get("producer_sha256", receipt_sha),
                )

    def test_linkage_recomputes_full_recipe_digest_and_binds_header_inventory(self):
        manifest, receipt, source_manifest, source_sha, receipt_sha, _, _ = self.linkage_fixture()
        # Every producer recipe component is covered, including siblings of
        # `recipe`; hashing only its targets must not admit changed inputs.
        for component, field, changed_value in (
            ("recipe", "targets", ["x86_64", "arm64"]),
            ("ghostty", "sha256", "2" * 64),
            ("zig", "sha256", "3" * 64),
            ("patches_sha256", "program-status-v0.4.patch", "4" * 64),
            ("developer", "sdk_settings_sha256", "5" * 64),
        ):
            with self.subTest(component=component):
                changed_recipe = copy.deepcopy(receipt)
                changed_recipe["library_recipe"][component][field] = changed_value
                with self.assertRaisesRegex(Rejected, "library recipe"):
                    validate_versioned_linkage(
                        VERSIONED_7501, manifest, changed_recipe,
                        source_manifest=source_manifest,
                        source_manifest_sha256=source_sha,
                        producer_sha256=receipt_sha,
                    )

        changed_headers = copy.deepcopy(receipt)
        changed_headers["headers_sha256"] = dict(changed_headers["headers_sha256"])
        changed_headers["headers_sha256"]["include/ghostty/vt/zmux_metadata.h"] = "0" * 64
        with self.assertRaisesRegex(Rejected, "header inventory"):
            validate_versioned_linkage(
                VERSIONED_7501, manifest, changed_headers,
                source_manifest=source_manifest,
                source_manifest_sha256=source_sha,
                producer_sha256=receipt_sha,
            )

    def test_linkage_binds_versioned_fixture_patch_to_captured_owner_and_receipt(self):
        manifest, receipt, source_manifest, source_sha, receipt_sha, library_sha, fixture_source_sha256 = self.linkage_fixture()
        validate_versioned_linkage(
            VERSIONED_7501, manifest, receipt,
            source_manifest=source_manifest,
            source_manifest_sha256=source_sha,
            target="aarch64-apple-darwin", library_sha256=library_sha,
            producer_sha256=receipt_sha, fixture_source_sha256=fixture_source_sha256,
        )

        corruptions = (
            ("missing fixture record", lambda changed: changed.pop("metadata_fixture")),
            ("wrong owner base", lambda changed: changed["metadata_fixture"].update(
                base_owner_sha256="0" * 64)),
            ("wrong fixture patch", lambda changed: changed["metadata_fixture"]["patches"][0].update(
                sha256="1" * 64)),
            ("missing staged digest", lambda changed: changed["metadata_fixture"].update(
                patched_owner_sha256="invalid")),
            ("uncaptured entrypoint", lambda changed: changed["source_sha256"].update(
                {METADATA_FIXTURE_ENTRYPOINT: "2" * 64})),
            ("fixture source outside historical pin", lambda changed: (
                changed["metadata_fixture"].update(entrypoint_sha256="3" * 64),
                changed["source_sha256"].update({METADATA_FIXTURE_ENTRYPOINT: "3" * 64}))),
        )
        for label, corrupt in corruptions:
            with self.subTest(contract=label):
                changed = copy.deepcopy(receipt)
                corrupt(changed)
                with self.assertRaises(Rejected):
                    validate_versioned_linkage(
                        VERSIONED_7501, manifest, changed,
                        source_manifest=source_manifest,
                        source_manifest_sha256=source_sha,
                        producer_sha256=receipt_sha,
                        fixture_source_sha256=fixture_source_sha256,
                    )


if __name__ == "__main__":
    unittest.main()
