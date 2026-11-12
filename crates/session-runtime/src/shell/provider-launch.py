#!/usr/bin/env python3
"""Session-scoped Codex/OpenCode launch and provider lifecycle producer."""

import base64
import hashlib
import hmac
import json
import os
import queue
import select
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from collections import deque

MAX_EVENT_BYTES = 4096
MAX_RPC_BYTES = 256 * 1024
MAX_DAEMON_STATUS_BYTES = 4096
MAX_WEBSOCKET_HEADER_BYTES = 8192
MAX_SOCKET_PATH_BYTES = 100
MAX_PENDING_NOTIFICATIONS = 128
RPC_TIMEOUT = 2.0
SNAPSHOT_SECONDS = 5.0
HOOK_WAIT_WARNING = 4.0
OPAQUE_ID_MAX = 256
LIFECYCLE_VERSION = 4
PROVIDER_ROUTE_RECEIPT_TIMEOUT = 2.0
MAX_PROVIDER_ROUTE_RECEIPT_BYTES = 512


def valid_id(value):
    return (
        isinstance(value, str)
        and bool(value)
        and len(value.encode("utf-8")) <= OPAQUE_ID_MAX
        and not any(ord(character) < 32 or ord(character) == 127 for character in value)
    )


def root_context():
    nonce = os.environ.get("ZMUX_AGENT_LIFECYCLE_NONCE", "")
    root = os.environ.get("ZMUX_SHELL_INTEGRATION_DIRECTORY", "")
    if len(nonce) != 32 or any(character not in "0123456789abcdef" for character in nonce):
        raise ValueError("managed lifecycle token is unavailable")
    if root != f"/tmp/zmux-shell-{nonce}":
        raise ValueError("managed shell integration directory does not match this session")
    metadata = os.stat(root, follow_symlinks=False)
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.geteuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise ValueError("managed shell integration directory is unsafe")
    return nonce, root


def provider_route_receipt_path(root, route_id):
    try:
        parsed = uuid.UUID(route_id)
    except (ValueError, TypeError, AttributeError):
        raise ValueError("provider route ID is invalid")
    if parsed.int == 0:
        raise ValueError("provider route ID is nil")
    return os.path.join(root, "p-" + parsed.hex)


def write_lifecycle_fifo(root, value):
    path = os.path.join(root, "agent-events")
    descriptor = os.open(path, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISFIFO(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
            or len(value) > os.fpathconf(descriptor, "PC_PIPE_BUF")
            or os.write(descriptor, value) != len(value)
        ):
            raise OSError("provider lifecycle FIFO write failed")
    finally:
        os.close(descriptor)


def emit_provider_route(root, provider, route_id, provider_generation, phase, expected_generation=None):
    if provider not in ("codex", "opencode") or phase not in ("starting", "ended", "cancelled"):
        raise ValueError("provider route event is invalid")
    route_hex = uuid.UUID(route_id).hex
    provider_generation_hex = uuid.UUID(provider_generation).hex
    if not route_hex.strip("0") or not provider_generation_hex.strip("0"):
        raise ValueError("provider route identity is nil")
    nonce, _ = root_context()
    try:
        generation = generation_value(root, wait_seconds=0.0)
    except FileNotFoundError:
        return None
    if expected_generation is not None and generation != expected_generation:
        return None
    frame = {
        "nonce": nonce,
        "generation": generation,
        "provider_route": {
            "provider": provider,
            "route_id": str(uuid.UUID(hex=route_hex)),
            "provider_generation": str(uuid.UUID(hex=provider_generation_hex)),
            "phase": phase,
        },
    }
    value = json.dumps(frame, separators=(",", ":"), ensure_ascii=True).encode("ascii") + b"\n"
    if len(value) > MAX_EVENT_BYTES:
        raise ValueError("provider route event exceeded its byte bound")
    write_lifecycle_fifo(root, value)
    return generation


def _owned_route_socket(path, identity=None):
    try:
        metadata = os.stat(path, follow_symlinks=False)
    except OSError:
        return None
    if (
        not stat.S_ISSOCK(metadata.st_mode)
        or metadata.st_uid != os.geteuid()
        or stat.S_IMODE(metadata.st_mode) != 0o600
        or metadata.st_nlink != 1
        or (identity is not None and (metadata.st_dev, metadata.st_ino) != identity)
    ):
        return None
    return metadata


def _remove_owned_route_socket(path, identity):
    metadata = _owned_route_socket(path, identity)
    if metadata is None:
        return
    try:
        os.unlink(path)
    except OSError:
        pass


def begin_provider_route(root, provider):
    """Wait for exact Desktop admission before starting a managed provider."""
    route_id = str(uuid.uuid4())
    provider_generation = str(uuid.uuid4())
    path = provider_route_receipt_path(root, route_id)
    endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    identity = None
    start_sent = False
    nonce, _ = root_context()
    try:
        endpoint.bind(path)
        os.chmod(path, 0o600)
        metadata = _owned_route_socket(path)
        if metadata is None:
            raise OSError("unsafe provider route receipt socket")
        identity = (metadata.st_dev, metadata.st_ino)
        generation = emit_provider_route(
            root, provider, route_id, provider_generation, "starting"
        )
        if generation is None:
            return None
        start_sent = True
        deadline = time.monotonic() + PROVIDER_ROUTE_RECEIPT_TIMEOUT
        while time.monotonic() < deadline:
            readable, _, _ = select.select([endpoint], [], [], deadline - time.monotonic())
            if not readable:
                break
            payload = endpoint.recv(MAX_PROVIDER_ROUTE_RECEIPT_BYTES + 1)
            if len(payload) > MAX_PROVIDER_ROUTE_RECEIPT_BYTES:
                continue
            try:
                receipt = json.loads(payload)
            except (ValueError, UnicodeDecodeError):
                continue
            if (
                not isinstance(receipt, dict)
                or set(receipt) != {"kind", "version", "nonce", "generation", "route_id", "accepted"}
                or receipt.get("kind") != "provider_route_receipt"
                or receipt.get("version") != LIFECYCLE_VERSION
                or uuid.UUID(receipt.get("nonce")).hex != nonce
                or uuid.UUID(receipt.get("generation")).hex != uuid.UUID(generation).hex
                or uuid.UUID(receipt.get("route_id")).hex != uuid.UUID(route_id).hex
                or type(receipt.get("accepted")) is not bool
            ):
                continue
            if receipt["accepted"]:
                return (route_id, provider_generation, generation)
            emit_provider_route(
                root,
                provider,
                route_id,
                provider_generation,
                "cancelled",
                expected_generation=generation,
            )
            start_sent = False
            return None
        if start_sent:
            emit_provider_route(
                root,
                provider,
                route_id,
                provider_generation,
                "cancelled",
                expected_generation=generation,
            )
            start_sent = False
        return None
    except (OSError, ValueError, TypeError, AttributeError):
        if start_sent:
            try:
                emit_provider_route(
                    root,
                    provider,
                    route_id,
                    provider_generation,
                    "cancelled",
                    expected_generation=locals().get("generation"),
                )
            except Exception:
                pass
        return None
    finally:
        endpoint.close()
        if identity is not None:
            _remove_owned_route_socket(path, identity)


def generation_value(root, wait_seconds=5.0):
    path = os.path.join(root, "agent-lifecycle-generation")
    deadline = time.monotonic() + wait_seconds
    while True:
        try:
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                metadata = os.fstat(descriptor)
                if (
                    not stat.S_ISREG(metadata.st_mode)
                    or metadata.st_uid != os.geteuid()
                    or stat.S_IMODE(metadata.st_mode) != 0o600
                    or metadata.st_nlink != 1
                    or metadata.st_size > 64
                ):
                    raise OSError("unsafe managed lifecycle generation file")
                value = os.read(descriptor, 65).decode("ascii").strip()
            finally:
                os.close(descriptor)
            parsed = uuid.UUID(value)
            if parsed.int == 0:
                raise ValueError("managed lifecycle generation is nil")
            return str(parsed)
        except FileNotFoundError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.05)


