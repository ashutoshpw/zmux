import sys
import struct
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import manifest


def elf64_fixture(*, machine=62, elf_type=2, interpreter=None, dynamic_tags=None):
    segments = [(1, b"\0")]
    if interpreter is not None:
        segments.append((3, interpreter.encode() + b"\0"))
    if dynamic_tags is not None:
        segments.append((2, b"".join(struct.pack("<qQ", tag, value) for tag, value in dynamic_tags)))

    table_offset = 64
    data_offset = table_offset + 56 * len(segments)
    headers = []
    payloads = []
    current_offset = data_offset
    for segment_type, payload in segments:
        headers.append(struct.pack(
            "<IIQQQQQQ", segment_type, 4, current_offset, 0x1000 if segment_type == 1 else 0, 0,
            len(payload), len(payload), 1,
        ))
        payloads.append(payload)
        current_offset += len(payload)

    ident = b"\x7fELF\x02\x01\x01" + (b"\0" * 9)
    header = struct.pack(
        "<16sHHIQQQIHHHHHH", ident, elf_type, machine, 1, 0x1000,
        table_offset if segments else 0, 0, 0, 64, 56, len(segments), 0, 0, 0,
    )
    return header + b"".join(headers) + b"".join(payloads)


class SchedulerManifestTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source_sha = "a" * 40
        self.key = "linux-x86_64"
        self.binary = self.root / self.key / "zmux-scheduler"
        self.binary.parent.mkdir()
        self.binary.write_bytes(elf64_fixture())
        self.binary.chmod(0o755)
        self.value = manifest.make_manifest(self.source_sha, {self.key: manifest.sha256(self.binary)})

    def test_valid_per_target_receipt_matches_bytes_and_source(self):
        checked = manifest.validate_manifest(
            self.value,
            self.root,
            expected_source_sha=self.source_sha,
            required_keys={self.key},
        )
        self.assertEqual(checked["helpers"][self.key]["format"], "elf")
        self.assertEqual(checked["helpers"][self.key]["linkage"], "static-musl")

    def test_rejects_changed_binary_or_source(self):
        self.binary.write_bytes(b"changed binary")
        with self.assertRaisesRegex(manifest.ManifestError, "SHA-256"):
            manifest.validate_manifest(self.value, self.root)
        self.binary.write_bytes(elf64_fixture())
        self.binary.chmod(0o755)
        with self.assertRaisesRegex(manifest.ManifestError, "source SHA"):
            manifest.validate_manifest(self.value, self.root, expected_source_sha="b" * 40)

    def test_rejects_wrong_entry_shape_and_missing_required_architecture(self):
        extra = {**self.value, "unreviewed": True}
        with self.assertRaisesRegex(manifest.ManifestError, "top-level shape"):
            manifest.validate_manifest(extra, self.root)
        with self.assertRaisesRegex(manifest.ManifestError, "missing required helpers"):
            manifest.validate_manifest(self.value, self.root, required_keys={"linux-aarch64"})

    def test_rejects_symlinked_helper(self):
        self.binary.unlink()
        alternate = self.root / "alternate"
        alternate.write_bytes(elf64_fixture())
        alternate.chmod(0o755)
        self.binary.symlink_to(alternate)
        with self.assertRaisesRegex(manifest.ManifestError, "regular executable"):
            manifest.validate_manifest(self.value, self.root)

    def test_accepts_static_pie_with_relocation_dynamic_section(self):
        self.binary.write_bytes(elf64_fixture(elf_type=3, dynamic_tags=[(7, 0), (0, 0)]))
        self.binary.chmod(0o755)
        value = manifest.make_manifest(self.source_sha, {self.key: manifest.sha256(self.binary)})
        manifest.validate_manifest(value, self.root)

    def test_rejects_dynamic_interpreter_and_needed_library(self):
        for fixture in (
            elf64_fixture(interpreter="/lib/ld-musl-x86_64.so.1"),
            elf64_fixture(elf_type=3, dynamic_tags=[(1, 1), (0, 0)]),
        ):
            with self.subTest(fixture=fixture[64:72]):
                self.binary.write_bytes(fixture)
                self.binary.chmod(0o755)
                value = manifest.make_manifest(self.source_sha, {self.key: manifest.sha256(self.binary)})
                with self.assertRaisesRegex(manifest.ManifestError, "dynamic|interpreter"):
                    manifest.validate_manifest(value, self.root)

    def test_rejects_wrong_elf_architecture(self):
        self.binary.write_bytes(elf64_fixture(machine=183))
        self.binary.chmod(0o755)
        value = manifest.make_manifest(self.source_sha, {self.key: manifest.sha256(self.binary)})
        with self.assertRaisesRegex(manifest.ManifestError, "architecture"):
            manifest.validate_manifest(value, self.root)


if __name__ == "__main__":
    unittest.main()
