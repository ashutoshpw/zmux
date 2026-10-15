#!/usr/bin/env python3
"""Report bounded raw kernel PTY write capacity without changing Session policy."""
import errno
import json
import os
import platform
import select
import time
import tty


def main():
    master, slave = os.openpty()
    try:
        tty.setraw(slave)
        os.set_blocking(master, False)
        os.set_blocking(slave, False)
        writes = []
        blocked = False
        stop_reason = "attempt_limit"
        for _ in range(32):
            try:
                count = os.write(master, b"p" * 4096)
                writes.append(count)
                if count == 0:
                    stop_reason = "zero_write"
                    break
            except BlockingIOError as error:
                if error.errno not in (errno.EAGAIN, errno.EWOULDBLOCK):
                    raise
                blocked = True
                stop_reason = "would_block"
                break
        accepted = sum(writes)
        read_back = 0
        deadline = time.monotonic() + 5
        while read_back < accepted:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([slave], [], [], remaining)[0]:
                raise RuntimeError("raw PTY accepted-byte readback deadline")
            data = os.read(slave, min(4096, accepted - read_back))
            if not data or data != b"p" * len(data):
                raise RuntimeError("raw PTY did not preserve accepted bytes")
            read_back += len(data)
        print(json.dumps({"system": platform.system(), "release": platform.release(),
                          "machine": platform.machine(), "requested_per_write": 4096,
                          "write_returns": writes, "accepted_bytes": accepted,
                          "read_back_bytes": read_back, "would_block": blocked,
                          "zero_write": 0 in writes, "stop_reason": stop_reason,
                          "bounded_attempts": 32, "backpressure_acceptance_claimed": False}))
    finally:
        os.close(master)
        os.close(slave)


if __name__ == "__main__":
    main()
