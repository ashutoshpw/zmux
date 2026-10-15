#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Original raw-PTY protocol peer; independently checks literal reply bytes."""
import fcntl
import os
import signal
import struct
import sys
import termios
import threading

PRE = b"PRE:"
POST = b":POST\x00"
CURSOR = b"\x1b[1;1R"
STATUS = b"\x1b[0n"
N = 32768


def report(byte):
    assert os.write(3, byte) == len(byte)


def exact(expected):
    offset = 0
    while offset < len(expected):
        data = os.read(0, min(4096, len(expected) - offset))
        assert data and data == expected[offset:offset + len(data)], "child exact input order differs"
        offset += len(data)


def write_all(data):
    while data:
        n = os.write(1, data)
        assert n > 0
        data = data[n:]


def control(expected):
    assert os.read(4, 1) == expected


def size(rows, cols):
    actual = struct.unpack("HHHH", fcntl.ioctl(0, termios.TIOCGWINSZ, bytes(8)))
    assert actual[:2] == (rows, cols), "child kernel geometry differs"


def main():
    mode = sys.argv[1]
    assert os.isatty(0) and os.isatty(1)
    attributes = termios.tcgetattr(0)
    assert not attributes[3] & (termios.ECHO | termios.ICANON)
    size(3, 8)
    if mode == "cancel":
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    report(b"R")
    if mode == "basic":
        exact(PRE)
        write_all(b"\x1b]0;pty;title\x07\x1b]7;file://host/pty;path\x07\x07\x1b[6n")
        exact(CURSOR)
        report(b"A")
        control(b"Z")
        size(4, 12)
        write_all(b"\x1b]777;notify;title;body;tail\x07\x1b[5n\x1b[6n")
        exact(STATUS + CURSOR + POST)
        report(b"V")
    elif mode in ("flood", "stall"):
        exact(PRE)
        def writer():
            write_all(b"\x1b[6n" * N)
            report(b"W")
        thread = threading.Thread(target=writer)
        thread.start()
        control(b"G")
        exact(CURSOR * N + POST)
        thread.join(timeout=1)
        assert not thread.is_alive()
        size(4, 12)
        report(b"V")
    elif mode in ("cancel", "close", "resize-error", "engine-error"):
        write_all(b"\x1b]2;last-good\x07")
        control(b"B")
        if mode == "engine-error":
            write_all(b"\x1b]2;candidate\x07\x07\x1b[6n\x1b[2J\x1b[Ha" + "\U0001d185".encode() * 64)
            control(b"X")
        elif mode == "resize-error":
            control(b"X")
        else:
            write_all(b"\x1b]2;candidate\x07\x07\x1b[6n")
            exact(CURSOR[:3])
            report(b"C")  # Actual child consumption, distinct from parent write success.
            if mode == "cancel":
                control(b"X")
    else:
        raise AssertionError("unknown fixture mode")


if __name__ == "__main__":
    try:
        main()
    except BaseException as error:
        report(b"O" if "child exact input order differs" in str(error) else b"E")
        raise
