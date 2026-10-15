"""Real subprocess lifecycle and output bounds for the feasibility orchestrator."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


spec = importlib.util.spec_from_file_location("terminal_feasibility", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerLifecycle(unittest.TestCase):
    def assert_not_running(self, pid):
        # An orphan may remain a zombie until the OS reaps it. Test that no
        # live task survives, without mistaking kill(pid, 0) for a liveness check.
        deadline = time.monotonic() + 2
        while True:
            status = subprocess.run(["ps", "-p", str(pid), "-o", "stat="],
                                    capture_output=True, text=True, timeout=2)
            value = status.stdout.strip()
            if not value or value.startswith("Z"):
                return
            if time.monotonic() >= deadline:
                self.fail(f"owned descendant {pid} is still running: {value}")
            time.sleep(0.01)

    def exercise_child(self, script, *, timeout, expires):
        with tempfile.TemporaryDirectory(prefix="zmux-feasibility-lifecycle-") as temporary:
            directory = Path(temporary)
            identity = directory / "owned.json"
            phases = []
            began = time.monotonic()
            try:
                command = [sys.executable, "-c", script, str(identity)]
                if expires:
                    with self.assertRaises(RuntimeError):
                        runner.run(command, "owned", directory, phases, timeout=timeout)
                else:
                    runner.run(command, "owned", directory, phases, timeout=timeout)
                self.assertEqual(phases[0]["timed_out"], expires, phases)
                self.assertLess(time.monotonic() - began, timeout + 2, phases)
                owned = json.loads(identity.read_text())
                self.assert_not_running(owned["child"])
            finally:
                if identity.exists():
                    owned = json.loads(identity.read_text())
                    try:
                        os.killpg(owned["leader"], signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    def test_stdout_eof_preserves_deadline_and_stops_the_whole_group(self):
        self.exercise_child("""
import json, os, subprocess, sys, time
from pathlib import Path
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL)
Path(sys.argv[1]).write_text(json.dumps({'leader': os.getpid(), 'child': child.pid}))
os.close(1)
os.close(2)
time.sleep(60)
""", timeout=0.5, expires=True)

    def test_successful_leader_cannot_leave_a_closed_stdout_descendant_running(self):
        self.exercise_child("""
import json, os, subprocess, sys
from pathlib import Path
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL)
Path(sys.argv[1]).write_text(json.dumps({'leader': os.getpid(), 'child': child.pid}))
""", timeout=1, expires=False)

    def test_exited_leader_with_stdout_holding_descendant_is_still_bounded(self):
        self.exercise_child("""
import json, os, subprocess, sys
from pathlib import Path
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
Path(sys.argv[1]).write_text(json.dumps({'leader': os.getpid(), 'child': child.pid}))
""", timeout=0.5, expires=True)

    def test_output_is_drained_past_the_retained_log_limit(self):
        with tempfile.TemporaryDirectory(prefix="zmux-feasibility-output-") as temporary:
            directory = Path(temporary)
            phases = []
            runner.run([sys.executable, "-c", "import os; os.write(1, b'x' * 2097152)"],
                       "output", directory, phases, timeout=3)
            self.assertEqual(phases[0]["log_bytes_seen"], 2097152)
            self.assertEqual((directory / "output.log").stat().st_size, 1048576)


if __name__ == "__main__":
    unittest.main()
