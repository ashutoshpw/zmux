"""Real stream/lifecycle contracts for package-local Cargo metadata capture."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("bounded_terminal_owner", HERE.parent / "terminal-feasibility/run.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class StructuredStdout(unittest.TestCase):
    def command(self, directory, script, *arguments):
        return [sys.executable, "-B", HERE / "capture-stdout.py", "--output", directory / "metadata.json", "--",
                sys.executable, "-c", script, *arguments]

    def assert_stopped(self, pid):
        deadline = time.monotonic() + 2
        while True:
            status = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], capture_output=True, text=True, timeout=2).stdout.strip()
            if not status or status.startswith("Z"):
                return
            if time.monotonic() >= deadline:
                self.fail("structured capture left a live owned command descendant")
            time.sleep(.01)

    def test_stderr_progress_cannot_enter_structured_stdout(self):
        with tempfile.TemporaryDirectory(prefix="zmux-metadata-streams-") as temporary:
            directory = Path(temporary)
            phases = []
            runner.run(self.command(directory, """
import json, os, threading
worker = threading.Thread(target=lambda: os.write(2, b'Downloading crates...\\n' + b'x' * 2097152))
worker.start()
os.write(1, json.dumps({'packages': ['actual stdout'], 'version': 1}).encode() + b'\\n')
worker.join()
"""), "metadata", directory, phases, timeout=3)
            self.assertEqual(json.loads((directory / "metadata.json").read_text()), {"packages": ["actual stdout"], "version": 1})
            self.assertIn(b'Downloading crates...', (directory / "metadata.log").read_bytes())
            self.assertEqual(phases[0]["log_bytes_seen"], len(b'Downloading crates...\n') + 2097152)
            self.assertEqual(phases[0]["log_bytes_retained"], runner.MAX_LOG_BYTES)

    def test_stdout_overflow_fails_with_a_bounded_sink(self):
        with tempfile.TemporaryDirectory(prefix="zmux-metadata-overflow-") as temporary:
            directory = Path(temporary)
            phases = []
            with self.assertRaises(RuntimeError):
                runner.run(self.command(directory, "import os; os.write(1, b'x' * (9 * 1024 * 1024))"),
                           "overflow", directory, phases, timeout=3)
            self.assertFalse(phases[0]["timed_out"])
            self.assertEqual((directory / "metadata.json").stat().st_size, 8 * 1024 * 1024)
            self.assertIn("structured stdout exceeded 8 MiB", (directory / "overflow.log").read_text())

    def test_an_existing_sink_is_never_overwritten_or_executed(self):
        with tempfile.TemporaryDirectory(prefix="zmux-metadata-existing-") as temporary:
            directory = Path(temporary)
            sink = directory / "metadata.json"
            sink.write_bytes(b"retained original bytes")
            marker = directory / "must-not-run"
            with self.assertRaises(RuntimeError):
                runner.run(self.command(directory, "from pathlib import Path; import sys; Path(sys.argv[1]).touch()", marker),
                           "existing", directory, [], timeout=3)
            self.assertEqual(sink.read_bytes(), b"retained original bytes")
            self.assertFalse(marker.exists())

    def test_valid_stdout_does_not_hide_a_failed_command(self):
        with tempfile.TemporaryDirectory(prefix="zmux-metadata-failure-") as temporary:
            directory = Path(temporary)
            phases = []
            with self.assertRaises(RuntimeError):
                runner.run(self.command(directory, "import sys; print('{\"version\":1}'); sys.exit(7)"),
                           "failed", directory, phases, timeout=3)
            self.assertEqual(phases[0]["exit_code"], 7)
            self.assertEqual(json.loads((directory / "metadata.json").read_text()), {"version": 1})

    def exercise_lifecycle(self, script, expires):
        with tempfile.TemporaryDirectory(prefix="zmux-metadata-lifetime-") as temporary:
            directory = Path(temporary)
            identity = directory / "child.json"
            phases = []
            began = time.monotonic()
            if expires:
                with self.assertRaises(RuntimeError):
                    runner.run(self.command(directory, script, identity), "lifetime", directory, phases, timeout=.4)
            else:
                runner.run(self.command(directory, script, identity), "lifetime", directory, phases, timeout=2)
            self.assertEqual(phases[0]["timed_out"], expires)
            self.assertLess(time.monotonic() - began, 3)
            for pid in json.loads(identity.read_text()).values():
                self.assert_stopped(pid)

    def test_stdout_eof_cannot_remove_the_parent_deadline(self):
        self.exercise_lifecycle("""
import json, os, sys, time
from pathlib import Path
Path(sys.argv[1]).write_text(json.dumps({'command': os.getpid()}))
os.close(1); os.close(2); time.sleep(60)
""", True)

    def test_a_descendant_holding_stdout_cannot_outlive_the_parent_deadline(self):
        self.exercise_lifecycle("""
import json, os, subprocess, sys
from pathlib import Path
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
Path(sys.argv[1]).write_text(json.dumps({'command': os.getpid(), 'descendant': child.pid}))
""", True)

    def test_a_closed_stdio_descendant_is_stopped_after_success(self):
        self.exercise_lifecycle("""
import json, os, subprocess, sys
from pathlib import Path
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
Path(sys.argv[1]).write_text(json.dumps({'command': os.getpid(), 'descendant': child.pid}))
""", False)


if __name__ == "__main__":
    unittest.main()
