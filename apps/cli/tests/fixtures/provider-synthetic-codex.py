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
MAX_WEBSOCKET_FRAME_BYTES = 65_536
MAX_WEBSOCKET_BUFFER_BYTES = (2 * MAX_WEBSOCKET_FRAME_BYTES) + 14
WEBSOCKET_FRAME_DEADLINE_SECONDS = 2.0
WEBSOCKET_HANDSHAKE_DEADLINE_SECONDS = 2.0
MAX_WEBSOCKET_FRAMES_PER_TICK = 16
WEBSOCKET_CLOSED = object()


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


class WebSocketProtocolError(ValueError):
    pass


class WebSocketFrameDeadlineExceeded(TimeoutError):
    pass


class FixtureError(Exception):
    pass


class WebSocket:
    def __init__(self, connection, initial=b""):
        self.connection = connection
        self.buffer = bytearray(initial)
        self.frame_deadline = (
            time.monotonic() + WEBSOCKET_FRAME_DEADLINE_SECONDS if initial else None
        )

    def _check_frame_deadline(self):
        if self.buffer and self.frame_deadline is None:
            self.frame_deadline = time.monotonic() + WEBSOCKET_FRAME_DEADLINE_SECONDS
        if self.frame_deadline is not None and time.monotonic() >= self.frame_deadline:
            raise WebSocketFrameDeadlineExceeded

    def _frame_header(self):
        self._check_frame_deadline()
        if len(self.buffer) < 2:
            return None
        first, second = self.buffer[:2]
        opcode = first & 0x0F
        final = bool(first & 0x80)
        masked = bool(second & 0x80)
        if first & 0x70 or not masked or not final:
            raise WebSocketProtocolError("fixture received an unsupported WebSocket frame")
        if opcode not in (1, 8, 9, 10):
            raise WebSocketProtocolError("fixture received an unsupported WebSocket opcode")

        length = second & 0x7F
        header_size = 2
        if length == 126:
            if len(self.buffer) < header_size + 2:
                return None
            length = struct.unpack("!H", self.buffer[header_size : header_size + 2])[0]
            header_size += 2
        elif length == 127:
            if len(self.buffer) < header_size + 8:
                return None
            length = struct.unpack("!Q", self.buffer[header_size : header_size + 8])[0]
            header_size += 8
        if length > MAX_WEBSOCKET_FRAME_BYTES:
            raise WebSocketProtocolError("fixture received an oversized WebSocket frame")
        if opcode in (8, 9, 10) and length > 125:
            raise WebSocketProtocolError("fixture received an oversized control frame")
        if len(self.buffer) < header_size + 4:
            return None
        return opcode, length, header_size

    def has_complete_frame(self):
        header = self._frame_header()
        if header is None:
            return False
        _, length, header_size = header
        return len(self.buffer) >= header_size + 4 + length

    def read_available(self):
        available = MAX_WEBSOCKET_BUFFER_BYTES - len(self.buffer)
        if available <= 0:
            raise WebSocketProtocolError("fixture WebSocket receive buffer exceeded its bound")
        # The caller has polled readability; read only currently available bytes,
        # never wait for the rest of a frame.
        chunk = self.connection.recv(min(65_536, available))
        if not chunk:
            raise EOFError
        was_empty = not self.buffer
        self.buffer.extend(chunk)
        if was_empty:
            self.frame_deadline = time.monotonic() + WEBSOCKET_FRAME_DEADLINE_SECONDS

    def receive_json(self):
        header = self._frame_header()
        if header is None:
            return False, None
        opcode, length, header_size = header
        frame_size = header_size + 4 + length
        if len(self.buffer) < frame_size:
            return False, None
        mask = self.buffer[header_size : header_size + 4]
        payload = self.buffer[header_size + 4 : frame_size]
        payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        del self.buffer[:frame_size]
        self.frame_deadline = (
            time.monotonic() + WEBSOCKET_FRAME_DEADLINE_SECONDS if self.buffer else None
        )

        if opcode == 8:
            return True, WEBSOCKET_CLOSED
        if opcode == 9:
            send_frame(self.connection, 10, payload)
            return True, None
        if opcode == 10:
            return True, None
        if opcode != 1:
            raise WebSocketProtocolError("fixture received a non-text WebSocket frame")
        request = json.loads(payload.decode("utf-8"))
        if not isinstance(request, dict):
            raise WebSocketProtocolError("fixture received a non-object JSON request")
        return True, request