def write_loss(root):
    path = os.path.join(root, "agent-events-loss")
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            metadata = os.fstat(descriptor)
            if (
                not stat.S_ISFIFO(metadata.st_mode)
                or metadata.st_uid != os.geteuid()
                or stat.S_IMODE(metadata.st_mode) != 0o600
                or metadata.st_nlink != 1
                or os.write(descriptor, b"!") != 1
            ):
                raise OSError("managed provider loss signal failed")
        finally:
            os.close(descriptor)
    except OSError:
        pass


def emit_observation(root, observation, expected_generation=None):
    encoded_id = observation.get("session_id")
    turn_id = observation.get("turn_id")
    state = observation.get("state")
    transition = observation.get("transition")
    marker = observation.get("marker")
    if (
        observation.get("provider") not in ("codex", "opencode")
        or observation.get("scope") != "root"
        or not valid_id(encoded_id)
        or (turn_id is not None and not valid_id(turn_id))
        or state not in ("working", "waiting", "idle")
        or transition not in (
            "snapshot",
            "state_changed",
            "turn_completed",
            "turn_failed",
            "turn_interrupted",
        )
        or marker not in ("live", "baseline", "recovery")
        or (state in ("working", "waiting") and turn_id is None)
        or (transition == "turn_completed" and (state != "idle" or marker != "live"))
    ):
        raise ValueError("provider lifecycle observation is invalid")
    nonce, _ = root_context()
    try:
        generation = generation_value(root, wait_seconds=0.0)
    except FileNotFoundError:
        return None
    if expected_generation is not None and generation != expected_generation:
        return None
    value = json.dumps(
        {"nonce": nonce, "generation": generation, "provider_observation": observation},
        separators=(",", ":"),
        ensure_ascii=True,
    ).encode("ascii") + b"\n"
    if len(value) > MAX_EVENT_BYTES:
        raise ValueError("provider lifecycle event exceeded its byte bound")
    path = os.path.join(root, "agent-events")
    descriptor = os.open(path, os.O_WRONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISFIFO(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
            or len(value) > os.fpathconf(descriptor, "PC_PIPE_BUF")
            or os.write(descriptor, value) != len(value)
        ):
            raise OSError("provider lifecycle FIFO write failed")
    finally:
        os.close(descriptor)
    return generation


def write_hook_identity(session_id):
    nonce, root = root_context()
    if not valid_id(session_id):
        raise ValueError("Codex supplied an invalid opaque thread ID")
    path = os.environ.get("ZMUX_PROVIDER_EVENT_SOCKET", "")
    if not path or not os.path.isabs(path):
        raise ValueError("Codex thread identity route is unavailable")
    payload = json.dumps(
        {"kind": "codex_session", "session_id": session_id},
        separators=(",", ":"),
        ensure_ascii=True,
    ).encode("ascii") + b"\n"
    if len(payload) > MAX_EVENT_BYTES:
        raise ValueError("Codex thread identity exceeded its byte bound")
    endpoint = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        endpoint.settimeout(0.5)
        endpoint.connect(path)
        endpoint.sendall(payload)
    finally:
        endpoint.close()


def codex_session_start():
    raw = sys.stdin.buffer.read(MAX_EVENT_BYTES + 1)
    if len(raw) > MAX_EVENT_BYTES:
        raise ValueError("Codex SessionStart hook input exceeded its byte bound")
    value = json.loads(raw)
    if not isinstance(value, dict) or value.get("hook_event_name") != "SessionStart":
        raise ValueError("Codex hook input was not SessionStart")
    write_hook_identity(value.get("session_id"))


def bounded_command_output(argv, byte_limit, timeout=RPC_TIMEOUT):
    """Capture one bounded metadata command without waiting past its deadline."""
    process = subprocess.Popen(
        argv,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        bufsize=0,
    )
    descriptor = process.stdout.fileno()
    os.set_blocking(descriptor, False)
    output = bytearray()
    eof = False
    deadline = time.monotonic() + timeout
    try:
        while not eof or process.poll() is None:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Codex metadata command timed out")
            readable, _, _ = select.select([descriptor], [], [], remaining)
            if readable:
                chunk = os.read(descriptor, min(8192, byte_limit + 1 - len(output)))
                if not chunk:
                    eof = True
                else:
                    output.extend(chunk)
                    if len(output) > byte_limit:
                        raise ValueError("Codex metadata command exceeded its byte bound")
            if process.poll() is not None and not readable:
                # A closed pipe remains readable until EOF; keep polling it.
                continue
        if process.returncode != 0:
            raise ValueError("Codex metadata command failed")
        return bytes(output)
    except Exception:
        if process.poll() is None:
            process.kill()
        try:
            process.wait(timeout=0.2)
        except subprocess.TimeoutExpired:
            pass
        raise
    finally:
        process.stdout.close()


class AppServer:
    """Read-only WebSocket attachment to the already-running shared app-server."""

    def __init__(self, executable):
        status_bytes = bounded_command_output(
            [executable, "app-server", "daemon", "version"],
            MAX_DAEMON_STATUS_BYTES,
        )
        status = json.loads(status_bytes)
        path = status.get("socketPath") if isinstance(status, dict) else None
        if not isinstance(status, dict) or status.get("status") != "running" or not isinstance(path, str):
            raise ValueError("Codex does not report a running shared app-server daemon")
        if not path or not os.path.isabs(path) or len(path.encode("utf-8")) > MAX_SOCKET_PATH_BYTES:
            raise ValueError("Codex daemon control socket path is outside its bounds")
        metadata = os.stat(path, follow_symlinks=True)
        if (
            not stat.S_ISSOCK(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or stat.S_IMODE(metadata.st_mode) & 0o077
        ):
            raise ValueError("Codex daemon control socket is not private to the current user")
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.socket.settimeout(RPC_TIMEOUT)
        self.socket.connect(path)
        self.buffer = bytearray()
        self.pending = deque()
        self.next_id = 0
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        request = (
            "GET / HTTP/1.1\r\n"
            "Host: localhost\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        ).encode("ascii")
        self.socket.sendall(request)
        deadline = time.monotonic() + RPC_TIMEOUT
        while b"\r\n\r\n" not in self.buffer:
            if len(self.buffer) > MAX_WEBSOCKET_HEADER_BYTES:
                raise ValueError("Codex app-server WebSocket header exceeded its byte bound")
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Codex app-server WebSocket handshake timed out")
            readable, _, _ = select.select([self.socket], [], [], remaining)
            if not readable:
                raise TimeoutError("Codex app-server WebSocket handshake timed out")
            chunk = self.socket.recv(4096)
            if not chunk:
                raise EOFError("Codex app-server WebSocket closed during handshake")
            self.buffer.extend(chunk)
        header, _, remainder = self.buffer.partition(b"\r\n\r\n")
        if len(header) > MAX_WEBSOCKET_HEADER_BYTES:
            raise ValueError("Codex app-server WebSocket header exceeded its byte bound")
        self.buffer[:] = remainder
        lines = header.decode("latin1").split("\r\n")
        if not lines or " 101 " not in lines[0]:
            raise ValueError("Codex app-server WebSocket upgrade was rejected")
        headers = {}
        for line in lines[1:]:
            name, separator, value = line.partition(":")
            if separator:
                headers[name.strip().lower()] = value.strip()
        expected_accept = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")).digest()
        ).decode("ascii")
        if (
            headers.get("upgrade", "").lower() != "websocket"
            or "upgrade" not in {value.strip().lower() for value in headers.get("connection", "").split(",")}
            or not hmac.compare_digest(headers.get("sec-websocket-accept", ""), expected_accept)
        ):
            raise ValueError("Codex app-server WebSocket handshake was invalid")
        self.socket.setblocking(False)
        result = self.rpc(
            "initialize",
            {
                "clientInfo": {"name": "zmux", "title": "Zmux", "version": "0.1.0"},
                "capabilities": {"experimentalApi": False},
            },
        )
        if not all(key in result for key in ("userAgent", "platformFamily", "platformOs")):
            raise ValueError("Codex app-server initialization was incomplete")
        self.notify("initialized", {})

    def _send_bytes(self, data, deadline):
        offset = 0
        while offset < len(data):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Codex app-server write timed out")
            _, writable, _ = select.select([], [self.socket], [], remaining)
            if not writable:
                raise TimeoutError("Codex app-server write timed out")
            try:
                count = self.socket.send(data[offset:])
            except BlockingIOError:
                continue
            if count <= 0:
                raise EOFError("Codex app-server closed while writing")
            offset += count

    def _send_frame(self, opcode, payload, deadline):
        if len(payload) > MAX_RPC_BYTES:
            raise ValueError("Codex app-server frame exceeded its byte bound")
        header = bytearray([0x80 | opcode])
        if len(payload) < 126:
            header.append(0x80 | len(payload))
        elif len(payload) <= 0xFFFF:
            header.append(0x80 | 126)
            header.extend(struct.pack("!H", len(payload)))
        else:
            header.append(0x80 | 127)
            header.extend(struct.pack("!Q", len(payload)))
        mask = os.urandom(4)
        header.extend(mask)
        masked = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        self._send_bytes(bytes(header) + masked, deadline)

    def _read_exact(self, size, deadline):
        while len(self.buffer) < size:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Codex app-server response timed out")
            readable, _, _ = select.select([self.socket], [], [], remaining)
            if not readable:
                raise TimeoutError("Codex app-server response timed out")
            try:
                chunk = self.socket.recv(max(1, size - len(self.buffer)))
            except BlockingIOError:
                continue
            if not chunk:
                raise EOFError("Codex app-server connection closed")
            self.buffer.extend(chunk)
            if len(self.buffer) > MAX_RPC_BYTES + 16:
                raise ValueError("Codex app-server response exceeded its byte bound")
        result = bytes(self.buffer[:size])
        del self.buffer[:size]
        return result

    def _read_frame(self, deadline):
        first, second = self._read_exact(2, deadline)
        if first & 0x70 or second & 0x80:
            raise ValueError("Codex app-server returned an invalid WebSocket frame")
        opcode = first & 0x0F
        final = bool(first & 0x80)
        length = second & 0x7F
        if length == 126:
            length = struct.unpack("!H", self._read_exact(2, deadline))[0]
        elif length == 127:
            length = struct.unpack("!Q", self._read_exact(8, deadline))[0]
        if length > MAX_RPC_BYTES or (opcode >= 8 and (not final or length > 125)):
            raise ValueError("Codex app-server frame exceeded its protocol bound")
        return final, opcode, self._read_exact(length, deadline)

    def read_message(self, deadline):
        message = bytearray()
        fragmented = False
        while True:
            final, opcode, payload = self._read_frame(deadline)
            if opcode == 8:
                raise EOFError("Codex app-server closed the WebSocket")
            if opcode == 9:
                self._send_frame(10, payload, deadline)
                continue
            if opcode == 10:
                continue
            if opcode == 1:
                if fragmented:
                    raise ValueError("Codex app-server interleaved WebSocket messages")
                message = bytearray(payload)
                fragmented = not final
            elif opcode == 0 and fragmented:
                message.extend(payload)
                if len(message) > MAX_RPC_BYTES:
                    raise ValueError("Codex app-server response exceeded its byte bound")
            else:
                raise ValueError("Codex app-server returned a non-text WebSocket message")
            if final:
                try:
                    value = json.loads(message.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    raise ValueError("Codex app-server returned invalid JSON") from error
                if not isinstance(value, dict):
                    raise ValueError("Codex app-server returned a non-object message")
                return value

    def rpc(self, method, params):
        self.next_id += 1
        request_id = self.next_id
        self.send_json({"method": method, "id": request_id, "params": params})
        deadline = time.monotonic() + RPC_TIMEOUT
        while True:
            response = self.read_message(deadline)
            if response.get("id") == request_id:
                if "error" in response:
                    raise ValueError("Codex app-server read request failed")
                result = response.get("result")
                if not isinstance(result, dict):
                    raise ValueError("Codex app-server response was incomplete")
                return result
            if "method" in response:
                self.queue_notification(response)

    def notify(self, method, params):
        self.send_json({"method": method, "params": params})

    def send_json(self, value):
        data = json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("ascii")
        if len(data) > MAX_RPC_BYTES:
            raise ValueError("Codex app-server request exceeded its byte bound")
        self._send_frame(1, data, time.monotonic() + RPC_TIMEOUT)

    def queue_notification(self, value):
        if len(self.pending) >= MAX_PENDING_NOTIFICATIONS:
            raise ValueError("Codex app-server notification queue overflowed")
        self.pending.append(value)

    def try_notification(self):
        if self.pending:
            return self.pending.popleft()
        try:
            value = self.read_message(time.monotonic() + 0.001)
        except TimeoutError:
            return None
        if "method" not in value:
            return None
        return value

    def snapshot(self, thread_id):
        value = self.rpc("thread/read", {"threadId": thread_id, "includeTurns": False})
        thread = value.get("thread")
        if not isinstance(thread, dict) or thread.get("id") != thread_id:
            raise ValueError("Codex app-server returned a different thread")
        if thread.get("parentThreadId") is not None:
            raise ValueError("Codex SessionStart identity belongs to a subagent thread")
        turns = self.rpc(
            "thread/turns/list",
            {
                "threadId": thread_id,
                "limit": 1,
                "sortDirection": "desc",
                "itemsView": "notLoaded",
            },
        )
        rows = turns.get("data")
        row = rows[0] if isinstance(rows, list) and rows else None
        turn_id = row.get("id") if isinstance(row, dict) else None
        if turn_id is not None and not valid_id(turn_id):
            raise ValueError("Codex app-server returned an invalid turn ID")
        status = thread.get("status")
        status_type = status.get("type") if isinstance(status, dict) else None
        if status_type == "idle":
            state = "idle"
        elif status_type == "active":
            flags = status.get("activeFlags")
            if not isinstance(flags, list):
                raise ValueError("Codex app-server returned an invalid active status")
            state = "waiting" if any(
                flag in ("waitingOnApproval", "waitingOnUserInput") for flag in flags
            ) else "working"
        elif status_type in ("notLoaded", "systemError"):
            raise ValueError("Codex thread no longer has live daemon status")
        else:
            raise ValueError("Codex app-server returned an unknown thread status")
        if state in ("working", "waiting"):
            if not isinstance(row, dict) or row.get("status") != "inProgress" or not turn_id:
                raise ValueError("Codex active thread has no exact in-progress turn")
        return state, turn_id

    def close(self):
        try:
            self._send_frame(8, b"", time.monotonic() + 0.15)
        except (OSError, TimeoutError, ValueError):
            pass
        self.socket.close()


class CodexWatcher:
    def __init__(self, server, thread_id, root):
        self.server = server
        self.thread_id = thread_id
        self.root = root
        self.active_turn = None
        self.last_state = None
        self.last_snapshot = time.monotonic()
        self.transport_generation = None

    def publish(self, state, turn_id, transition, marker):
        if self.transport_generation is None:
            return False
        return emit_observation(
            self.root,
            {
                "provider": "codex",
                "scope": "root",
                "session_id": self.thread_id,
                "turn_id": turn_id,
                "state": state,
                "transition": transition,
                "marker": marker,
            },
            expected_generation=self.transport_generation,
        )

    def attach(self):
        self.transport_generation = generation_value(self.root)
        state, turn_id = self.server.snapshot(self.thread_id)
        self.active_turn = turn_id if state in ("working", "waiting") else None
        self.last_state = state
        self.publish(state, turn_id, "snapshot", "baseline")
        self.last_snapshot = time.monotonic()

    def discard_notifications(self):
        for _ in range(MAX_PENDING_NOTIFICATIONS + 1):
            if self.server.try_notification() is None:
                return
        raise ValueError("Codex app-server notification queue did not drain")

    def notification(self, value):
        params = value.get("params")
        if not isinstance(params, dict) or params.get("threadId") != self.thread_id:
            return
        method = value.get("method")
        if method == "turn/started":
            turn = params.get("turn")
            turn_id = turn.get("id") if isinstance(turn, dict) else None
            if not valid_id(turn_id):
                raise ValueError("Codex turn start omitted its exact ID")
            if self.active_turn == turn_id and self.last_state == "working":
                return
            self.active_turn = turn_id
            self.last_state = "working"
            self.publish("working", turn_id, "state_changed", "live")
        elif method == "turn/completed":
            turn = params.get("turn")
            turn_id = turn.get("id") if isinstance(turn, dict) else None
            status = turn.get("status") if isinstance(turn, dict) else None
            if not valid_id(turn_id):
                raise ValueError("Codex turn completion omitted its exact ID")
            transition = {
                "completed": "turn_completed",
                "failed": "turn_failed",
                "interrupted": "turn_interrupted",
            }.get(status)
            if not transition:
                raise ValueError("Codex returned an unsupported turn outcome")
            self.active_turn = None
            self.last_state = "idle"
            self.publish("idle", turn_id, transition, "live")
        elif method == "thread/status/changed":
            status = params.get("status")
            status_type = status.get("type") if isinstance(status, dict) else None
            if status_type == "active":
                flags = status.get("activeFlags")
                if not isinstance(flags, list):
                    raise ValueError("Codex returned an invalid active status")
                state = "waiting" if any(
                    flag in ("waitingOnApproval", "waitingOnUserInput") for flag in flags
                ) else "working"
                turn_id = self.active_turn
                if turn_id is None:
                    snapshot_state, snapshot_turn = self.server.snapshot(self.thread_id)
                    if snapshot_state not in ("working", "waiting"):
                        raise ValueError("Codex active thread has no exact live turn")
                    turn_id = snapshot_turn
                if not valid_id(turn_id):
                    raise ValueError("Codex active thread has no exact live turn ID")
                if self.last_state == state and self.active_turn == turn_id:
                    return
                self.active_turn = turn_id
                self.last_state = state
                self.publish(state, turn_id, "state_changed", "live")
            elif status_type == "idle":
                turn_id = self.active_turn
                self.active_turn = None
                self.last_state = "idle"
                self.publish("idle", turn_id, "snapshot", "live")
            elif status_type in ("notLoaded", "systemError"):
                raise ValueError("Codex thread no longer has live daemon status")

    def poll(self):
        try:
            current_generation = generation_value(self.root, wait_seconds=0.0)
        except FileNotFoundError:
            current_generation = None
        if current_generation != self.transport_generation:
            self.discard_notifications()
            self.transport_generation = current_generation
            if current_generation is None:
                return
            state, turn_id = self.server.snapshot(self.thread_id)
            self.active_turn = turn_id if state in ("working", "waiting") else None
            self.last_state = state
            # A stream reattachment gets a current source snapshot. Buffered
            # notifications are drained so an outage cannot replay old turns.
            self.publish(state, turn_id, "snapshot", "live")
            self.discard_notifications()
            self.last_snapshot = time.monotonic()
            return
        if current_generation is None:
            self.discard_notifications()
            return
        while True:
            value = self.server.try_notification()
            if value is None:
                break
            self.notification(value)
        if time.monotonic() - self.last_snapshot >= SNAPSHOT_SECONDS:
            state, turn_id = self.server.snapshot(self.thread_id)
            self.active_turn = turn_id if state in ("working", "waiting") else None
            self.last_state = state
            # A live source refresh keeps current state fresh but cannot create
            # a transition or completion eligibility.
            self.publish(state, turn_id, "snapshot", "live")
            self.last_snapshot = time.monotonic()


def interactive_codex(args):
    if any(value in ("--help", "-h", "--version", "-V") for value in args):
        return False
    if any(
        value in ("--no-daemon", "--remote", "--remote-auth-token-env")
        or value.split("=", 1)[0] in ("--cd", "-C", "--profile", "-p")
        or value == "--dangerously-bypass-hook-trust"
        or value.startswith("--remote=")
        or value.startswith("--remote-auth-token-env=")
        for value in args
    ):
        return False
    try:
        first = first_positional(
            args,
            value_options={
                "-c", "--config", "--enable", "--disable", "-i", "--image", "-m",
                "--model", "-p", "--profile", "-s", "--sandbox", "-C", "--cd",
                "--add-dir", "-a", "--ask-for-approval",
            },
            boolean_options={
                "--strict-config", "--oss", "--approve-for-me",
                "--dangerously-bypass-approvals-and-sandbox", "--worktree", "--search",
                "--no-alt-screen", "-h", "--help", "-V", "--version",
            },
        )
    except ValueError:
        return False
    return first not in {
        "exec", "review", "login", "logout", "mcp", "mcp-server", "plugin", "completion",
        "features", "app-server", "remote-control", "debug", "sandbox", "agents", "queue",
        "archive", "delete", "unarchive", "apply", "migrate-rollouts", "migrate-nfs-db",
        "update", "doctor", "cloud", "exec-server", "help",
    }


def first_positional(args, value_options, boolean_options):
    index = 0
    while index < len(args):
        argument = args[index]
        if argument == "--":
            return None
        if not argument.startswith("-"):
            return argument
        option = argument.split("=", 1)[0]
        if option in value_options:
            if "=" not in argument:
                index += 1
                if index >= len(args):
                    raise ValueError("provider option is missing its value")
        elif option not in boolean_options:
            raise ValueError("provider option is outside the reviewed CLI surface")
        index += 1
    return None


def config_override(args):
    index = 0
    while index < len(args):
        value = args[index]
        config = None
        if value in ("-c", "--config"):
            index += 1
            if index >= len(args):
                raise ValueError("Codex --config argument is missing")
            config = args[index]
        elif value.startswith("--config="):
            config = value[len("--config=") :]
        elif value.startswith("-c="):
            config = value[len("-c=") :]
        if config is not None:
            key = config.split("=", 1)[0].strip().strip("\"'")
            if key == "hooks" or key.startswith("hooks."):
                return True
        index += 1
    return False


def toml_value(value, depth=0):
    if depth > 16:
        raise ValueError("Codex hook configuration nesting exceeds its bound")
    if value is None:
        raise ValueError("Codex hook configuration contains unsupported null")
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return json.dumps(value, allow_nan=False)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=True)
    if isinstance(value, list) and len(value) <= 256:
        return "[" + ",".join(toml_value(item, depth + 1) for item in value) + "]"
    if isinstance(value, dict) and len(value) <= 64:
        values = []
        for key, item in value.items():
            if item is None:
                continue
            values.append(json.dumps(key, ensure_ascii=True) + "=" + toml_value(item, depth + 1))
        return "{" + ",".join(values) + "}"
    raise ValueError("Codex hook configuration exceeds its bound")


def codex_untracked(executable, args, reason):
    print(f"zmux: managed SSH Codex tracking unavailable ({reason}); launching once without lifecycle tracking", file=sys.stderr)
    return subprocess.run([executable, *args]).returncode


def serve_codex_hook(listener, events, stop, root):
    listener.settimeout(0.2)
    while not stop.is_set():
        try:
            connection, _ = listener.accept()
        except socket.timeout:
            continue
        except OSError:
            return
        with connection:
            connection.settimeout(0.3)
            data = bytearray()
            try:
                while len(data) <= MAX_EVENT_BYTES:
                    chunk = connection.recv(min(1024, MAX_EVENT_BYTES + 1 - len(data)))
                    if not chunk:
                        break
                    data.extend(chunk)
                    if b"\n" in data:
                        break
                if len(data) > MAX_EVENT_BYTES:
                    raise ValueError("Codex hook identity exceeded its byte bound")
                value = json.loads(bytes(data).split(b"\n", 1)[0])
                session_id = value.get("session_id") if isinstance(value, dict) else None
                if value.get("kind") != "codex_session" or not valid_id(session_id):
                    raise ValueError("Codex hook identity was malformed")
                events.put_nowait(session_id)
            except queue.Full:
                write_loss(root)
                stop.set()
                return
            except Exception:
                write_loss(root)
                stop.set()
                return


def run_codex(args, root):
    executable = shutil.which("codex")
    if not executable:
        print("zmux: codex executable was not found on PATH", file=sys.stderr)
        return 127
    if not interactive_codex(args):
        return codex_untracked(executable, args, "this command is not an interactive Codex TUI")
    if config_override(args):
        return codex_untracked(executable, args, "this invocation overrides Codex hooks")

    runtime_dir = tempfile.mkdtemp(prefix=f"codex-{os.getpid()}-", dir=root)
    os.chmod(runtime_dir, 0o700)
    socket_path = os.path.join(runtime_dir, "hook.sock")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    events = queue.Queue(maxsize=64)
    stop = threading.Event()
    provider = None
    server = None
    listener_thread = None
    spawned = False
    route = None
    try:
        listener.bind(socket_path)
        os.chmod(socket_path, 0o600)
        listener.listen(8)
        listener_thread = threading.Thread(
            target=serve_codex_hook,
            args=(listener, events, stop, root),
            name="zmux-remote-codex-hook",
            daemon=True,
        )
        listener_thread.start()
        server = AppServer(executable)
        effective = server.rpc("config/read", {"cwd": os.getcwd(), "includeLayers": False})
        hooks = effective.get("config", {}).get("hooks", {}).get("SessionStart", [])
        if hooks is None:
            hooks = []
        if not isinstance(hooks, list) or len(hooks) >= 128:
            raise ValueError("Codex SessionStart hooks are not a bounded array")
        hooks = list(hooks)
        hooks.append(
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": 'python3 "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" codex-session-start',
                    }
                ]
            }
        )
        setting = "hooks.SessionStart=" + toml_value(hooks)
        if len(setting.encode("utf-8")) > 32768:
            raise ValueError("Codex SessionStart hook settings exceed their byte bound")
        provider_args = ["--config", setting, *args]
        environment = os.environ.copy()
        environment["ZMUX_PROVIDER_EVENT_SOCKET"] = socket_path
        route = begin_provider_route(root, "codex")
        if route is None:
            return codex_untracked(
                executable, args, "Desktop provider-route admission was not confirmed"
            )
        provider = subprocess.Popen([executable, *provider_args], env=environment)
        spawned = True
        watcher = None
        warned_at = time.monotonic() + HOOK_WAIT_WARNING
        while provider.poll() is None:
            if not stop.is_set():
                try:
                    session_id = events.get(timeout=0.05)
                except queue.Empty:
                    session_id = None
                if session_id is not None and watcher is None:
                    try:
                        watcher = CodexWatcher(server, session_id, root)
                        watcher.attach()
                    except Exception:
                        watcher = None
                        print("zmux: Codex thread could not be attached to the existing app-server; activity remains Unknown", file=sys.stderr)
                if watcher is not None:
                    watcher.poll()
            if watcher is None and time.monotonic() >= warned_at:
                print("zmux: Codex SessionStart tracking is not active; review the Zmux hook in /hooks. No lifecycle state is being inferred.", file=sys.stderr)
                warned_at = time.monotonic() + 60.0
            time.sleep(0.05)
        if watcher is not None:
            # The remote protocol has no provider-specific end frame. Fence the
            # tracked evidence so the managed stream publishes Unknown rather
            # than leaving an exited Codex session looking connected.
            write_loss(root)
        return provider.returncode if provider.returncode is not None else 1
    except Exception as error:
        if spawned:
            write_loss(root)
            print(f"zmux: Codex lifecycle evidence was lost ({error}); activity remains Unknown", file=sys.stderr)
            return provider.wait() if provider is not None else 1
        if route is not None:
            try:
                emit_provider_route(
                    root, "codex", route[0], route[1], "cancelled", expected_generation=route[2]
                )
            except Exception:
                write_loss(root)
            route = None
        return codex_untracked(executable, args, "read-only app-server/config preflight failed")
    finally:
        if route is not None:
            try:
                emit_provider_route(
                    root, "codex", route[0], route[1], "ended", expected_generation=route[2]
                )
            except Exception:
                write_loss(root)
        stop.set()
        listener.close()
        if listener_thread is not None:
            listener_thread.join(timeout=0.3)
        if server is not None:
            server.close()
        try:
            shutil.rmtree(runtime_dir)
        except FileNotFoundError:
            pass
        except OSError:
            print("zmux: remote Codex adapter files retained for review", file=sys.stderr)


