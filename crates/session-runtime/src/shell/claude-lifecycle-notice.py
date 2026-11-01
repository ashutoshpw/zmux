#!/usr/bin/env python3
"""Emit the generic terminal notice even when remote lifecycle transport is active."""

import json
import os
import re
import stat
import sys

MAX_INPUT_BYTES = 65_536


def write_notice(root, event):
    path = os.path.join(root, "attention")
    descriptor = os.open(
        path,
        os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC,
    )
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISFIFO(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            raise OSError("unsafe attention FIFO")
        if os.write(descriptor, (event + "\n").encode("ascii")) != len(event) + 1:
            raise OSError("short attention FIFO write")
    finally:
        os.close(descriptor)


def main():
    event = sys.argv[1] if len(sys.argv) == 2 else ""
    if event not in ("attention", "stop"):
        return 2
    nonce = os.environ.get("ZMUX_AGENT_LIFECYCLE_NONCE", "")
    root = os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", "")
    if not re.fullmatch(r"[0-9a-f]{32}", nonce) or root != f"/tmp/zmux-shell-{nonce}":
        return 2
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        return 2
    try:
        payload = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        return 2
    if not isinstance(payload, dict):
        return 2
    # Transport liveness does not prove the lifecycle owner admitted this exact
    # event. Preserve the terminal notice until a receipt can fence suppression.
    write_notice(root, event)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError):
        raise SystemExit(1)