def accept_websocket(connection):
    connection.setblocking(True)
    request = bytearray()
    deadline = time.monotonic() + WEBSOCKET_HANDSHAKE_DEADLINE_SECONDS
    while b"\r\n\r\n" not in request:
        if len(request) > 8192:
            raise WebSocketProtocolError("fixture WebSocket request exceeded its bound")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("fixture WebSocket handshake deadline exceeded")
        readable, _, _ = select.select([connection], [], [], remaining)
        if not readable:
            raise TimeoutError("fixture WebSocket handshake deadline exceeded")
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
    return WebSocket(connection, remainder)


class FixtureDiagnostics:
    PHASES = {
        "starting",
        "listening",
        "connected",
        "probe_seen",
        "notification_sent",
        "notification_acked",
        "client_closed",
    }
    ERRORS = {
        "none",
        "peer_closed",
        "handshake_timeout",
        "websocket_protocol_error",
        "invalid_json",
        "frame_timeout",
        "socket_error",
        "fixture_error",
    }

    def __init__(self):
        self.path = os.environ.get("ZMUX_FAKE_CODEX_DIAGNOSTICS")
        self.phase_value = "starting"
        self.error_value = "none"
        self._write()

    def _write(self):
        if not self.path:
            return
        content = f"phase={self.phase_value}\nerror={self.error_value}\n"
        temporary = self.path + ".tmp"
        try:
            with open(temporary, "w", encoding="ascii") as output:
                output.write(content)
            os.chmod(temporary, 0o600)
            os.replace(temporary, self.path)
        except OSError:
            try:
                os.unlink(temporary)
            except OSError:
                pass

    def phase(self, value):
        self.phase_value = value if value in self.PHASES else "connected"
        self._write()

    def error(self, exception):
        if isinstance(exception, WebSocketFrameDeadlineExceeded):
            value = "frame_timeout"
        elif isinstance(exception, TimeoutError):
            value = "handshake_timeout"
        elif isinstance(exception, EOFError):
            value = "peer_closed"
        elif isinstance(exception, json.JSONDecodeError):
            value = "invalid_json"
        elif isinstance(exception, FixtureError):
            value = "fixture_error"
        elif isinstance(exception, OSError):
            value = "socket_error"
        elif isinstance(exception, (ValueError, KeyError, struct.error)):
            value = "websocket_protocol_error"
        else:
            value = "fixture_error"
        self.error_value = value if value in self.ERRORS else "fixture_error"
        self._write()


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
    diagnostics = FixtureDiagnostics()
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(path)
    os.chmod(path, 0o600)
    listener.listen(2)
    listener.settimeout(0.1)
    diagnostics.phase("listening")
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
                    diagnostics.phase("connected")
                    previous_state = current_state()
                    while os.path.isdir(integration_root):
                        closed = False
                        for _ in range(MAX_WEBSOCKET_FRAMES_PER_TICK):
                            complete, request = websocket.receive_json()
                            if not complete:
                                break
                            if request is WEBSOCKET_CLOSED:
                                diagnostics.phase("client_closed")
                                closed = True
                                break
                            if request is not None:
                                answer(websocket, request)
                        if closed:
                            break

                        if foreign_probe and not foreign_sent and os.path.exists(foreign_probe):
                            diagnostics.phase("probe_seen")
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
                            diagnostics.phase("notification_sent")
                            foreign_ack = os.environ.get("ZMUX_FAKE_CODEX_FOREIGN_ACK")
                            if foreign_ack:
                                try:
                                    with open(foreign_ack, "w", encoding="ascii") as output:
                                        output.write("sent")
                                except OSError as error:
                                    raise FixtureError from error
                            diagnostics.phase("notification_acked")
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

                        if websocket.has_complete_frame():
                            continue
                        readable, _, _ = select.select([connection], [], [], 0.025)
                        if readable:
                            websocket.read_available()
                except Exception as error:
                    diagnostics.error(error)
    finally:
        listener.close()
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass


def daemon_version(start=False):
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
    if not start:
        print(json.dumps({"status": "stopped"}), flush=True)
        return
    daemon = subprocess.Popen(
        [sys.executable, __file__, "app-server-daemon"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    with open(pid_path, "w", encoding="ascii") as output:
        output.write(str(daemon.pid))
    deadline = time.monotonic() + 10.0
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
    if sys.argv[1:] == ["fixture-daemon-start"]:
        daemon_version(start=True)
        raise SystemExit(0)
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
