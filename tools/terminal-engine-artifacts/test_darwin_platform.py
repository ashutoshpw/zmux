"""Use real Clang-produced Mach-O objects; no engine or macOS SDK build is needed."""

from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

from darwin_platform import inspect_darwin_archive, inspect_macho
from verify import Rejected


class DarwinPlatformChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="zmux-macho-platform-")
        cls.root = Path(cls.temporary.name)
        cls.clang = shutil.which("clang")
        if not cls.clang:
            raise RuntimeError("Clang is required for actual Mach-O fixture compilation")
        cls.source = cls.root / "consumer.c"
        cls.source.write_text("long zmux_macho_probe(long value) { return value + 7; }\n")
        cls.objects = {arch: cls.compile_object(arch, f"{arch}-apple-macos14.0")
                       for arch in ("arm64", "x86_64")}

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @classmethod
    def compile_object(cls, name, target, extra=()):
        path = cls.root / (name + ".o")
        subprocess.run([cls.clang, "-target", target, *extra, "-c", str(cls.source), "-o", str(path)],
                       check=True, capture_output=True, timeout=15)
        return path

    def test_actual_clang_macos_objects_expose_architecture_and_deployment(self):
        for arch, path in self.objects.items():
            with self.subTest(architecture=arch):
                result = inspect_macho(path.read_bytes(), arch)
                self.assertEqual(result["minimum_macos"], [14, 0, 0])
                self.assertEqual(result["architecture"], arch)

    def test_actual_archive_reuses_bounded_parser_and_rejects_mixed_slices(self):
        archive = self.root / "arm64.a"
        subprocess.run(["ar", "rcs", str(archive), str(self.objects["arm64"])], check=True, timeout=15)
        self.assertEqual(len(inspect_darwin_archive(archive, "arm64")["members"]), 1)
        subprocess.run(["ar", "r", str(archive), str(self.objects["x86_64"])], check=True, timeout=15)
        with self.assertRaisesRegex(Rejected, "architecture mismatch"):
            inspect_darwin_archive(archive, "arm64")

    def test_actual_lower_minimum_and_wrong_sdk_are_rejected(self):
        older = self.compile_object("older", "arm64-apple-macos13.0")
        with self.assertRaisesRegex(Rejected, "minimum mismatch"):
            inspect_macho(older.read_bytes(), "arm64")
        other_sdk = self.compile_object("other-sdk", "x86_64-apple-macos14.0",
                                        ("-Xclang", "-target-sdk-version=15.4"))
        with self.assertRaisesRegex(Rejected, "SDK mismatch"):
            inspect_macho(other_sdk.read_bytes(), "x86_64")

    def test_actual_ios_object_cannot_substitute_for_macos(self):
        ios = self.compile_object("ios", "arm64-apple-ios17.0")
        with self.assertRaisesRegex(Rejected, "platform"):
            inspect_macho(ios.read_bytes(), "arm64")

    def test_actual_specialized_cpu_object_cannot_substitute_for_generic_slice(self):
        specialized = self.compile_object("haswell", "x86_64h-apple-macos14.0")
        with self.assertRaisesRegex(Rejected, "CPU subtype"):
            inspect_macho(specialized.read_bytes(), "x86_64")

    def test_actual_legacy_macos_load_command_is_supported_but_not_accepted_as_14(self):
        older = self.compile_object("legacy", "x86_64-apple-macos10.13")
        data = older.read_bytes()
        result = inspect_macho(data, "x86_64", minimum=(10, 13, 0))
        self.assertEqual(result["minimum_macos"], [10, 13, 0])
        with self.assertRaisesRegex(Rejected, "minimum mismatch"):
            inspect_macho(data, "x86_64")

    def test_truncated_load_command_region_of_real_object_is_rejected(self):
        data = bytearray(self.objects["arm64"].read_bytes())
        struct.pack_into("<I", data, 20, len(data) + 1)
        with self.assertRaisesRegex(Rejected, "bounds"):
            inspect_macho(data, "arm64")


if __name__ == "__main__":
    unittest.main()