def mirror_config_dir(source, destination, plugins):
    if not os.path.isabs(source) or not os.path.isdir(source):
        raise ValueError("existing OPENCODE_CONFIG_DIR is not an accessible directory")
    entries = list(os.scandir(source))
    if len(entries) > 256:
        raise ValueError("existing OpenCode config directory exceeds its entry bound")
    for entry in entries:
        if entry.name == "plugins":
            if not entry.is_dir(follow_symlinks=True):
                raise ValueError("existing OpenCode plugins path is not a directory")
            plugin_entries = list(os.scandir(entry.path))
            if len(plugin_entries) > 128:
                raise ValueError("existing OpenCode plugin directory exceeds its entry bound")
            for plugin in plugin_entries:
                os.symlink(plugin.path, os.path.join(plugins, plugin.name))
        else:
            os.symlink(entry.path, os.path.join(destination, entry.name))


def advertises_option(help_output, option):
    """Match an option declaration, not an incidental mention in help text."""
    prefix = option.encode("ascii")
    for line in help_output.splitlines():
        candidate = line.lstrip()
        if not candidate.startswith(prefix):
            continue
        suffix = candidate[len(prefix) :]
        if not suffix or suffix[:1] in (b" ", b"\t", b"=", b"<"):
            return True
    return False


