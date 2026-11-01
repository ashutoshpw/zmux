#!/usr/bin/env python3
"""Multiplex sanitized hook frames and SSH-channel heartbeats to stdout."""

import fcntl
import json
import os
import re
import select
import stat
import sys
import time

VERSION = 2
MAX_LINE_BYTES = 1024
HEARTBEAT_SECONDS = 5.0
FIFO_WAIT_SECONDS = 20.0


def send(frame):
    sys.stdout.buffer.write(
        json.dumps(frame, separators=(",", ":"), ensure_ascii=True).encode("ascii") + b"\n"
    )
    sys.stdout.buffer.flush()


def activate(nonce, root):
    raw = sys.stdin.buffer.readline(MAX_LINE_BYTES + 1)
    if len(raw) > MAX_LINE_BYTES:
        return None
    try:
        frame = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        return None
    if (
        not isinstance(frame, dict)
        or frame.get("kind") != "activate"
        or frame.get("version") != VERSION
        or frame.get("nonce") != nonce
    ):
        return None
    path = os.path.join(root, "agent-lifecycle-active.lock")
    try:
        descriptor = os.open(
            path,
            os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC,
            0o600,
        )
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            os.close(descriptor)
            return None
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except (BlockingIOError, OSError):
        try:
            os.close(descriptor)
        except (NameError, OSError):
            pass
        return None
    send({"kind": "active", "version": VERSION, "nonce": nonce})
    return descriptor


def main():
    nonce = sys.argv[1] if len(sys.argv) == 2 else ""
    root = os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", "")
    if not re.fullmatch(r"[0-9a-f]{32}", nonce) or not root or not os.path.isabs(root):
        return 70
    fifo = os.path.join(root, "agent-events")
    loss_fifo = os.path.join(root, "agent-events-loss")
    deadline = time.monotonic() + FIFO_WAIT_SECONDS
    while time.monotonic() < deadline:
        try:
            if (
                stat.S_ISFIFO(os.stat(fifo, follow_symlinks=False).st_mode)
                and stat.S_ISFIFO(os.stat(loss_fifo, follow_symlinks=False).st_mode)
            ):
                break
        except OSError:
            pass
        time.sleep(0.05)
    else:
        return 71

    try:
        reader = os.open(fifo, os.O_RDONLY | os.O_NONBLOCK)
        # Keep the FIFO readable while no hook subprocess currently has it open.
        writer_hold = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
        loss_reader = os.open(loss_fifo, os.O_RDONLY | os.O_NONBLOCK)
        loss_writer_hold = os.open(loss_fifo, os.O_WRONLY | os.O_NONBLOCK)
    except OSError:
        return 72

    send({"kind": "hello", "version": VERSION, "nonce": nonce})
    active_descriptor = activate(nonce, root)
    if active_descriptor is None:
        os.close(reader)
        os.close(writer_hold)
        os.close(loss_reader)
        os.close(loss_writer_hold)
        return 74
    pending = bytearray()
    sequence = 0
    next_heartbeat = time.monotonic() + HEARTBEAT_SECONDS
    try:
        while os.path.exists(fifo):
            timeout = max(0.0, min(0.5, next_heartbeat - time.monotonic()))
            readable, _, _ = select.select(
                [reader, loss_reader, sys.stdin.buffer], [], [], timeout
            )
            if sys.stdin.buffer in readable:
                if sys.stdin.buffer.read(1):
                    return 75
                return 0
            if loss_reader in readable and os.read(loss_reader, 4096):
                sequence += 1
                send(
                    {
                        "kind": "gap",
                        "version": VERSION,
                        "nonce": nonce,
                        "sequence": sequence,
                    }
                )
                return 74
            if readable:
                chunk = os.read(reader, 4096) if reader in readable else b""
                if chunk:
                    pending.extend(chunk)
                    if len(pending) > MAX_LINE_BYTES and b"\n" not in pending:
                        return 73
                    while b"\n" in pending:
                        line, _, remainder = pending.partition(b"\n")
                        pending = bytearray(remainder)
                        if not line or len(line) > MAX_LINE_BYTES:
                            continue
                        try:
                            supplied = json.loads(line)
                        except (ValueError, UnicodeDecodeError):
                            continue
                        if (
                            not isinstance(supplied, dict)
                            or supplied.get("nonce") != nonce
                            or not isinstance(supplied.get("event"), dict)
                        ):
                            continue
                        sequence += 1
                        send(
                            {
                                "kind": "hook",
                                "version": VERSION,
                                "nonce": nonce,
                                "sequence": sequence,
                                "event": supplied["event"],
                            }
                        )
                elif pending:
                    # An unterminated partial record is never repaired into a
                    # lifecycle event.
                    pending.clear()
            now = time.monotonic()
            if now >= next_heartbeat:
                sequence += 1
                send(
                    {
                        "kind": "heartbeat",
                        "version": VERSION,
                        "nonce": nonce,
                        "sequence": sequence,
                    }
                )
                next_heartbeat = now + HEARTBEAT_SECONDS
    finally:
        os.close(reader)
        os.close(writer_hold)
        os.close(loss_reader)
        os.close(loss_writer_hold)
        os.close(active_descriptor)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
