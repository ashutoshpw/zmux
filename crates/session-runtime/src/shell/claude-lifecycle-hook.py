#!/usr/bin/env python3
"""Sanitize one Claude hook input and relay it over the session FIFO."""

import json
import os
import re
import select
import socket
import stat
import sys
import time
import uuid

MAX_INPUT_BYTES = 65_536
MAX_FRAME_BYTES = 2_048
RECEIPT_WAIT_SECONDS = 1.8
LOSS_WARNING = b"zmux: Claude lifecycle evidence was dropped; state will become unknown\n"
EVENTS = {
    "SessionStart",
    "UserPromptSubmit",
    "PermissionRequest",
    "Elicitation",
    "ElicitationResult",
    "Notification",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "SessionEnd",
}
ATTENTION_NOTIFICATION_TYPES = {
    "permission_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
}


def optional_token(value):
    if not isinstance(value, str) or len(value) > 64:
        return None
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", value):
        return None
    return value


def optional_uuid(value):
    if not isinstance(value, str):
        return None
    try:
        return str(uuid.UUID(value))
    except (ValueError, AttributeError):
        return None


def uuid_hex(value):
    try:
        parsed = uuid.UUID(value)
    except (ValueError, TypeError, AttributeError):
        return None
    if parsed.int == 0:
        return None
    return parsed.hex


def report_lost_evidence(root):
    fifo = os.path.join(root, "agent-events-loss")
    try:
        descriptor = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
        try:
            if os.write(descriptor, b"!") != 1:
                raise OSError("short lifecycle loss signal")
        finally:
            os.close(descriptor)
        return
    except (OSError, ValueError):
        try:
            _, writable, _ = select.select([], [2], [], 0)
            if writable:
                os.write(2, LOSS_WARNING)
        except (OSError, ValueError):
            pass


def lifecycle_root(nonce, root):
    if re.fullmatch(r"[0-9a-f]{32}", nonce) and root == f"/tmp/zmux-shell-{nonce}":
        try:
            metadata = os.stat(root, follow_symlinks=False)
        except OSError:
            return None
        if (
            stat.S_ISDIR(metadata.st_mode)
            and metadata.st_uid == os.geteuid()
            and stat.S_IMODE(metadata.st_mode) == 0o700
        ):
            return root
    return None


def alert_fallback_kind(event):
    if event.get("name") == "Stop":
        return "Stop"
    if (
        event.get("name") == "Notification"
        and event.get("notification_type")
        in ("permission_prompt", "elicitation_dialog", "elicitation_url_dialog")
    ):
        return "Notification"
    return None


