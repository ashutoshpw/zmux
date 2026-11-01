#!/usr/bin/env python3
"""Sanitize one Claude hook input and relay it over the session FIFO."""

import json
import os
import re
import select
import sys
import uuid

MAX_INPUT_BYTES = 65_536
MAX_FRAME_BYTES = 512
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
        # The hook stays nonblocking even when both bounded FIFOs are unavailable.
        try:
            _, writable, _ = select.select([], [2], [], 0)
            if writable:
                os.write(2, LOSS_WARNING)
        except (OSError, ValueError):
            pass


def lifecycle_root(nonce, root):
    if (
        re.fullmatch(r"[0-9a-f]{32}", nonce)
        and root == f"/tmp/zmux-shell-{nonce}"
    ):
        return root
    return None


def main():
    nonce = os.environ.get("ZMUX_AGENT_LIFECYCLE_NONCE", "")
    root = lifecycle_root(
        nonce,
        os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", ""),
    )
    raw = sys.stdin.buffer.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        if root:
            report_lost_evidence(root)
        return
    try:
        payload = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        if root:
            report_lost_evidence(root)
        return
    if not isinstance(payload, dict):
        if root:
            report_lost_evidence(root)
        return
    name = payload.get("hook_event_name")
    session_id = optional_uuid(payload.get("session_id"))
    if name not in EVENTS:
        return
    if not root or session_id is None:
        if root:
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

    frame = json.dumps(
        {"nonce": nonce, "event": event}, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii") + b"\n"
    if len(frame) > MAX_FRAME_BYTES:
        report_lost_evidence(root)
        return
    fifo = os.path.join(root, "agent-events")
    lost = False
    try:
        descriptor = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
        try:
            lost = os.write(descriptor, frame) != len(frame)
        except OSError:
            lost = True
        finally:
            os.close(descriptor)
    except (OSError, ValueError):
        lost = True
    if lost:
        # This side channel never upgrades the provider claim. It forces the
        # SSH adapter to fence its current generation before more hooks apply.
        report_lost_evidence(root)


if __name__ == "__main__":
    main()
