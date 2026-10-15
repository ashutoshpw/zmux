"""Real subprocess lifecycle and output bounds for offline verification runners."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("terminal_feasibility", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

browser_spec = importlib.util.spec_from_file_location(
    "browser_process", Path(__file__).resolve().parents[2] / "scripts/browser_process.py")
browser = importlib.util.module_from_spec(browser_spec)
sys.modules[browser_spec.name] = browser
browser_spec.loader.exec_module(browser)


class ProcessGroupIdentity(unittest.TestCase):
    def test_capture_setup_failure_stops_reaps_and_closes_the_actual_child(self):
        spawn = subprocess.Popen
        for owner in ("terminal", "browser"):
            with self.subTest(owner=owner), tempfile.TemporaryDirectory(prefix="zmux-feasibility-setup-") as temporary:
                children = []

                def record_spawn(*arguments, **options):
                    process = spawn(*arguments, **options)
                    children.append(process)
                    return process

                try:
                    with patch.object(subprocess, "Popen", record_spawn), patch.object(
                            runner.selectors, "DefaultSelector", side_effect=OSError("capture setup failed")):
                        command = [sys.executable, "-c", "import time; time.sleep(60)"]
                        with self.assertRaisesRegex(OSError, "capture setup failed"):
                            if owner == "terminal":
                                runner.run(command, "setup", Path(temporary), [], timeout=2)
                            else:
                                browser.run_browser(command, timeout=2)
                    self.assertEqual(len(children), 1)
                    process = children[0]
                    self.assertIsNotNone(process.returncode, "Capture setup failure left its actual child unreaped")
                    self.assertTrue(process.stdout.closed)
                    self.assertTrue(process.stderr is None or process.stderr.closed)
                    with self.assertRaises(ChildProcessError):
                        os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                finally:
                    # The test retains these direct children if the regression
                    # fails; cleanup never signals an adopted/global group PID.
                    for process in children:
                        if process.returncode is None:
                            process.kill()
                            process.wait(timeout=2)
                        for pipe in (process.stdout, process.stderr):
                            if pipe is not None:
                                pipe.close()

    def test_group_cleanup_retains_the_real_child_identity_until_after_signalling(self):
        # Observe the actual kernel-owned child at the irreversible signal
        # boundary, then forward the real signal. No PID reuse/churn is needed
        # to detect that a reaped leader no longer reserves its group identity.
        signal_group = os.killpg
        for owner in ("terminal", "browser"):
            with self.subTest(owner=owner), tempfile.TemporaryDirectory(prefix="zmux-feasibility-identity-") as temporary:
                signals = []

                def checked_signal(group, value):
                    try:
                        status = os.waitid(os.P_PID, group, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                    except ChildProcessError:
                        signals.append((group, "released"))
                        return  # Never send a test signal after ownership was lost.
                    signals.append((group, status))
                    signal_group(group, value)

                command = [sys.executable, "-c", "print('owned leader completed')"]
                with patch.object(os, "killpg", checked_signal):
                    if owner == "terminal":
                        phases = []
                        runner.run(command, "identity", Path(temporary), phases, timeout=2)
                        code, timed_out = phases[0]["exit_code"], phases[0]["timed_out"]
                    else:
                        result = browser.run_browser(command, timeout=2)
                        code, timed_out = result.returncode, result.timed_out
                self.assertTrue(signals, "The process group was never physically cleaned up")
                self.assertTrue(all(status != "released" for _, status in signals),
                                "Group cleanup signalled after its owned child identity was released")
                self.assertTrue(any(status is not None for _, status in signals),
                                "The completed child was not retained until group cleanup")
                self.assertEqual(code, 0)
                self.assertFalse(timed_out)
                with self.assertRaises(ChildProcessError):
                    os.waitid(os.P_PID, signals[0][0], os.WEXITED | os.WNOHANG | os.WNOWAIT)


class RunnerLifecycle(unittest.TestCase):
    def run_process(self, command, name, directory, phases, *, timeout):
        return runner.run(command, name, directory, phases, timeout=timeout)

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
            command = [sys.executable, "-c", script, str(identity)]
            if expires:
                with self.assertRaises(RuntimeError):
                    self.run_process(command, "owned", directory, phases, timeout=timeout)
            else:
                self.run_process(command, "owned", directory, phases, timeout=timeout)
            self.assertEqual(phases[0]["timed_out"], expires, phases)
            self.assertLess(time.monotonic() - began, timeout + 2, phases)
            owned = json.loads(identity.read_text())
            self.assert_not_running(owned["child"])
            # run_process owns cleanup before reaping. Its returned leader PID
            # is no longer a reserved identity and must never be signalled here.

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
            self.run_process([sys.executable, "-c", "import os; os.write(1, b'x' * 2097152)"],
                             "output", directory, phases, timeout=3)
            self.assertEqual(phases[0]["log_bytes_seen"], 2097152)
            self.assertEqual((directory / "output.log").stat().st_size, 1048576)


class BrowserLifecycle(RunnerLifecycle):
    def run_process(self, command, name, directory, phases, *, timeout):
        # Only translate the production helper's result for the shared fixture.
        # The helper owns spawning, capture, deadlines and process-group cleanup.
        result = browser.run_browser(command, timeout)
        (directory / f"{name}.log").write_text(result.stdout)
        phases.append({"timed_out": result.timed_out,
                       "log_bytes_seen": result.stdout_bytes_seen})
        if result.timed_out or result.returncode:
            raise RuntimeError(f"Browser subprocess failed: {result}")
        return result.stdout

    def test_both_streams_drain_independently_with_bounded_prefix_and_tail(self):
        result = browser.run_browser([sys.executable, "-c", """
import os, threading
def write_stdout():
    os.write(1, b'x' * 2097152)
worker = threading.Thread(target=write_stdout)
worker.start()
os.write(2, b'y' * 2097152 + b'END')
worker.join()
"""], timeout=3)
        self.assertEqual(result.returncode, 0)
        self.assertFalse(result.timed_out)
        self.assertEqual(result.stdout_bytes_seen, 2097152)
        self.assertEqual(result.stderr_bytes_seen, 2097155)
        self.assertEqual(result.stdout, 'x' * 1048576)
        self.assertEqual(result.stderr, 'y' * 65533 + 'END')
        self.assertTrue(result.output_truncated)


if __name__ == "__main__":
    unittest.main()