def run_opencode(args, root):
    executable = shutil.which("opencode")
    if not executable:
        print("zmux: opencode executable was not found on PATH", file=sys.stderr)
        return 127
    if not interactive_opencode(args):
        return opencode_untracked(executable, args, "this command is not an interactive OpenCode TUI")
    try:
        expected_session = expected_opencode_session(args)
    except ValueError:
        return opencode_untracked(executable, args, "the explicit OpenCode session ID is ambiguous")
    route = None
    admission_attempted = False
    try:
        help_result = bounded_command_output([executable, "--help"], 65536, timeout=2.0)
        if not advertises_option(help_result, "--standalone"):
            raise ValueError("installed OpenCode does not advertise --standalone")
        config_dir = tempfile.mkdtemp(prefix=f"opencode-{os.getpid()}-", dir=root)
        os.chmod(config_dir, 0o700)
        plugins = os.path.join(config_dir, "plugins")
        os.mkdir(plugins, 0o700)
        existing = os.environ.get("OPENCODE_CONFIG_DIR")
        if existing:
            mirror_config_dir(existing, config_dir, plugins)
        plugin_name = "zmux-provider-" + uuid.uuid4().hex + ".js"
        plugin_path = os.path.join(plugins, plugin_name)
        with open(os.path.join(root, "provider-opencode-plugin.js"), "rb") as source:
            plugin = source.read(131073)
        if len(plugin) > 131072:
            raise ValueError("OpenCode provider plugin exceeded its byte bound")
        descriptor = os.open(plugin_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
        with os.fdopen(descriptor, "wb") as output:
            output.write(plugin)
        provider_args = list(args)
        if "--standalone" not in provider_args:
            provider_args.insert(0, "--standalone")
        environment = os.environ.copy()
        environment["OPENCODE_CONFIG_DIR"] = config_dir
        environment["ZMUX_PROVIDER_EVENTS_FIFO"] = os.path.join(root, "agent-events")
        environment["ZMUX_PROVIDER_LOSS_FIFO"] = os.path.join(root, "agent-events-loss")
        if expected_session is not None:
            environment["ZMUX_OPENCODE_EXPECTED_SESSION_ID"] = expected_session
        admission_attempted = True
        route = begin_provider_route(root, "opencode")
        if route is None:
            raise ValueError("Desktop provider-route admission was not confirmed")
        provider = subprocess.Popen([executable, *provider_args], env=environment)
    except Exception as error:
        if "provider" in locals():
            # A process may have started; never replay an uncertain launch.
            write_loss(root)
            print(f"zmux: OpenCode lifecycle setup failed after launch ({error}); activity remains Unknown", file=sys.stderr)
            return provider.wait()
        if route is not None:
            try:
                emit_provider_route(
                    root,
                    "opencode",
                    route[0],
                    route[1],
                    "cancelled",
                    expected_generation=route[2],
                )
            except Exception:
                write_loss(root)
        config = locals().get("config_dir")
        if config is not None:
            try:
                shutil.rmtree(config)
            except OSError:
                pass
        reason = (
            "Desktop provider-route admission was not confirmed"
            if admission_attempted
            else "invocation-scoped plugin setup failed"
        )
        return opencode_untracked(executable, args, reason)
    try:
        warned_at = time.monotonic() + HOOK_WAIT_WARNING * 2
        while provider.poll() is None:
            if time.monotonic() >= warned_at:
                print(
                    "zmux: OpenCode root-session tracking has not reported a session; review Zmux plugin loading and trust in OpenCode's startup output. Activity remains Unknown.",
                    file=sys.stderr,
                )
                warned_at = time.monotonic() + 60.0
            time.sleep(0.05)
        return provider.returncode if provider.returncode is not None else 1
    finally:
        if route is not None:
            try:
                emit_provider_route(
                    root,
                    "opencode",
                    route[0],
                    route[1],
                    "ended",
                    expected_generation=route[2],
                )
            except Exception:
                write_loss(root)
        try:
            shutil.rmtree(config_dir)
        except FileNotFoundError:
            pass
        except OSError:
            print("zmux: remote OpenCode adapter files retained for review", file=sys.stderr)


def interactive_opencode(args):
    if any(value in ("--help", "-h", "--version", "-v") for value in args):
        return False
    if any(value == "--server" or value.startswith("--server=") for value in args):
        return False
    try:
        first = first_positional(
            args,
            value_options={"--session", "-s", "--prompt", "--server", "--log-level", "--completions"},
            boolean_options={
                "--standalone", "--auto", "--continue", "-c", "--print-logs", "--wizard",
                "--help", "-h", "--version", "-v",
            },
        )
    except ValueError:
        return False
    return first not in {
        "run", "serve", "acp", "api", "auth", "mcp", "models", "plugin", "session",
        "service", "debug", "upgrade", "update", "uninstall", "reload", "pair", "stats",
    }


def expected_opencode_session(args):
    expected = None
    index = 0
    value_options = {"--prompt", "--server", "--log-level", "--completions"}
    while index < len(args):
        argument = args[index]
        if argument == "--":
            break
        if argument in ("--session", "-s"):
            index += 1
            if index >= len(args) or not valid_id(args[index]):
                raise ValueError("OpenCode session option is missing or invalid")
            value = args[index]
        elif argument.startswith("--session=") or argument.startswith("-s="):
            value = argument.split("=", 1)[1]
            if not valid_id(value):
                raise ValueError("OpenCode session option is invalid")
        else:
            option = argument.split("=", 1)[0]
            if option in value_options and "=" not in argument:
                index += 1
        if argument in ("--session", "-s") or argument.startswith("--session=") or argument.startswith("-s="):
            if expected is not None:
                raise ValueError("OpenCode session option is ambiguous")
            expected = value
        index += 1
    return expected


def opencode_untracked(executable, args, reason):
    print(f"zmux: managed SSH OpenCode tracking unavailable ({reason}); launching once without lifecycle tracking", file=sys.stderr)
    return subprocess.run([executable, *args]).returncode


def main():
    if len(sys.argv) >= 2 and sys.argv[1] == "codex-session-start":
        codex_session_start()
        return 0
    if len(sys.argv) < 3 or sys.argv[2] != "--" or sys.argv[1] not in ("codex", "opencode"):
        print("zmux: provider launch helper requires codex|opencode -- [arguments]", file=sys.stderr)
        return 64
    try:
        _, root = root_context()
    except Exception as error:
        print(f"zmux: managed provider tracking unavailable ({error})", file=sys.stderr)
        executable = shutil.which(sys.argv[1])
        if not executable:
            return 127
        return subprocess.run([executable, *sys.argv[3:]]).returncode
    args = sys.argv[3:]
    if sys.argv[1] == "codex":
        return run_codex(args, root)
    return run_opencode(args, root)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"zmux: provider helper failed ({error})", file=sys.stderr)
        raise SystemExit(1)
