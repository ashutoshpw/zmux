"""Exercise real compiler archives and retained notice failures without an engine build."""

import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("engine_artifact_verify", HERE / "verify.py")
verify = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verify)


class ArtifactAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="zmux-artifact-verifier-")
        cls.root = Path(cls.temporary.name)
        system = {"Linux": "linux", "Darwin": "macos"}[platform.system()]
        machine = {"x86_64": "x86_64", "arm64": "arm64", "aarch64": "arm64"}[platform.machine()]
        cls.target = f"{system}-{machine}"
        source = cls.root / "member-with-a-name-longer-than-ar-header.c"
        source.write_text("int zmux_artifact_probe(void) { return 7; }\n")
        obj = source.with_suffix(".o")
        cls.archive = cls.root / "misleading-arm64-name.a"
        subprocess.run(["cc", "-c", str(source), "-o", str(obj)], check=True, timeout=15)
        subprocess.run(["ar", "rcs", str(cls.archive), str(obj)], check=True, timeout=15)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_real_archive_uses_object_architecture_and_ignores_filename(self):
        result = verify.inspect_archive(self.archive, self.target)
        self.assertEqual(len(result["members"]), 1)
        self.assertEqual(result["members"][0]["architecture"], verify.TARGETS[self.target][1])
        self.assertGreater(result["members"][0]["bytes"], 32)

    def test_real_archive_rejects_wrong_architecture_and_platform(self):
        for target in verify.TARGETS:
            if target != self.target:
                with self.subTest(target=target), self.assertRaisesRegex(verify.Rejected, "object target mismatch"):
                    verify.inspect_archive(self.archive, target)

    def test_changed_or_truncated_archive_is_not_accepted(self):
        damaged = self.root / "truncated.a"
        damaged.write_bytes(self.archive.read_bytes()[:-1])
        with self.assertRaisesRegex(verify.Rejected, "truncated"):
            verify.inspect_archive(damaged, self.target)
        # A valid archive of the right architecture still cannot substitute for
        # the candidate. No invented producer receipt grants acceptance.
        target = json.loads((HERE / "candidate.json").read_text())["artifact"]["target"]
        if target == self.target:
            with self.assertRaisesRegex(verify.Rejected, "library sha256"):
                verify.verify_candidate(self.archive, HERE / "evidence/accepted-library-witness.json", target)

    def test_oversize_and_thin_archives_are_rejected(self):
        thin = self.root / "thin.a"
        thin.write_bytes(b"!<thin>\n" + self.archive.read_bytes()[8:])
        with self.assertRaisesRegex(verify.Rejected, "regular ar"):
            verify.inspect_archive(thin, self.target)
        oversized = self.root / "oversized.a"
        with oversized.open("wb") as stream:
            stream.truncate(verify.MAX_ARCHIVE_BYTES + 1)
        with self.assertRaisesRegex(verify.Rejected, "exceeds"):
            verify.inspect_archive(oversized, self.target)

    def test_named_pipe_input_is_rejected_without_waiting_for_writer(self):
        fifo = self.root / "no-writer.a"
        os.mkfifo(fifo)
        result = subprocess.run(
            [sys.executable, "-B", str(HERE / "verify.py"), "inspect",
             "--library", str(fifo), "--target", self.target],
            capture_output=True, text=True, timeout=2,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("expected a regular file", result.stderr)

    def notice_copy(self, label):
        root = self.root / label
        root.mkdir()
        shutil.copytree(HERE / "licenses", root / "licenses")
        for name in ("notices.json", "candidate.json"):
            shutil.copyfile(HERE / name, root / name)
        return root

    def test_retained_notices_pass_integrity(self):
        self.assertGreater(verify.verify_notices(), 5)

    def test_missing_or_modified_unicode_notice_rejects_package(self):
        for label in ("missing", "modified"):
            root = self.notice_copy(label)
            notice = root / "licenses/uucode-Unicode-V3"
            if label == "missing":
                notice.unlink()
                error = OSError
            else:
                notice.write_bytes(notice.read_bytes() + b"altered\n")
                error = verify.Rejected
            with self.subTest(case=label), self.assertRaises(error):
                verify.verify_notices(root)

    def test_deleting_notice_inventory_entry_does_not_bypass_verification(self):
        root = self.notice_copy("inventory-tamper")
        inventory = json.loads((root / "notices.json").read_text())
        inventory["notices"].pop()
        (root / "notices.json").write_text(json.dumps(inventory))
        with self.assertRaisesRegex(verify.Rejected, "sha256 mismatch: notices.json"):
            verify.verify_notices(root)

    def retained_candidate(self):
        manifest = json.loads((HERE / "candidate.json").read_text())
        library = Path(manifest["artifact"]["local_path"])
        if not library.is_file():
            self.skipTest("exact retained candidate archive is unavailable; no package acceptance inferred")
        self.assertEqual(verify.digest(library.read_bytes()), manifest["artifact"]["sha256"])
        return manifest, library, HERE / manifest["producer_receipt"]["path"]

    def test_exact_retained_candidate_passes_only_scoped_integrity_acceptance(self):
        manifest, library, producer = self.retained_candidate()
        result = verify.verify_candidate(library, producer, manifest["artifact"]["target"])
        self.assertEqual(result["state"], "verified")
        self.assertFalse(any(result["claims"].values()))

    def test_unresolved_provenance_rejects_the_actual_valid_retained_archive(self):
        manifest, library, producer = self.retained_candidate()
        root = self.root / "unresolved-candidate"
        shutil.copytree(HERE, root, ignore=shutil.ignore_patterns("__pycache__"))
        manifest["unresolved_notice_provenance"] = ["additional runtime notice coverage is unresolved"]
        (root / "candidate.json").write_text(json.dumps(manifest))
        with self.assertRaisesRegex(verify.Rejected, "unresolved runtime notice provenance"):
            verify.verify_candidate(library, producer, manifest["artifact"]["target"], root)


if __name__ == "__main__":
    unittest.main()
