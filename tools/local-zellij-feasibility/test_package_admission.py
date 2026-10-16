"""Archive/filesystem and exact-download contracts at the package admission owner."""
import copy
import gzip
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile
import shutil
import subprocess
import sys

import package_admission as admission


def archive(entries):
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as stream:
        for name, value, kind in entries:
            item = tarfile.TarInfo(name)
            item.type = kind
            item.size = len(value) if kind == tarfile.REGTYPE else 0
            item.linkname = "../../outside" if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE) else ""
            stream.addfile(item, io.BytesIO(value) if item.isfile() else None)
    return gzip.compress(raw.getvalue())


class PackageAdmission(unittest.TestCase):
    def test_regular_archive_bytes_are_extracted_exclusively(self):
        files = admission.archive_files(archive([("legal/notice", b"original legal bytes\n", tarfile.REGTYPE)]))
        with tempfile.TemporaryDirectory(prefix="zmux-package-extract-") as temporary:
            output = Path(temporary) / "owned"
            admission.extract_files(files, output)
            self.assertEqual((output / "legal/notice").read_bytes(), b"original legal bytes\n")
            with self.assertRaises(FileExistsError):
                admission.extract_files({"legal/notice": b"replacement"}, output)
            self.assertEqual((output / "legal/notice").read_bytes(), b"original legal bytes\n")

    def test_path_aliases_traversal_and_controls_refuse_before_writes(self):
        for name in ("../outside", "/outside", "a/./b", "a//b", "a\\b", ".", "a\nb"):
            with self.subTest(name=name), self.assertRaisesRegex(admission.Rejected, "package path"):
                admission.archive_files(archive([(name, b"bad", tarfile.REGTYPE)]))

    def test_links_devices_duplicate_files_and_parent_conflicts_refuse(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.FIFOTYPE, tarfile.DIRTYPE):
            with self.subTest(kind=kind), self.assertRaisesRegex(admission.Rejected, "nonregular"):
                admission.archive_files(archive([("member", b"", kind)]))
        with self.assertRaisesRegex(admission.Rejected, "duplicate"):
            admission.archive_files(archive([("same", b"one", tarfile.REGTYPE), ("same", b"two", tarfile.REGTYPE)]))
        with self.assertRaisesRegex(admission.Rejected, "parent directory"):
            admission.archive_files(archive([("a", b"one", tarfile.REGTYPE), ("a/b", b"two", tarfile.REGTYPE)]))

    def test_oversized_member_is_rejected_without_reading_its_body(self):
        item = tarfile.TarInfo("oversized")
        item.size = 64 * 1024 * 1024 + 1
        data = gzip.compress(item.tobuf() + b"\0" * 1024)
        with self.assertRaisesRegex(admission.Rejected, "oversized"):
            admission.archive_files(data)

    def test_regular_file_reader_does_not_follow_links_or_block_on_fifo(self):
        with tempfile.TemporaryDirectory(prefix="zmux-package-files-") as temporary:
            root = Path(temporary)
            original = root / "source"
            original.write_bytes(b"original")
            link = root / "link"
            link.symlink_to(original)
            with self.assertRaises(OSError):
                admission.read_file(link, 32)
            fifo = root / "fifo"
            import os
            os.mkfifo(fifo)
            with self.assertRaisesRegex(admission.Rejected, "regular file"):
                admission.read_file(fifo, 32)
            with self.assertRaisesRegex(admission.Rejected, "byte bound"):
                admission.read_file(original, 2)

    def test_build_admission_refuses_unverified_native_architectures(self):
        self.assertEqual(admission.native_target("host", "Darwin", "arm64"), "aarch64-apple-darwin")
        for arguments in (("universal", "Darwin", "arm64"), ("host", "Darwin", "x86_64"), ("host", "Linux", "arm64")):
            with self.subTest(arguments=arguments), self.assertRaisesRegex(admission.Rejected, "Intel/universal"):
                admission.native_target(*arguments)

    def test_builder_refusal_preserves_an_existing_generated_bundle(self):
        # Exercise the real public builder, with its filesystem effects fenced
        # inside an owned checkout-shaped directory rather than the user's dist.
        with tempfile.TemporaryDirectory(prefix="zmux-builder-refusal-") as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            tools = root / "tools/local-zellij-feasibility"
            tools.mkdir(parents=True)
            shutil.copy2(admission.HERE.parent.parent / "scripts/build-macos-dev", root / "scripts/build-macos-dev")
            shutil.copy2(admission.HERE / "package_admission.py", tools / "package_admission.py")
            bundle = root / "dist/zmux.app"
            bundle.mkdir(parents=True)
            sentinel = bundle / "retained-user-build"
            sentinel.write_bytes(b"previous development bundle")
            before = sentinel.stat()
            cases = [(["--with-zellij"], "must be supplied together"),
                     (["--with-zellij", "--zellij-package-root", "/missing"], "Intel/universal are unverified")]
            if sys.platform == "linux":
                cases.append((["--architecture", "host", "--with-zellij", "--zellij-package-root", "/missing"], "Intel/universal are unverified"))
            for arguments, reason in cases:
                result = subprocess.run([sys.executable, "-B", str(root / "scripts/build-macos-dev"), *arguments],
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 2)
                self.assertIn(reason, result.stderr)
                self.assertEqual(sentinel.read_bytes(), b"previous development bundle")
                self.assertEqual(sentinel.stat().st_mtime_ns, before.st_mtime_ns)
                self.assertEqual(list(bundle.iterdir()), [sentinel])

    def test_changed_or_expired_download_identity_has_no_fallback(self):
        accepted = admission.pin()
        download = accepted["download"]
        metadata = {"id": 12014537209, "name": download["name"], "expired": False,
                    "expires_at": "2026-10-22T23:40:05Z", "size_in_bytes": 62648250,
                    "digest": "sha256:0960c93edcd6c8a0efb1d04398f874043ba32402c0860fad2354140cb92820fc",
                    "workflow_run": {"id": 38823056442, "head_sha": "986600f2a258a20f896083a4407f08b3778afa2c"}}
        admission.verify_download(metadata, accepted)
        for key, value in (("id", 0), ("name", "latest"), ("expired", True), ("expires_at", "2099-01-01T00:00:00Z"),
                           ("size_in_bytes", 1), ("digest", "sha256:" + "0" * 64), ("workflow_run", {"id": 0, "head_sha": "0" * 40})):
            changed = copy.deepcopy(metadata)
            changed[key] = value
            with self.subTest(key=key), self.assertRaisesRegex(admission.Rejected, "no fallback"):
                admission.verify_download(changed, accepted)

    def test_zip_aliases_links_duplicates_and_changed_bytes_refuse_before_extraction(self):
        name = "accepted.tar.gz"
        link = zipfile.ZipInfo(name)
        link.create_system = 3
        import stat
        link.external_attr = (stat.S_IFLNK | 0o777) << 16
        cases = [([(name, b"tar"), (name + ".sha256", b"hash")], True, "ZIP digest/size"),
                 ([('../' + name, b"tar"), (name + '.sha256', b"hash")], False, "ZIP inventory"),
                 ([(name, b"tar"), (name, b"duplicate")], False, "ZIP inventory"),
                 ([(link, b"../../outside"), (name + '.sha256', b"hash")], False, "ZIP member type")]
        import warnings
        for entries, changed_digest, reason in cases:
            with self.subTest(reason=reason), tempfile.TemporaryDirectory(prefix="zmux-package-zip-") as temporary:
                root = Path(temporary)
                path = root / "input.zip"
                with warnings.catch_warnings():
                    warnings.simplefilter("ignore", UserWarning)  # Deliberate duplicate ZIP member.
                    with zipfile.ZipFile(path, "w") as zipped:
                        for item, data in entries:
                            zipped.writestr(item, data)
                raw = path.read_bytes()
                accepted = {"archive_name": name, "download": {"zip_bytes": len(raw),
                            "zip_sha256": "0" * 64 if changed_digest else admission.digest(raw)}}
                output = root / "output"
                with self.assertRaisesRegex(admission.Rejected, reason):
                    admission.unpack_download(path, output, accepted)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
