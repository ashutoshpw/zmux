#!/usr/bin/env python3
"""Multiplex sanitized hook frames, bounded receipts, and SSH heartbeats."""

import fcntl
from collections import OrderedDict
import json
import math
import os
import re
import select
import socket
import stat
import sys
import time
import uuid

VERSION = 3
MAX_LINE_BYTES = 4096
HEARTBEAT_SECONDS = 5.0
FIFO_WAIT_SECONDS = 20.0
RECEIPT_WAIT_SECONDS = 1.8
MAX_PENDING_RECEIPTS = 64
MAX_SEEN_EVENT_IDS = 256


def send(frame):
    encoded = json.dumps(frame, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    if len(encoded) + 1 > MAX_LINE_BYTES:
        raise ValueError("lifecycle frame exceeded its byte bound")
    sys.stdout.buffer.write(encoded + b"\n")
    sys.stdout.buffer.flush()


def uuid_hex(value):
    try:
        parsed = uuid.UUID(value)
    except (ValueError, TypeError, AttributeError):
        return None
    if parsed.int == 0:
        return None
    return parsed.hex


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
        or uuid_hex(frame.get("nonce")) != nonce
    ):
        return None
    generation = uuid_hex(frame.get("generation"))
    if generation is None:
        return None
    path = os.path.join(root, "agent-lifecycle-active.lock")
    descriptor = None
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
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError:
                pass
        return None
    send(
        {
            "kind": "active",
            "version": VERSION,
            "nonce": nonce,
            "generation": str(uuid.UUID(hex=generation)),
        }
    )
    return descriptor, generation


def receipt_path(root, event_id):
    event_hex = uuid_hex(event_id)
    if event_hex is None:
        return None
    return os.path.join(root, "r-" + event_hex)


def send_hook_receipt(root, path, event_id, nonce, generation, disposition):
    metadata = os.stat(path, follow_symlinks=False)
    if (
        not stat.S_ISSOCK(metadata.st_mode)
        or metadata.st_uid != os.geteuid()
        or stat.S_IMODE(metadata.st_mode) != 0o600
        or metadata.st_nlink != 1
    ):
        raise OSError("unsafe per-hook receipt endpoint")
    payload = json.dumps(
        {
            "kind": "receipt",
            "nonce": nonce,
            "generation": str(uuid.UUID(hex=generation)),
            "event_id": event_id,
            "disposition": disposition,
        },
        separators=(",", ":"),
    ).encode("ascii")
    if len(payload) > 512:
        raise OSError("receipt exceeded its datagram bound")
    endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    try:
        endpoint.sendto(payload, path)
    finally:
        endpoint.close()


def receipt_endpoint_available(path):
    if path is None:
        return False
    try:
        metadata = os.stat(path, follow_symlinks=False)
    except OSError:
        return False
    return (
        stat.S_ISSOCK(metadata.st_mode)
        and metadata.st_uid == os.geteuid()
        and stat.S_IMODE(metadata.st_mode) == 0o600
        and metadata.st_nlink == 1
    )


def valid_receipt(frame, nonce, generation, pending, root):
    if not isinstance(frame, dict) or set(frame) != {
        "kind",
        "version",
        "nonce",
        "generation",
        "event_id",
        "disposition",
    }:
        return
    event_id = frame.get("event_id")
    event_hex = uuid_hex(event_id)
    if (
        frame.get("kind") != "receipt"
        or frame.get("version") != VERSION
        or uuid_hex(frame.get("nonce")) != nonce
        or uuid_hex(frame.get("generation")) != generation
        or frame.get("disposition")
        not in ("alert_covered", "handled_no_alert", "not_handled")
        or event_hex is None
    ):
        return
    entry = pending.get(event_hex)
    if entry is None:
        return
    path, deadline = entry
    if time.monotonic() >= deadline:
        pending.pop(event_hex, None)
        return
    try:
        send_hook_receipt(
            root,
            path,
            event_id,
            str(uuid.UUID(hex=nonce)),
            generation,
            frame["disposition"],
        )
    except OSError:
        pass
    pending.pop(event_hex, None)


def process_input_bytes(data, nonce, generation, pending, root, input_pending):
    if not data:
        return
    input_pending.extend(data)
    input_bytes = input_pending
    if len(input_bytes) > MAX_LINE_BYTES and b"\n" not in input_bytes:
        input_bytes.clear()
        return
    while b"\n" in input_bytes:
        line, _, remainder = input_bytes.partition(b"\n")
        input_bytes[:] = remainder
        if not line or len(line) > MAX_LINE_BYTES:
            continue
        try:
            frame = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        valid_receipt(frame, nonce, generation, pending, root)


def cleanup_pending(pending):
    now = time.monotonic()
    for event_hex, (_, deadline) in list(pending.items()):
        if deadline <= now:
            pending.pop(event_hex, None)


def remember_event_id(seen, event_hex):
    if event_hex in seen:
        return False
    seen[event_hex] = None
    if len(seen) > MAX_SEEN_EVENT_IDS:
        seen.popitem(last=False)
    return True


