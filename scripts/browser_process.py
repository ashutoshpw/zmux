"""Bounded capture and lifecycle ownership for the real browser DOM harness."""
from dataclasses import dataclass
import os
import selectors
import signal
import subprocess
import time


MAX_STDOUT_BYTES = 1024 * 1024
MAX_STDERR_BYTES = 64 * 1024
DRAIN_SECONDS = 1
REAP_SECONDS = 3


@dataclass(frozen=True)
class BrowserProcessResult:
    returncode: int
    stdout: str
    stderr: str
    stdout_bytes_seen: int
    stderr_bytes_seen: int
    timed_out: bool

    @property
    def output_truncated(self):
        return (self.stdout_bytes_seen > MAX_STDOUT_BYTES
                or self.stderr_bytes_seen > MAX_STDERR_BYTES)


def run_browser(command, timeout):
    """Drain both pipes under one deadline and always stop the owned process group.

    Retain at most a 1 MiB stdout prefix and 64 KiB stderr tail, while continuing
    to drain output beyond those limits. A caller must reject truncated DOM data.
    EOF is independent of leader exit: either can happen while descendants live.
    """
    deadline = time.monotonic() + timeout
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, start_new_session=True)
    selector = selectors.DefaultSelector()
    buffers = {"stdout": bytearray(), "stderr": bytearray()}
    seen = {"stdout": 0, "stderr": 0}
    timed_out = False
    killed_at = None

    def stop_group():
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass  # This invocation's group already exited.

    try:
        for name in buffers:
            pipe = getattr(process, name)
            os.set_blocking(pipe.fileno(), False)
            selector.register(pipe, selectors.EVENT_READ, name)
        while selector.get_map() or process.poll() is None:
            now = time.monotonic()
            if now >= deadline and killed_at is None:
                timed_out = True
                killed_at = now
                stop_group()
            end = deadline if killed_at is None else killed_at + DRAIN_SECONDS
            if now >= end and killed_at is not None:
                break
            for key, _ in selector.select(timeout=min(0.05, max(0, end - now))):
                try:
                    chunk = os.read(key.fileobj.fileno(), 65536)
                except BlockingIOError:
                    continue
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                name = key.data
                seen[name] += len(chunk)
                retained = buffers[name]
                if name == "stdout":
                    retained.extend(chunk[:max(0, MAX_STDOUT_BYTES - len(retained))])
                else:
                    retained.extend(chunk)
                    if len(retained) > MAX_STDERR_BYTES:
                        del retained[:-MAX_STDERR_BYTES]
    finally:
        selector.close()
        process.stdout.close()
        process.stderr.close()
        # Also stop descendants whose leader exited successfully after they
        # closed/inherited away both output streams. Never kill by browser name.
        stop_group()
        try:
            code = process.wait(timeout=REAP_SECONDS)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError("Owned browser leader did not exit after group cleanup") from error
    return BrowserProcessResult(code, buffers["stdout"].decode(errors="replace"),
                                buffers["stderr"].decode(errors="replace"),
                                seen["stdout"], seen["stderr"], timed_out)