def write_silent_fallback(root, event):
    fallback = {
        "Stop": "stop_silent",
        "Notification": "attention_silent",
    }.get(alert_fallback_kind(event))
    if fallback is None:
        return
    fifo = os.path.join(root, "attention")
    try:
        descriptor = os.open(
            fifo,
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
            value = (fallback + "\n").encode("ascii")
            if os.write(descriptor, value) != len(value):
                raise OSError("short attention FIFO write")
        finally:
            os.close(descriptor)
    except (OSError, ValueError):
        pass


def write_configured_silent_fallback(root, hook_name, matcher):
    if hook_name == "Stop":
        write_silent_fallback(root, {"name": "Stop"})
    elif hook_name == "Notification" and matcher in ATTENTION_NOTIFICATION_TYPES:
        write_silent_fallback(
            root,
            {"name": "Notification", "notification_type": matcher},
        )


def receipt_endpoint(root, event_id):
    path = os.path.join(root, "r-" + event_id.replace("-", ""))
    endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    previous_mask = os.umask(0o077)
    bound = False
    identity = None
    try:
        endpoint.bind(path)
        bound = True
        metadata = os.stat(path, follow_symlinks=False)
        if (
            not stat.S_ISSOCK(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or metadata.st_nlink != 1
        ):
            raise OSError("unsafe per-hook receipt endpoint")
        identity = (metadata.st_dev, metadata.st_ino)
        os.chmod(path, 0o600)
        metadata = os.stat(path, follow_symlinks=False)
        if (
            not stat.S_ISSOCK(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            raise OSError("unsafe per-hook receipt endpoint")
        return endpoint, path
    except OSError:
        endpoint.close()
        if bound and identity is not None:
            try:
                metadata = os.stat(path, follow_symlinks=False)
                if (
                    stat.S_ISSOCK(metadata.st_mode)
                    and metadata.st_uid == os.geteuid()
                    and (metadata.st_dev, metadata.st_ino) == identity
                ):
                    os.unlink(path)
            except OSError:
                pass
        raise
    finally:
        os.umask(previous_mask)


def receive_receipt(endpoint, event_id, nonce, deadline):
    if endpoint is None:
        return False
    remaining = max(0.0, deadline - time.monotonic())
    endpoint.settimeout(remaining)
    while remaining > 0:
        try:
            raw = endpoint.recv(512)
        except (OSError, socket.timeout):
            return False
        try:
            value = json.loads(raw)
            if not isinstance(value, dict):
                raise ValueError("receipt must be an object")
            if set(value) != {"kind", "nonce", "generation", "event_id", "disposition"}:
                raise ValueError("receipt has unknown or missing fields")
            receipt_nonce = uuid_hex(value.get("nonce"))
            receipt_generation = uuid_hex(value.get("generation"))
        except (ValueError, TypeError, AttributeError, UnicodeDecodeError):
            remaining = max(0.0, deadline - time.monotonic())
            endpoint.settimeout(remaining)
            continue
        if (
            value.get("kind") != "receipt"
            or value.get("event_id") != event_id
            or receipt_nonce != uuid_hex(nonce)
            or receipt_generation is None
            or value.get("disposition")
            not in ("alert_covered", "handled_no_alert", "not_handled")
        ):
            remaining = max(0.0, deadline - time.monotonic())
            endpoint.settimeout(remaining)
            continue
        return value["disposition"]
    return False


def cleanup_endpoint(endpoint, path):
    if endpoint is not None:
        endpoint.close()
    if path is not None:
        try:
            os.unlink(path)
        except OSError:
            pass


def main():
    hook_name = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] in EVENTS else None
    matcher = None
    if len(sys.argv) == 3:
        if hook_name != "Notification" or sys.argv[2] not in ATTENTION_NOTIFICATION_TYPES:
            return
        matcher = sys.argv[2]

    nonce = os.environ.get("ZMUX_AGENT_LIFECYCLE_NONCE", "")
    root = lifecycle_root(
        nonce,
        os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", ""),
    )
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        if root:
            write_configured_silent_fallback(root, hook_name, matcher)
            report_lost_evidence(root)
        return
    try:
        payload = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        if root:
            write_configured_silent_fallback(root, hook_name, matcher)
            report_lost_evidence(root)
        return
    if not isinstance(payload, dict):
        if root:
            write_configured_silent_fallback(root, hook_name, matcher)
            report_lost_evidence(root)
        return
    name = payload.get("hook_event_name")
    if matcher is not None:
        # The generic Notification command handles every parseable payload.
        # Matcher-specific invocations exist only to supply a trusted fallback
        # kind when that payload cannot be decoded as a hook event.
        if name == "Notification":
            return
        if isinstance(name, str):
            return
        write_configured_silent_fallback(root, hook_name, matcher)
        report_lost_evidence(root)
        return
    if hook_name is not None and name is not None and name != hook_name:
        return
    if name not in EVENTS:
        if (name is None or not isinstance(name, str)) and root:
            write_configured_silent_fallback(root, hook_name, matcher)
            report_lost_evidence(root)
        return
    if not root:
        return
    session_id = optional_uuid(payload.get("session_id"))
    if session_id is None:
        fallback_event = {"name": name}
        notification_type = optional_token(payload.get("notification_type"))
        if notification_type is not None:
            fallback_event["notification_type"] = notification_type
        write_silent_fallback(root, fallback_event)
        report_lost_evidence(root)
        return

    event = {
        "name": name,
        "session_id": session_id,
        "main_session": not bool(payload.get("agent_id") or payload.get("agent_type")),
    }
    for key in ("source", "permission_mode", "notification_type", "tool_name", "reason"):
        value = optional_token(payload.get(key))
        if value is not None:
            event[key] = value
    prompt_id = optional_uuid(payload.get("prompt_id"))
    if prompt_id is not None:
        event["prompt_id"] = prompt_id
    if name == "Stop":
        stop_hook_active = payload.get("stop_hook_active")
        if isinstance(stop_hook_active, bool):
            event["stop_hook_active"] = stop_hook_active
        background_tasks = payload.get("background_tasks")
        event["has_background_tasks"] = (
            isinstance(background_tasks, list) and len(background_tasks) > 0
        )

    event_id = str(uuid.uuid4())
    fallback = alert_fallback_kind(event)
    receipt_deadline = time.monotonic() + RECEIPT_WAIT_SECONDS if fallback else None
    endpoint = None
    endpoint_path = None
    if fallback is not None:
        try:
            endpoint, endpoint_path = receipt_endpoint(root, event_id)
        except OSError:
            endpoint = None

    frame_value = {
        "nonce": nonce,
        "event_id": event_id,
        # The stream must fence every alert-bearing event. Keep endpoint
        # availability separate so a failed bind cannot make a stale socket
        # look like this invocation's receipt route.
        "receipt_required": fallback is not None,
        "receipt_requested": fallback is not None and endpoint is not None,
        "event": event,
    }
    if receipt_deadline is not None:
        frame_value["receipt_deadline"] = receipt_deadline
    frame = json.dumps(frame_value, separators=(",", ":"), ensure_ascii=True).encode("ascii") + b"\n"
    if len(frame) > MAX_FRAME_BYTES:
        write_silent_fallback(root, event)
        report_lost_evidence(root)
        cleanup_endpoint(endpoint, endpoint_path)
        return

    fifo = os.path.join(root, "agent-events")
    lost = False
    try:
        descriptor = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            metadata = os.fstat(descriptor)
            if (
                not stat.S_ISFIFO(metadata.st_mode)
                or metadata.st_uid != os.geteuid()
                or stat.S_IMODE(metadata.st_mode) != 0o600
                or metadata.st_nlink != 1
            ):
                raise OSError("unsafe lifecycle FIFO")
            lost = os.write(descriptor, frame) != len(frame)
        except OSError:
            lost = True
        finally:
            os.close(descriptor)
    except (OSError, ValueError):
        lost = True
    if lost:
        if fallback is not None:
            write_silent_fallback(root, event)
        report_lost_evidence(root)
    elif fallback is not None:
        disposition = receive_receipt(endpoint, event_id, nonce, receipt_deadline)
        if disposition not in ("alert_covered", "handled_no_alert"):
            write_silent_fallback(root, event)
    cleanup_endpoint(endpoint, endpoint_path)


if __name__ == "__main__":
    main()