def main():
    nonce_argument = sys.argv[1] if len(sys.argv) == 2 else ""
    nonce = uuid_hex(nonce_argument)
    root = os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", "")
    if nonce is None or root != f"/tmp/zmux-shell-{nonce}":
        return 70
    try:
        root_metadata = os.stat(root, follow_symlinks=False)
    except OSError:
        return 70
    if (
        not stat.S_ISDIR(root_metadata.st_mode)
        or root_metadata.st_uid != os.geteuid()
        or stat.S_IMODE(root_metadata.st_mode) != 0o700
    ):
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
        reader = os.open(
            fifo, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC
        )
        writer_hold = os.open(
            fifo, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC
        )
        loss_reader = os.open(
            loss_fifo, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC
        )
        loss_writer_hold = os.open(
            loss_fifo, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC
        )
        for descriptor in (reader, writer_hold, loss_reader, loss_writer_hold):
            metadata = os.fstat(descriptor)
            if (
                not stat.S_ISFIFO(metadata.st_mode)
                or metadata.st_uid != os.geteuid()
                or stat.S_IMODE(metadata.st_mode) != 0o600
                or metadata.st_nlink != 1
            ):
                raise OSError("unsafe lifecycle FIFO")
    except OSError:
        return 72

    send({"kind": "hello", "version": VERSION, "nonce": nonce})
    activation = activate(nonce, root)
    if activation is None:
        os.close(reader)
        os.close(writer_hold)
        os.close(loss_reader)
        os.close(loss_writer_hold)
        return 74
    active_descriptor, generation = activation
    pending = {}
    seen_event_ids = OrderedDict()
    input_pending = bytearray()
    fifo_pending = bytearray()
    sequence = 0
    next_heartbeat = time.monotonic() + HEARTBEAT_SECONDS
    try:
        while os.path.exists(fifo):
            cleanup_pending(pending)
            timeout = max(0.0, min(0.1, next_heartbeat - time.monotonic()))
            readable, _, _ = select.select(
                [reader, loss_reader, sys.stdin.buffer], [], [], timeout
            )
            if sys.stdin.buffer in readable:
                incoming = os.read(sys.stdin.fileno(), MAX_LINE_BYTES)
                if not incoming:
                    return 0
                process_input_bytes(
                    incoming, nonce, generation, pending, root, input_pending
                )
            if loss_reader in readable and os.read(loss_reader, 4096):
                sequence += 1
                send(
                    {
                        "kind": "gap",
                        "version": VERSION,
                        "nonce": nonce,
                        "generation": str(uuid.UUID(hex=generation)),
                        "sequence": sequence,
                    }
                )
                return 74
            if reader in readable:
                chunk = os.read(reader, 4096)
                if chunk:
                    fifo_pending.extend(chunk)
                    if len(fifo_pending) > MAX_LINE_BYTES and b"\n" not in fifo_pending:
                        return 73
                    while b"\n" in fifo_pending:
                        line, _, remainder = fifo_pending.partition(b"\n")
                        fifo_pending[:] = remainder
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
                        event_id = supplied.get("event_id")
                        event_hex = uuid_hex(event_id)
                        if event_hex is None or not remember_event_id(
                            seen_event_ids, event_hex
                        ):
                            continue
                        required = supplied.get("receipt_required") is True
                        requested = supplied.get("receipt_requested") is True
                        if requested and not required:
                            continue
                        receipt_deadline = supplied.get("receipt_deadline")
                        path = receipt_path(root, event_id) if requested else None
                        endpoint_available = receipt_endpoint_available(path)
                        receipt_live = (
                            required
                            and requested
                            and event_hex is not None
                            and path is not None
                            and endpoint_available
                            and isinstance(receipt_deadline, (int, float))
                            and math.isfinite(receipt_deadline)
                            and time.monotonic() < receipt_deadline <= time.monotonic() + 3.0
                            and event_hex not in pending
                            and len(pending) < MAX_PENDING_RECEIPTS
                        )
                        if required:
                            if not receipt_live:
                                # The invocation may still emit a silent visual
                                # fallback, but the owner must not keep stale
                                # activity after an alert-bearing event was
                                # dropped. Fence this generation so the desktop
                                # publishes Unknown before the evidence ages out.
                                sequence += 1
                                send(
                                    {
                                        "kind": "gap",
                                        "version": VERSION,
                                        "nonce": nonce,
                                        "generation": str(uuid.UUID(hex=generation)),
                                        "sequence": sequence,
                                    }
                                )
                                return 74
                            pending[event_hex] = (path, receipt_deadline)
                        sequence += 1
                        send(
                            {
                                "kind": "hook",
                                "version": VERSION,
                                "nonce": nonce,
                                "generation": str(uuid.UUID(hex=generation)),
                                "sequence": sequence,
                                "event_id": event_id,
                                "receipt_requested": receipt_live,
                                "event": supplied["event"],
                            }
                        )
                    if len(fifo_pending) > MAX_LINE_BYTES and b"\n" not in fifo_pending:
                        return 73
            now = time.monotonic()
            if now >= next_heartbeat:
                sequence += 1
                send(
                    {
                        "kind": "heartbeat",
                        "version": VERSION,
                        "nonce": nonce,
                        "generation": str(uuid.UUID(hex=generation)),
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
