"""Retain a spawned process-group identity until cleanup signals finish."""
import errno
from contextlib import ExitStack
import os
import platform
import selectors
import signal
import subprocess
import time


def confirm_darwin_zombie_group(process):
    """Accept EPERM only for a reserved, entirely exited Darwin process group."""
    if (platform.system() != "Darwin" or process.returncode is not None
            or os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None):
        raise RuntimeError("Permission denied while the owned group leader may still execute")
    probe = subprocess.Popen(["/bin/ps", "-A", "-o", "pid=,pgid=,stat="],
                             stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL)
    selector = selectors.DefaultSelector()
    output = bytearray()
    deadline = time.monotonic() + 2
    try:
        selector.register(probe.stdout, selectors.EVENT_READ)
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise RuntimeError("Public process-group inspection timed out")
            for key, _ in selector.select(0.03):
                data = os.read(key.fileobj.fileno(), 16384)
                if not data:
                    selector.unregister(key.fileobj)
                if len(output) + len(data) > 65536:
                    raise RuntimeError("Public process-group inspection exceeded its byte bound")
                output.extend(data)
        if probe.wait(timeout=1) != 0:
            raise RuntimeError("Public process-group inspection failed")
    finally:
        selector.close()
        probe.stdout.close()
        if probe.returncode is None:
            # This direct child remains unreaped; no adopted/global PID signal.
            probe.kill()
        probe.wait(timeout=2)
    members = []
    for row in output.decode("ascii", errors="strict").splitlines():
        fields = row.split()
        if len(fields) != 3:
            raise RuntimeError("Malformed public process-group snapshot")
        pid, group = int(fields[0]), int(fields[1])
        if group == process.pid:
            members.append({"pid": pid, "state": fields[2]})
    if not any(member["pid"] == process.pid for member in members):
        raise RuntimeError("Public snapshot omitted the unreaped group leader")
    if not all(member["state"].startswith("Z") for member in members):
        raise RuntimeError("Permission denied with live process-group members: " + str(members))
    return members


class ProcessGroup:
    """Own spawning, nonreaping observation, group signalling and final reaping.

    Callers retain their stream-capture and deadline policies. They may read the
    child's pipes, but must leave poll/wait and cleanup signals to this owner.
    """
    def __init__(self, command, **options):
        if not hasattr(os, "waitid") or not hasattr(os, "WNOWAIT"):
            raise RuntimeError("Nonreaping owned-child wait API unavailable")
        self.process = subprocess.Popen(command, start_new_session=True, **options)
        self.confirmed_zombie_group = None
        self.reap_retried = False
        self.closed = False

    def exited(self):
        if self.closed or self.process.returncode is not None:
            raise RuntimeError("The process-group identity has already been released")
        # WNOWAIT reserves the real child PID even after execution has ended.
        return os.waitid(os.P_PID, self.process.pid,
                         os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None

    def stop(self):
        self.exited()  # Verify child ownership before any irreversible signal.
        try:
            os.killpg(self.process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError as error:
            if error.errno != errno.EPERM:
                raise
            self.confirmed_zombie_group = confirm_darwin_zombie_group(self.process)

    def close(self, *, timeout, retry=False):
        if self.closed:
            return self.process.returncode
        try:
            # Close every owned pipe even if one close reports an error, then
            # still stop/reap the child. Capture setup can fail after spawning.
            with ExitStack() as cleanup:
                for pipe in (self.process.stdin, self.process.stdout, self.process.stderr):
                    if pipe is not None:
                        cleanup.callback(pipe.close)
        finally:
            try:
                self.stop()
            finally:
                try:
                    try:
                        code = self.process.wait(timeout=timeout)
                    except subprocess.TimeoutExpired as error:
                        if not retry:
                            raise RuntimeError("Owned group leader did not exit after cleanup") from error
                        self.reap_retried = True
                        try:
                            self.stop()
                        finally:
                            code = self.process.wait(timeout=timeout)
                finally:
                    self.closed = self.process.returncode is not None
        return code
