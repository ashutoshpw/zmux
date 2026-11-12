"""Check retained download/extraction and exact Cargo-output admission without builds."""
import base64
import copy
import importlib.util
import json
from pathlib import Path
import stat
import struct
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile

import candidate_sets
from candidate_sets import HISTORICAL, VERSIONED_7501
from verify import Rejected, digest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("darwin_linkage", HERE / "check-darwin-linkage.py")
linkage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(linkage)


class RetainedLinkageChecks(unittest.TestCase):
    @staticmethod
    def write_json(path, document):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(document, indent=2) + "\n")

    @staticmethod
    def read_json(path):
        return json.loads(path.read_text())

    def make_versioned_fixture(self, root):
        repository = root / "repository"
        inventory = repository / "tools/terminal-engine-artifacts"
        source_path = inventory / "candidate-sets/ghostty-7501-v1/source.json"
        linkage_path = inventory / "candidate-sets/ghostty-7501-v1/linkage.json"

        baseline_path = inventory / "candidate.json"
        upstream_path = inventory / "upstream-inputs.json"
        recipe_path = inventory / "darwin-recipe.json"
        baseline_path.parent.mkdir(parents=True, exist_ok=True)
        fixture_source_data = {
            candidate_sets.METADATA_FIXTURE_ENTRYPOINT: b"metadata entrypoint fixture\n",
            candidate_sets.METADATA_FIXTURE_INTEGRATED: b"integrated fixture source\n",
            candidate_sets.METADATA_FIXTURE_OWNER: b"legacy metadata owner\n",
        }
        for name, data in fixture_source_data.items():
            path = repository / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.write_json(baseline_path, {
            "schema_version": 1,
            "repository_inputs_sha256": {
                name: digest(data) for name, data in fixture_source_data.items()
            },
        })
        upstream_path.write_bytes(b'{"schema_version":1}\n')
        recipe = {
            "schema_version": 1,
            "xcode_version": "16.4",
            "sdk_version": "15.5",
            "rust_version": "1.95.0",
            "minimum_macos": "14.0",
        }
        self.write_json(recipe_path, recipe)

        patch_paths = [
            "tools/terminal-feasibility/resource.patch",
            "tools/terminal-feasibility/metadata.patch",
            "tools/terminal-feasibility/program-status-v0.4.patch",
        ]
        fixture_patch_path = "tools/terminal-feasibility/program-status-v0.4-owner-fixture.patch"
        source_inputs = {}
        for name in patch_paths + [fixture_patch_path,
                                   "tools/terminal-feasibility/program-status-consumer.h"]:
            data = ("fixture input: " + name + "\n").encode()
            path = repository / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            source_inputs[name] = digest(data)
        source_manifest = {
            "schema_version": 1,
            "candidate_kind": "source",
            "candidate_set": VERSIONED_7501,
            "base_candidate_sha256": digest(baseline_path.read_bytes()),
            "upstream_inputs_sha256": digest(upstream_path.read_bytes()),
            "recipe_sha256": digest(recipe_path.read_bytes()),
            "source_inputs_sha256": source_inputs,
            "patches": [{"path": name, "sha256": source_inputs[name]} for name in patch_paths],
            "fixture_patches": [{
                "path": fixture_patch_path,
                "sha256": source_inputs[fixture_patch_path],
                "target": candidate_sets.METADATA_FIXTURE_OWNER,
                "base_sha256": digest(fixture_source_data[candidate_sets.METADATA_FIXTURE_OWNER]),
            }],
            "post_patch_headers_sha256": {
                "include/ghostty/vt/zmux_metadata.h": digest(b"patched header fixture\n"),
            },
        }
        self.write_json(source_path, source_manifest)
        source_pin = {
            "candidate_set": VERSIONED_7501,
            "path": "tools/terminal-engine-artifacts/candidate-sets/ghostty-7501-v1/source.json",
            "sha256": digest(source_path.read_bytes()),
        }

        review_path = inventory / "evidence/versioned-independent-review.json"
        review_path.parent.mkdir(parents=True, exist_ok=True)
        review_bytes = b'{"review_fixture":"temporary test input"}\n'
        review_path.write_bytes(review_bytes)
        source_sha = digest(b"source revision fixture")[:40]
        linkage_manifest = {
            "schema_version": 1,
            "candidate_kind": "accepted_linkage",
            "candidate_set": VERSIONED_7501,
            "scope": "temporary production-validator fixture",
            "source_candidate": source_pin,
            "darwin": {
                "producer_sha256": digest(b"producer receipt fixture"),
                "library_recipe_sha256": digest(b"library recipe fixture"),
                "supplement": {},
                "independent_review_path": "evidence/versioned-independent-review.json",
                "independent_review_sha256": digest(review_bytes),
                "source_sha": source_sha,
                "libraries": {
                    "aarch64-apple-darwin": digest(b"arm64 library fixture"),
                    "x86_64-apple-darwin": digest(b"x86_64 library fixture"),
                },
                "download": {
                    "repository": "ashutoshpw/zmux",
                    "run": 92000123456,
                    "artifact": 92000654321,
                    "name": "terminal-engine-darwin-thin-" + source_sha,
                    "archive_sha256": digest(b"download archive fixture"),
                    "archive_bytes": len(b"download archive fixture"),
                },
            },
            "shim_inputs_sha256": {},
        }
        self.write_json(linkage_path, linkage_manifest)
        return {
            "repository": repository,
            "inventory": inventory,
            "source_path": source_path,
            "linkage_path": linkage_path,
        }

    def check_versioned_fixture(self, fixture):
        with (patch.object(linkage, "HERE", fixture["inventory"]),
              patch.object(linkage, "REPO", fixture["repository"]),
              patch.object(candidate_sets, "REPO", fixture["repository"]),
              patch.dict(candidate_sets.SOURCE_MANIFESTS, {VERSIONED_7501: fixture["source_path"]}),
              patch.dict(candidate_sets.LINKAGE_MANIFESTS, {VERSIONED_7501: fixture["linkage_path"]})):
            return linkage.check_inputs(VERSIONED_7501)

    def test_check_inputs_accepts_well_formed_versioned_manifest_with_fresh_ids(self):
        with tempfile.TemporaryDirectory(prefix="zmux-versioned-linkage-") as temporary:
            fixture = self.make_versioned_fixture(Path(temporary))
            accepted, _ = self.check_versioned_fixture(fixture)
            self.assertEqual(accepted["download"]["run"], 92000123456)
            self.assertEqual(accepted["download"]["artifact"], 92000654321)
            self.assertEqual(set(accepted["libraries"]), {
                "aarch64-apple-darwin", "x86_64-apple-darwin",
            })

    def test_check_inputs_rejects_invalid_versioned_manifest_contracts(self):
        def rewrite_linkage(fixture, edit):
            manifest = self.read_json(fixture["linkage_path"])
            edit(manifest)
            self.write_json(fixture["linkage_path"], manifest)

        def bad_linkage_schema(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest.update(schema_version=2))

        def bad_source_schema(fixture):
            source = self.read_json(fixture["source_path"])
            source["schema_version"] = 2
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def bad_source_boolean_schema(fixture):
            source = self.read_json(fixture["source_path"])
            source["schema_version"] = True
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def bad_linkage_boolean_schema(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest.update(schema_version=True))

        def wrong_source_pin(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256="0" * 64))

        def wrong_recipe_pin(fixture):
            source = self.read_json(fixture["source_path"])
            source["recipe_sha256"] = "0" * 64
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def missing_fixture_patch_chain(fixture):
            source = self.read_json(fixture["source_path"])
            source.pop("fixture_patches")
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def fixture_patch_digest_mismatch(fixture):
            source = self.read_json(fixture["source_path"])
            source["fixture_patches"][0]["sha256"] = "0" * 64
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def fixture_base_drift(fixture):
            source = self.read_json(fixture["source_path"])
            source["fixture_patches"][0]["base_sha256"] = "0" * 64
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def extra_fixture_patch(fixture):
            source = self.read_json(fixture["source_path"])
            name = "tools/terminal-feasibility/extra-owner-fixture.patch"
            sha256 = digest(b"unexpected fixture patch\n")
            source["source_inputs_sha256"][name] = sha256
            source["fixture_patches"].append({
                "path": name,
                "sha256": sha256,
                "target": candidate_sets.METADATA_FIXTURE_OWNER,
                "base_sha256": source["fixture_patches"][0]["base_sha256"],
            })
            self.write_json(fixture["source_path"], source)
            pin = digest(fixture["source_path"].read_bytes())
            rewrite_linkage(fixture, lambda manifest: manifest["source_candidate"].update(sha256=pin))

        def missing_target(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["darwin"]["libraries"].pop("x86_64-apple-darwin"))

        def latest_run(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["darwin"]["download"].update(run="latest"))

        def missing_artifact_id(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["darwin"]["download"].update(artifact=0))

        def missing_supplement_inventory(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["darwin"].pop("supplement"))

        def invalid_supplement_inventory(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["darwin"].update(supplement=None))

        def unsafe_supplement_path(fixture):
            rewrite_linkage(fixture, lambda manifest: manifest["darwin"].update(supplement={"../escape": "a" * 64}))

        cases = (
            ("linkage schema", bad_linkage_schema),
            ("boolean linkage schema", bad_linkage_boolean_schema),
            ("source schema", bad_source_schema),
            ("boolean source schema", bad_source_boolean_schema),
            ("source manifest pin", wrong_source_pin),
            ("source recipe pin", wrong_recipe_pin),
            ("missing versioned fixture patch chain", missing_fixture_patch_chain),
            ("fixture patch source digest", fixture_patch_digest_mismatch),
            ("fixture base owner pin", fixture_base_drift),
            ("unexpected fixture patch order", extra_fixture_patch),
            ("both Apple targets", missing_target),
            ("run locator", latest_run),
            ("artifact locator", missing_artifact_id),
            ("missing supplement inventory", missing_supplement_inventory),
            ("invalid supplement inventory", invalid_supplement_inventory),
            ("unsafe supplement path", unsafe_supplement_path),
        )
        for label, mutate in cases:
            with self.subTest(contract=label), tempfile.TemporaryDirectory(prefix="zmux-invalid-linkage-") as temporary:
                fixture = self.make_versioned_fixture(Path(temporary))
                mutate(fixture)
                with self.assertRaises(Rejected):
                    self.check_versioned_fixture(fixture)

    def test_versioned_download_identity_keeps_exact_observed_api_pins(self):
        with tempfile.TemporaryDirectory(prefix="zmux-versioned-linkage-") as temporary:
            fixture = self.make_versioned_fixture(Path(temporary))
            accepted, _ = self.check_versioned_fixture(fixture)
            pin = accepted["download"]
            valid = {
                "id": pin["artifact"],
                "name": pin["name"],
                "expired": False,
                "size_in_bytes": pin["archive_bytes"],
                "digest": "sha256:" + pin["archive_sha256"],
                "workflow_run": {"id": pin["run"], "head_sha": accepted["source_sha"]},
            }
            linkage.verify_download_identity(valid, accepted)
            changed = (
                ("artifact id", "id", 1),
                ("artifact name", "name", "latest"),
                ("expiry", "expired", True),
                ("archive size", "size_in_bytes", pin["archive_bytes"] + 1),
                ("archive digest", "digest", "sha256:" + "0" * 64),
                ("run id", "workflow_run", {"id": pin["run"] + 1, "head_sha": accepted["source_sha"]}),
                ("source SHA", "workflow_run", {"id": pin["run"], "head_sha": "0" * 40}),
            )
            for label, key, value in changed:
                with self.subTest(identity=label), self.assertRaises(Rejected):
                    metadata = copy.deepcopy(valid)
                    metadata[key] = value
                    linkage.verify_download_identity(metadata, accepted)

    def test_versioned_source_hashes_include_the_exact_linked_program_status_case(self):
        hashes = linkage.source_hashes(candidate_set=VERSIONED_7501)
        path = "crates/terminal-adapter/tests/program_status.rs"
        self.assertIn(path, hashes)
        self.assertEqual(hashes[path], digest((Path(__file__).resolve().parents[2] / path).read_bytes()))
        # The shared owner defines the semantics exercised by the linked tests;
        # its implementation and dependency contract must be retained as inputs.
        for shared in ("crates/program-status/src/lib.rs", "crates/program-status/Cargo.toml"):
            self.assertTrue(shared in hashes, f"shared protocol input missing: {shared}")
            self.assertEqual(hashes[shared], digest((Path(__file__).resolve().parents[2] / shared).read_bytes()))

        session_path = Path(__file__).resolve().parents[2] / "crates/session-runtime/tests/program_status.rs"
        session_hashes = linkage.source_hashes(
            include_session_runtime=True, candidate_set=VERSIONED_7501)
        if session_path.is_file():
            relative = "crates/session-runtime/tests/program_status.rs"
            self.assertIn(relative, session_hashes)
            self.assertEqual(session_hashes[relative], digest(session_path.read_bytes()))

    def test_program_status_lane_requires_the_exact_single_linked_test(self):
        passing = (
            "running 1 test\n"
            "test linked_engine_stages_raw_reports_query_prompt_ris_child_exit_and_close ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.02s\n"
        )
        linkage.require_program_status_test(passing)
        for output in (
            passing.replace("1 passed; 0 failed", "0 passed; 1 failed"),
            passing.replace("linked_engine_stages_raw_reports_query_prompt_ris_child_exit_and_close", "another_test"),
            passing.replace("1 passed; 0 failed", "7 passed; 0 failed"),
        ):
            with self.subTest(output=output), self.assertRaisesRegex(Rejected, "exactly once"):
                linkage.require_program_status_test(output)

        session_passing = (
            "running 1 test\n"
            "test linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.02s\n"
        )
        linkage.require_session_program_status_test(session_passing)
        with self.assertRaisesRegex(Rejected, "exactly once"):
            linkage.require_session_program_status_test(
                session_passing.replace("1 passed; 0 failed", "0 passed; 1 failed"))

    def test_check_inputs_keeps_historical_run_and_artifact_ids_pinned(self):
        for locator in ("run", "artifact"):
            with self.subTest(locator=locator), tempfile.TemporaryDirectory(prefix="zmux-historical-linkage-") as temporary:
                historical = linkage.document(HERE / "linkage-candidates.json")
                historical["darwin"]["download"][locator] += 1
                manifest_path = Path(temporary) / "linkage-candidates.json"
                self.write_json(manifest_path, historical)
                with patch.dict(candidate_sets.LINKAGE_MANIFESTS, {HISTORICAL: manifest_path}):
                    with self.assertRaisesRegex(Rejected, "unsupported retained download identity"):
                        linkage.check_inputs(HISTORICAL)

    def test_exact_download_identity_refuses_changed_or_expired_inputs(self):
        accepted, _ = linkage.check_inputs()
        pin = accepted["download"]
        valid = {"id": pin["artifact"], "name": pin["name"], "expired": False,
                 "size_in_bytes": pin["archive_bytes"], "digest": "sha256:" + pin["archive_sha256"],
                 "workflow_run": {"id": pin["run"], "head_sha": accepted["source_sha"]}}
        linkage.verify_download_identity(valid, accepted)
        for key, value in (("id", 0), ("name", "newest"), ("expired", True),
                           ("size_in_bytes", pin["archive_bytes"] + 1), ("digest", "sha256:" + "0" * 64),
                           ("workflow_run", {"id": pin["run"], "head_sha": "0" * 40})):
            with self.subTest(field=key), self.assertRaises(Rejected):
                changed = copy.deepcopy(valid)
                changed[key] = value
                linkage.verify_download_identity(changed, accepted)

    def extract(self, entries, *, wrong_digest=False):
        with tempfile.TemporaryDirectory(prefix="zmux-retained-zip-") as temporary:
            root = Path(temporary)
            archive = root / "input.zip"
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)  # Deliberately duplicate ZIP entry.
                with zipfile.ZipFile(archive, "w") as output:
                    for name, value in entries:
                        output.writestr(name, value)
            data = archive.read_bytes()
            destination = root / "output"
            destination.mkdir()
            result = linkage.extract_retained(archive, destination,
                                               "0" * 64 if wrong_digest else digest(data), len(data))
            self.assertEqual((destination / "accepted/receipt.json").read_bytes(), b"original")
            return result

    def test_retained_zip_extracts_exact_regular_bytes(self):
        result = self.extract([("accepted/receipt.json", b"original")])
        self.assertEqual(result["entries"], 1)

    def test_changed_zip_bytes_refuse_before_extraction(self):
        with self.assertRaisesRegex(Rejected, "digest/size"):
            self.extract([("accepted/receipt.json", b"original")], wrong_digest=True)

    def test_traversal_absolute_alias_and_duplicate_zip_entries_refuse(self):
        for entries in ([('../escape', b'x')], [('/escape', b'x')], [('a/./b', b'x')],
                        [('a\\b', b'x')], [('same', b'x'), ('same', b'y')]):
            with self.subTest(entries=entries), self.assertRaisesRegex(Rejected, "ZIP entry"):
                self.extract(entries)

    def test_symlink_zip_entries_refuse(self):
        link = zipfile.ZipInfo("accepted/link")
        link.create_system = 3
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        with self.assertRaisesRegex(Rejected, "ZIP entry"):
            self.extract([(link, b"../../escape")])

    def test_source_supplement_requires_full_exact_committed_bytes(self):
        data = b"original source including hidden path\n"
        response = {"type": "file", "path": ".github/workflows/engine.yml", "encoding": "base64",
                    "size": len(data), "content": base64.b64encode(data).decode()}
        self.assertEqual(linkage.supplement_bytes(response, response["path"], digest(data)), data)
        changed = dict(response, content=base64.b64encode(data + b"changed").decode())
        with self.assertRaisesRegex(Rejected, "digest/size"):
            linkage.supplement_bytes(changed, response["path"], digest(data))

    def test_cargo_admits_only_exact_package_artifacts_inside_fresh_target(self):
        with tempfile.TemporaryDirectory(prefix="zmux-cargo-output-") as temporary:
            root = Path(temporary).resolve()
            executable = root / "staged-engine"
            owner = root / "owner-out"
            messages = [{"reason": "compiler-artifact", "package_id": "adapter-id", "target": {
                         "name": "staged_engine", "kind": ["test"]}, "executable": str(executable)},
                        {"reason": "build-script-executed", "package_id": "bridge-id", "out_dir": str(owner)}]
            encode = lambda records: "\n".join(json.dumps(record) for record in records)
            self.assertEqual(linkage.cargo_outputs(encode(messages), "adapter-id", "bridge-id", root), (executable, owner))
            status_messages = copy.deepcopy(messages)
            status_messages[0]["target"]["name"] = "program_status"
            self.assertEqual(
                linkage.cargo_outputs(encode(status_messages), "adapter-id", "bridge-id", root, "program_status"),
                (executable, owner),
            )
            self.assertEqual(
                linkage.cargo_test_executable(
                    encode(status_messages[:1]), "adapter-id", root, "program_status"),
                executable,
            )
            session_messages = copy.deepcopy(messages)
            session_messages[0]["package_id"] = "session-id"
            session_messages[0]["target"]["name"] = "engine"
            self.assertEqual(linkage.cargo_outputs(encode(session_messages), "session-id", "bridge-id", root, "engine"), (executable, owner))
            with self.assertRaisesRegex(Rejected, "one exact"):
                linkage.cargo_outputs(encode(session_messages), "session-id", "bridge-id", root)

            with self.assertRaisesRegex(Rejected, "one exact"):
                linkage.cargo_outputs(encode(messages), "different-adapter", "bridge-id", root)
            outside = copy.deepcopy(messages)
            outside[0]["executable"] = str(root.parent / "stale-test")
            with self.assertRaisesRegex(Rejected, "escaped"):
                linkage.cargo_outputs(encode(outside), "adapter-id", "bridge-id", root)
            ambiguous = copy.deepcopy(messages)
            ambiguous.append(dict(messages[0], executable=str(root / "second-test")))
            with self.assertRaisesRegex(Rejected, "one exact"):
                linkage.cargo_outputs(encode(ambiguous), "adapter-id", "bridge-id", root)

    def test_session_runtime_closure_only_adds_the_documented_system_libraries(self):
        def commands(libraries, rpath=False):
            # Mach-O load command bytes from the published ABI. The production
            # driver separately validates complete executable/version geometry.
            values = []
            for library in libraries:
                name = library.encode() + b"\0"
                length = (24 + len(name) + 7) & ~7
                values.append(struct.pack("<6I", 0xC, length, 24, 0, 0, 0) + name + b"\0" * (length - 24 - len(name)))
            if rpath:
                values.append(struct.pack("<4I", 0x8000001C, 16, 12, 0))
            return struct.pack("<8I", 0xfeedfacf, 0x100000C, 0, 2, len(values), sum(map(len, values)), 0, 0) + b"".join(values)
        system = "/usr/lib/libSystem.B.dylib"
        proc = "/usr/lib/libproc.dylib"
        iconv = "/usr/lib/libiconv.2.dylib"
        for libraries in ([system], [system, proc], [system, iconv], [system, proc, iconv]):
            self.assertEqual(linkage.dynamic_libraries(commands(libraries), linkage.SESSION_OPTIONAL_DYLIBS), libraries)
        for library in (proc, iconv):
            with self.subTest(adapter_library=library), self.assertRaisesRegex(Rejected, "unexplained"):
                linkage.dynamic_libraries(commands([system, library]))  # Adapter remains System only.
        for libraries in ([proc], [system, system], [system, iconv, iconv],
                          [system, "/usr/lib/libiconv.dylib"],
                          [system, "/opt/homebrew/lib/libiconv.2.dylib"],
                          [system, "/opt/homebrew/lib/libssl.dylib"]):
            with self.subTest(libraries=libraries), self.assertRaisesRegex(Rejected, "unexplained"):
                linkage.dynamic_libraries(commands(libraries), linkage.SESSION_OPTIONAL_DYLIBS)
        with self.assertRaisesRegex(Rejected, "search path"):
            linkage.dynamic_libraries(commands([system], rpath=True), linkage.SESSION_OPTIONAL_DYLIBS)


if __name__ == "__main__":
    unittest.main()
