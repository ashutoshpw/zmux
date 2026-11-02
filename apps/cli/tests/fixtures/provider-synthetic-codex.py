#!/usr/bin/env python3
"""Synthetic Codex CLI and read-only Unix WebSocket daemon for managed SSH."""

import base64
import hashlib
import json
import os
import select
import signal
import socket
import stat
import struct
import subprocess
import sys
import time

THREAD = "thread:fixture/root"
TURN = "turn:fixture/live"


def current_state():
    try:
        with open(os.environ["ZMUX_FAKE_CODEX_STATE"], encoding="utf-8") as source:
            return source.read().strip()
    except OSError:
        return "idle"


def send_frame(connection, opcode, payload):
    header = bytearray([0x80 | opcode])
    if len(payload) < 126:
        header.append(len(payload))
    elif len(payload) <= 0xFFFF:
        header.append(126)
        header.extend(struct.pack("!H", len(payload)))
    else:
        header.append(127)
        header.extend(struct.pack("!Q", len(payload)))
    connection.sendall(bytes(header) + payload)


def send_json(connection, value):
    send_frame(connection, 1, json.dumps(value, separators=(",", ":")).encode())


class WebSocket:
    def __init__(self, connection, initial=b""):
        self.connection = connection
        self.buffer = bytearray(initial)

    def read_exact(self, size):
        while len(self.buffer) < size:
            chunk = self.connection.recv(max(1, size - len(self.buffer)))
            if not chunk:
                raise EOFError
            self.buffer.extend(chunk)
        value = bytes(self.buffer[:size])
        del self.buffer[:size]
        return value

    def receive_json(self):
        while True:
            first, second = self.read_exact(2)
            opcode = first & 0x0F
            final = bool(first & 0x80)
            if first & 0x70 or not second & 0x80 or not final:
                raise ValueError("fixture received an unsupported WebSocket frame")
            length = second & 0x7F
            if length == 126:
                length = struct.unpack("!H", self.read_exact(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self.read_exact(8))[0]
            if length > 65536:
                raise ValueError("fixture received an oversized WebSocket frame")
            mask = self.read_exact(4)
            payload = self.read_exact(length)
            payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
            if opcode == 8:
                return None
            if opcode == 9:
                send_frame(self.connection, 10, payload)
                continue
            if opcode == 10:
                continue
            if opcode != 1:
                raise ValueError("fixture received a non-text WebSocket frame")
            return json.loads(payload.decode("utf-8"))


def accept_websocket(connection):
    connection.settimeout(2.0)
    request = bytearray()
    while b"\r\n\r\n" not in request:
        if len(request) > 8192:
            raise ValueError("fixture WebSocket request exceeded its bound")
        chunk = connection.recv(4096)
        if not chunk:
            raise EOFError
        request.extend(chunk)
    header, _, remainder = request.partition(b"\r\n\r\n")
    lines = header.decode("latin1").split("\r\n")
    headers = {}
    for line in lines[1:]:
        name, separator, value = line.partition(":")
        if separator:
            headers[name.strip().lower()] = value.strip()
    key = headers["sec-websocket-key"]
    accept = base64.b64encode(
        hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
    ).decode("ascii")
    connection.sendall(
        (
            "HTTP/1.1 101 Switching Protocols\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
        ).encode("ascii")
    )
    connection.settimeout(None)
    return WebSocket(connection, remainder)


def status_for(state):
    if state == "start":
        return {"type": "active", "activeFlags": []}
    return {"type": "idle"}


def answer(websocket, request):
    method = request.get("method")
    request_id = request.get("id")
    if request_id is None:
        return
    state = current_state()
    if method == "initialize":
        result = {"userAgent": "synthetic", "platformFamily": "unix", "platformOs": "linux"}
    elif method == "config/read":
        result = {
            "config": {
                "hooks": {
                    "SessionStart": [
                        {"hooks": [{"type": "command", "command": "fixture-user-session-start"}]}
                    ],
                    "Stop": [{"hooks": [{"type": "command", "command": "fixture-user-stop"}]}],
                }
            }
        }
    elif method == "thread/read":
        result = {
            "thread": {
                "id": THREAD,
                "status": status_for(state),
            }
        }
    elif method == "thread/turns/list":
        turns = [{"id": TURN, "status": "inProgress"}] if state == "start" else []
        result = {"data": turns}
    else:
        result = {}
    send_json(websocket.connection, {"id": request_id, "result": result})


def app_server():
    path = os.environ["ZMUX_FAKE_CODEX_SOCKET"]
    integration_root = os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", "")
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(path)
    os.chmod(path, 0o600)
    listener.listen(2)
    listener.settimeout(0.1)
    foreign_probe = os.environ.get("ZMUX_FAKE_CODEX_FOREIGN_PROBE")
    foreign_sent = False
    try:
        while os.path.isdir(integration_root):
            try:
                connection, _ = listener.accept()
            except socket.timeout:
                continue
            with connection:
                try:
                    websocket = accept_websocket(connection)
                    previous_state = current_state()
                    while os.path.isdir(integration_root):
                        if foreign_probe and not foreign_sent and os.path.exists(foreign_probe):
                            send_json(
                                connection,
                                {
                                    "method": "turn/started",
                                    "params": {
                                        "threadId": "thread:foreign/child",
                                        "turn": {
                                            "id": "turn:foreign/opaque",
                                            "status": "inProgress",
                                        },
                                    },
                                },
                            )
                            foreign_ack = os.environ.get("ZMUX_FAKE_CODEX_FOREIGN_ACK")
                            if foreign_ack:
                                with open(foreign_ack, "w", encoding="ascii") as output:
                                    output.write("sent")
                            foreign_sent = True
                        state = current_state()
                        if state != previous_state:
                            if state == "start":
                                send_json(
                                    connection,
                                    {
                                        "method": "turn/started",
                                        "params": {
                                            "threadId": THREAD,
                                            "turn": {"id": TURN, "status": "inProgress"},
                                        },
                                    },
                                )
                            elif state == "complete":
                                send_json(
                                    connection,
                                    {
                                        "method": "turn/completed",
                                        "params": {
                                            "threadId": THREAD,
                                            "turn": {"id": TURN, "status": "completed"},
                                        },
                                    },
                                )
                            previous_state = state
                        readable, _, _ = select.select([connection], [], [], 0.025)
                        if readable:
                            request = websocket.receive_json()
                            if request is None:
                                break
                            answer(websocket, request)
                except EOFError:
                    continue
    except (EOFError, OSError, ValueError, KeyError, json.JSONDecodeError):
        return
    finally:
        listener.close()
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass


def daemon_version():
    path = os.environ["ZMUX_FAKE_CODEX_SOCKET"]
    pid_path = os.environ["ZMUX_FAKE_CODEX_DAEMON_PID"]
    try:
        with open(pid_path, encoding="ascii") as source:
            pid = int(source.read())
        os.kill(pid, 0)
        if stat.S_ISSOCK(os.stat(path, follow_symlinks=False).st_mode):
            print(json.dumps({"status": "running", "socketPath": path}), flush=True)
            return
    except (OSError, ValueError):
        pass
    daemon = subprocess.Popen(
        [sys.executable, __file__, "app-server-daemon"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    with open(pid_path, "w", encoding="ascii") as output:
        output.write(str(daemon.pid))
    deadline = time.monotonic() + 2.0
    while not os.path.exists(path) and time.monotonic() < deadline:
        time.sleep(0.01)
    if not os.path.exists(path):
        try:
            os.kill(daemon.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        raise RuntimeError("synthetic daemon did not bind")
    print(json.dumps({"status": "running", "socketPath": path}), flush=True)


def stop_daemon():
    try:
        with open(os.environ["ZMUX_FAKE_CODEX_DAEMON_PID"], encoding="ascii") as source:
            pid = int(source.read())
        os.kill(pid, signal.SIGTERM)
    except (OSError, ValueError):
        pass


def invoke_session_hook(helper, payload):
    if os.environ.get("ZMUX_CLI_PATH"):
        command = [
            os.environ["ZMUX_CLI_PATH"],
            "hooks",
            "provider",
            "codex-session-start",
        ]
    else:
        command = [sys.executable, helper, "codex-session-start"]
    return subprocess.run(
        command,
        input=json.dumps(payload).encode(),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    ).returncode


def provider():
    args = sys.argv[1:]
    calls = os.environ.get("ZMUX_FAKE_CODEX_CALLS")
    if calls:
        with open(calls, "a", encoding="ascii") as output:
            output.write("launch\n")
    setting = args[args.index("--config") + 1] if "--config" in args else ""
    report = {
        "args": args,
        "existing_session_start_preserved": "fixture-user-session-start" in setting,
        "zmux_session_start_added": "hooks.SessionStart=" in setting
        and (
            "provider-launch.py" in setting
            or "hooks provider codex-session-start" in setting
        ),
        "stop_override_added": "hooks.Stop=" in setting,
    }
    with open(os.environ["ZMUX_FAKE_CODEX_REPORT"], "w", encoding="utf-8") as output:
        json.dump(report, output)
        output.flush()

    if any(value.split("=", 1)[0] in ("--profile", "-p", "--cd", "-C", "mcp") for value in args):
        print("CODEX_CONTEXT_PASSTHROUGH", flush=True)
        return 0

    helper = os.path.join(
        os.environ["ZMUX_SHELL_INTEGRATION_DIRECTORY"], "provider-launch.py"
    )
    if (
        invoke_session_hook(
            helper, {"hook_event_name": "SessionStart", "session_id": THREAD}
        )
        != 0
    ):
        stop_daemon()
        return 81
    print("CODEX_FIXTURE_READY", flush=True)

    stop_attempted = False
    try:
        while not os.path.exists(os.environ["ZMUX_FAKE_CODEX_RELEASE"]):
            if (
                not stop_attempted
                and os.path.exists(os.environ["ZMUX_FAKE_CODEX_STOP_PROBE"])
            ):
                result = invoke_session_hook(
                    helper,
                    {
                        "hook_event_name": "Stop",
                        "session_id": THREAD,
                        "decision": "block",
                        "permissionDecision": "approve",
                    },
                )
                with open(
                    os.environ["ZMUX_FAKE_CODEX_STOP_RESULT"], "w", encoding="utf-8"
                ) as output:
                    output.write(str(result))
                stop_attempted = True
            time.sleep(0.02)
    finally:
        if os.environ.get("ZMUX_FAKE_CODEX_KEEP_DAEMON") != "1":
            stop_daemon()
    print("CODEX_FIXTURE_DONE", flush=True)
    return 0


if __name__ == "__main__":
    if sys.argv[1:4] == ["app-server", "daemon", "version"]:
        daemon_version()
        raise SystemExit(0)
    if sys.argv[1:2] == ["app-server-daemon"]:
        app_server()
        raise SystemExit(0)
    if sys.argv[1:] in (["--help"], ["-h"]):
        print("CODEX_HELP_PASSTHROUGH")
        raise SystemExit(0)
    if sys.argv[1:] in (["--version"], ["-V"]):
        print("codex-cli 0.166.0")
        raise SystemExit(0)
    raise SystemExit(provider())
