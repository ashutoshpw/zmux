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
import warnings
import zipfile

from verify import Rejected, digest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("darwin_linkage", HERE / "check-darwin-linkage.py")
linkage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(linkage)


class RetainedLinkageChecks(unittest.TestCase):
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
