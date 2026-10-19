#!/usr/bin/env python3
"""Run the real native CloudClient against an owned loopback HTTP fixture."""
import argparse
import base64
import datetime
import hashlib
import http.server
import hmac
import json
from pathlib import Path
import platform
import secrets
import subprocess
import sys
import tempfile
import threading
import uuid


def canonical_uuid(value):
    if not isinstance(value, str):
        raise ValueError("Expected a canonical UUID string")
    parsed = str(uuid.UUID(value))
    if value != parsed:
        raise ValueError("Expected a canonical lowercase UUID string")
    return parsed


class FixtureState:
    def __init__(self):
        self.lock = threading.Lock()
        self.tokens = {}
        self.gates = {}
        self.pending = {}
        self.devices = {}
        self.requests = {}
        self.directory_reads = {}
        self.queue_writes = {}
        self.status_failures = {}
        self.presence_updates = {}

    def gate(self, route, subject):
        with self.lock:
            gate = self.pending.pop((route, subject), None)
            if gate:
                gate["entered"].set()
        if gate and not gate["release"].wait(8):
            raise TimeoutError("Native cloud fixture gate was not released")
        return gate["status"] if gate else 200

    @staticmethod
    def owner(subject):
        return "relay-shared-owner" if subject in ("relay-sender", "relay-host") else subject

    @staticmethod
    def encoded(data):
        return base64.urlsafe_b64encode(data).decode().rstrip("=")

    @staticmethod
    def timestamp(value):
        return value.astimezone(datetime.timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")

    def device_document(self, device):
        return {"id": device["id"], "name": device["name"], "platform": device["platform"],
                "installation_id": device["installation_id"], "public_key": device["public_key"],
                "online": device["online"], "sharing": device["sharing"],
                "sharing_epoch": device["sharing_epoch"], "last_seen_at": None, "revoked_at": None}


class FixtureServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, state):
        super().__init__(("127.0.0.1", 0), Handler)
        self.state = state


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        # Headers and bodies include ephemeral credentials. Never log them.
        pass

    def reply(self, status, value):
        body = json.dumps(value, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass  # Cancellation deliberately closes an owned fixture request.

    def no_content(self):
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def read_body(self):
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            return None
        limit = 16 * 1024 if self.path.startswith("/fixture/") else 384 * 1024
        if not 0 <= length <= limit:
            return None
        raw = self.rfile.read(length) if length else b"{}"
        try:
            value = json.loads(raw)
            return value if isinstance(value, dict) else None
        except (UnicodeDecodeError, json.JSONDecodeError):
            return None

    def subject(self):
        authorization = self.headers.get("Authorization", "")
        if not authorization.startswith("Bearer "):
            return None
        with self.server.state.lock:
            return self.server.state.tokens.get(authorization.removeprefix("Bearer "))

    def proof_device(self, subject):
        device_id = self.headers.get("X-Zmux-Device-Id", "")
        supplied_secret = self.headers.get("X-Zmux-Device-Secret", "")
        with self.server.state.lock:
            device = self.server.state.devices.get(device_id)
            if device is None or device["owner"] != self.server.state.owner(subject):
                return None
            if not hmac.compare_digest(device["device_secret"].encode(), supplied_secret.encode()):
                return None
            return dict(device)

    def do_GET(self):
        self.handle_request()

    def do_POST(self):
        self.handle_request()

    def do_DELETE(self):
        self.handle_request()

    def handle_request(self):
        try:
            body = self.read_body()
            if body is None:
                return self.reply(413, {})
            if self.path == "/fixture/hold":
                gate = {"entered": threading.Event(), "release": threading.Event(), "status": int(body["status"])}
                with self.server.state.lock:
                    self.server.state.gates[body["name"]] = gate
                    self.server.state.pending[(body["route"], body["subject"])] = gate
                return self.reply(200, {})
            if self.path in ("/fixture/entered", "/fixture/release"):
                with self.server.state.lock:
                    gate = self.server.state.gates[body["name"]]
                if self.path.endswith("release"):
                    gate["release"].set()
                return self.reply(200, {"entered": gate["entered"].is_set()})
            if self.path == "/fixture/stats":
                with self.server.state.lock:
                    return self.reply(200, {"device_reads": dict(self.server.state.directory_reads),
                                            "queue_writes": dict(self.server.state.queue_writes),
                                            "presence_updates": dict(self.server.state.presence_updates)})
            if self.path == "/fixture/reset-presence" and self.command == "POST":
                subject = body.get("subject")
                if not isinstance(subject, str) or not subject:
                    return self.reply(400, {})
                with self.server.state.lock:
                    self.server.state.presence_updates[subject] = []
                return self.reply(200, {})
            if self.path == "/fixture/fail-status":
                try:
                    status = int(body["status"])
                    device_id = str(uuid.UUID(body["device_id"]))
                except (KeyError, ValueError, TypeError):
                    return self.reply(400, {})
                if status not in (403, 429, 500):
                    return self.reply(400, {})
                with self.server.state.lock:
                    self.server.state.status_failures[device_id] = status
                return self.reply(200, {})
            if self.path == "/v1/native/token" and self.command == "POST":
                subject = body.get("code")
                if subject == "reject":
                    return self.reply(401, {})
                status = self.server.state.gate("token", subject)
                if status != 200:
                    return self.reply(status, {})
                token = secrets.token_urlsafe(32)
                with self.server.state.lock:
                    self.server.state.tokens[token] = subject
                return self.reply(200, {"access_token": token, "token_type": "Bearer", "expires_in": 3600})

            subject = self.subject()
            if subject is None:
                return self.reply(401, {})
            state = self.server.state

            if self.path == "/v1/config" and self.command == "GET":
                return self.reply(200, {"capabilities": {"stable_device_identity": True, "terminal_relay": True}})
            if self.path == "/v1/me" and self.command == "GET":
                status = state.gate("me", subject)
                if status != 200:
                    return self.reply(status, {})
                return self.reply(200, {"user": {"id": subject, "name": subject, "email": "fixture@example.invalid"}})
            if self.path == "/v1/devices" and self.command == "GET":
                with state.lock:
                    state.directory_reads[subject] = state.directory_reads.get(subject, 0) + 1
                status = state.gate("devices", subject)
                if status != 200:
                    return self.reply(status, {})
                if subject not in ("relay-sender", "relay-host", "relay-account-two"):
                    return self.reply(200, {"devices": [{"id": "device_" + subject, "name": subject, "platform": "macos"}]})
                owner = state.owner(subject)
                with state.lock:
                    devices = [state.device_document(device) for device in state.devices.values() if device["owner"] == owner]
                return self.reply(200, {"devices": devices})
            if self.path == "/v1/devices" and self.command == "POST":
                return self.register_device(subject, body)
            if self.path == "/v1/native/revoke" and self.command == "POST":
                status = state.gate("revoke", subject)
                authorization = self.headers.get("Authorization", "")
                with state.lock:
                    state.tokens.pop(authorization.removeprefix("Bearer "), None)
                if status != 200:
                    return self.reply(status, {})
                return self.reply(200, {})
            if self.path.startswith("/v1/devices/") and self.path.endswith("/presence") and self.command == "POST":
                return self.set_presence(subject, body)
            if self.path == "/v1/terminal-relay/inbox" and self.command == "GET":
                return self.inbox(subject)
            if self.path == "/v1/terminal-relay/requests" and self.command == "POST":
                return self.enqueue(subject, body)
            if self.path.startswith("/v1/terminal-relay/requests/"):
                return self.relay_request(subject, body)
            return self.reply(404, {})
        except Exception as error:
            # Do not stringify exceptions containing a request or credential.
            print("Native cloud fixture request failed: " + type(error).__name__, flush=True)
            self.reply(500, {})

    def register_device(self, subject, body):
        state = self.server.state
        try:
            installation_id = canonical_uuid(body["installation_id"])
            secret = base64.urlsafe_b64decode(body["device_secret"] + "=" * (-len(body["device_secret"]) % 4))
            public_key = base64.urlsafe_b64decode(body["public_key"] + "=" * (-len(body["public_key"]) % 4))
        except (KeyError, ValueError, TypeError):
            return self.reply(400, {})
        if len(secret) != 32 or len(public_key) != 32:
            return self.reply(400, {})
        owner = state.owner(subject)
        with state.lock:
            existing = next((device for device in state.devices.values()
                             if device["owner"] == owner and device["installation_id"] == installation_id), None)
            if existing:
                if existing["device_secret"] != body["device_secret"] or existing["public_key"] != body["public_key"]:
                    return self.reply(409, {})
                existing["name"] = body.get("name", "")
                existing["platform"] = body.get("platform", "")
                device = dict(existing)
            else:
                device = {"id": str(uuid.uuid4()), "owner": owner, "installation_id": installation_id,
                          "device_secret": body["device_secret"], "public_key": body["public_key"],
                          "name": body.get("name", ""), "platform": body.get("platform", "macos"),
                          "online": True, "sharing": False, "sharing_epoch": None}
                state.devices[device["id"]] = device
        return self.reply(200, state.device_document(device))

    def set_presence(self, subject, body):
        device = self.proof_device(subject)
        if device is None:
            return self.reply(403, {})
        path_device_id = self.path.split("/")[3]
        if path_device_id != device["id"]:
            return self.reply(403, {})
        try:
            epoch = canonical_uuid(body["epoch"])
        except (KeyError, ValueError, TypeError):
            return self.reply(400, {})
        sharing = bool(body.get("sharing"))
        with self.server.state.lock:
            self.server.state.presence_updates.setdefault(subject, []).append(sharing)
        status = self.server.state.gate("presence", subject)
        if status != 200:
            return self.reply(status, {})
        with self.server.state.lock:
            current = self.server.state.devices[device["id"]]
            current["sharing"] = sharing
            current["sharing_epoch"] = epoch if sharing else None
            current["online"] = True
        return self.reply(200, {})

    def inbox(self, subject):
        device = self.proof_device(subject)
        if device is None:
            return self.reply(403, {})
        with self.server.state.lock:
            request = next((item for item in self.server.state.requests.values()
                            if item["target_device_id"] == device["id"] and item["state"] == "pending"), None)
            if request is None:
                return self.reply(200, {"requests": []})
            request["state"] = "claimed"
            response = {"id": request["id"], "sender_device_id": request["sender_device_id"],
                        "sender_public_key": request["sender_public_key"], "payload": request["request_payload"],
                        "expires_at": request["expires_at"], "target_epoch": request["target_epoch"]}
        return self.reply(200, {"requests": [response]})

    def enqueue(self, subject, body):
        sender = self.proof_device(subject)
        if sender is None:
            return self.reply(403, {})
        state = self.server.state
        try:
            request_id = canonical_uuid(body["id"])
            target_id = canonical_uuid(body["target_device_id"])
            payload = body["payload"]
        except (KeyError, ValueError, TypeError):
            return self.reply(400, {})
        if not isinstance(payload, str) or len(payload) > 350_000:
            return self.reply(413, {})
        now = datetime.datetime.now(datetime.timezone.utc)
        with state.lock:
            target = state.devices.get(target_id)
            if (target is None or target["owner"] != sender["owner"] or not target["online"] or
                    not target["sharing"] or target["id"] == sender["id"] or target["sharing_epoch"] is None):
                return self.reply(404, {})
            if request_id in state.requests:
                old = state.requests[request_id]
                if old["sender_device_id"] != sender["id"] or old["target_device_id"] != target_id or old["request_payload"] != payload:
                    return self.reply(409, {})
                return self.reply(200, {"id": request_id, "state": old["state"], "expires_at": old["expires_at"]})
            expires_at = state.timestamp(now + datetime.timedelta(seconds=20))
            request = {"id": request_id, "state": "pending", "sender_device_id": sender["id"],
                       "sender_public_key": sender["public_key"], "sender_token": self.headers.get("Authorization", ""),
                       "target_device_id": target_id, "target_epoch": target["sharing_epoch"],
                       "request_payload": payload, "response_payload": None, "expires_at": expires_at}
            state.requests[request_id] = request
            state.queue_writes[sender["id"]] = state.queue_writes.get(sender["id"], 0) + 1
        return self.reply(200, {"id": request_id, "state": "pending", "expires_at": expires_at})

    def relay_request(self, subject, body):
        device = self.proof_device(subject)
        if device is None:
            return self.reply(403, {})
        suffix = self.path.removeprefix("/v1/terminal-relay/requests/")
        if suffix.endswith("/begin") and self.command == "POST":
            return self.begin(device, suffix.removesuffix("/begin"))
        if suffix.endswith("/response") and self.command == "POST":
            return self.respond(device, suffix.removesuffix("/response"), body)
        if self.command == "GET" and "/" not in suffix:
            return self.request_status(device, suffix)
        return self.reply(404, {})

    def begin(self, device, raw_id):
        try:
            request_id = str(uuid.UUID(raw_id))
        except ValueError:
            return self.reply(404, {})
        state = self.server.state
        with state.lock:
            request = state.requests.get(request_id)
            if request is None or request["target_device_id"] != device["id"]:
                return self.reply(404, {})
            target = state.devices.get(device["id"])
            sender_token = request["sender_token"].removeprefix("Bearer ")
            sender_subject = state.tokens.get(sender_token)
            if (sender_subject is None or state.owner(sender_subject) != device["owner"] or
                    not target["sharing"] or target["sharing_epoch"] != request["target_epoch"] or
                    request["state"] != "claimed"):
                return self.reply(403, {})
            request["state"] = "executing"
        return self.reply(200, {"id": request_id, "state": "executing"})

    def respond(self, device, raw_id, body):
        try:
            request_id = str(uuid.UUID(raw_id))
            payload = body["payload"]
        except (KeyError, ValueError, TypeError):
            return self.reply(400, {})
        if not isinstance(payload, str) or len(payload) > 350_000:
            return self.reply(413, {})
        state = self.server.state
        with state.lock:
            request = state.requests.get(request_id)
            if request is None or request["target_device_id"] != device["id"]:
                return self.reply(404, {})
            target = state.devices.get(device["id"])
            if request["state"] != "executing" or not target["sharing"] or target["sharing_epoch"] != request["target_epoch"]:
                return self.reply(403, {})
            request["response_payload"] = payload
            request["state"] = "completed"
        return self.no_content()

    def request_status(self, device, raw_id):
        try:
            request_id = str(uuid.UUID(raw_id))
        except ValueError:
            return self.reply(404, {})
        with self.server.state.lock:
            request = self.server.state.requests.get(request_id)
            if request is None or request["sender_device_id"] != device["id"]:
                return self.reply(404, {})
            failed_status = self.server.state.status_failures.pop(device["id"], None)
            if failed_status is not None:
                return self.reply(failed_status, {})
            value = {"id": request_id, "state": request["state"], "expires_at": request["expires_at"]}
            if request["state"] == "completed":
                value["payload"] = request["response_payload"]
        return self.reply(200, value)


def compile_probe(sources, binary):
    subprocess.run(["xcrun", "swiftc", "-parse-as-library", "-swift-version", "6",
                    "-strict-concurrency=complete", "-warnings-as-errors", "-O",
                    *map(str, sources), "-framework", "Security", "-framework", "CryptoKit",
                    "-framework", "LocalAuthentication", "-o", str(binary)],
                   check=True, timeout=60)


def execute(binary, args, expected_failure=None):
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
    from browser_process import run_browser

    result = run_browser([str(binary), *args], timeout=100)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    if result.timed_out or result.output_truncated:
        raise RuntimeError("Native cloud probe timed out or exceeded its output bound")
    if expected_failure is not None:
        if result.returncode != 1 or expected_failure not in result.stderr.splitlines():
            raise RuntimeError("Pre-fix client did not fail for the intended credential race")
    elif result.returncode != 0:
        raise RuntimeError("Native cloud fixture failed")


def exercise(root, client_source, binary_dir, origins, expected_failure=None):
    print("Native CloudClient source SHA256: " + hashlib.sha256(client_source.read_bytes()).hexdigest(), flush=True)
    fixture = root / "tools/native-fixtures/cloud-client.swift"
    current_sources = [root / "apps/macos/Sources/CloudDeviceIdentity.swift",
                      root / "apps/macos/Sources/CloudTerminalTransport.swift"]
    client_sources = [client_source]
    if expected_failure is None:
        client_sources.extend(current_sources)
    # The historical baseline has the older CloudFailure API and no identity helpers.
    compile_probe([*client_sources, fixture], binary_dir / "client")
    execute(binary_dir / "client", [origins[0]], expected_failure)
    if expected_failure is not None:
        return

    terminal_fixture = root / "tools/native-fixtures/cloud-terminal.swift"
    terminal_binary = binary_dir / "terminal"
    compile_probe([root / "apps/macos/Sources/CloudClient.swift", *current_sources, terminal_fixture], terminal_binary)
    lifecycle = [str(terminal_binary), "--prepare", origins[1], origins[2]]
    subprocess.run(lifecycle, check=True, timeout=15)
    owned_origins = False
    try:
        owned_origins = True
        execute(terminal_binary, origins[1:3])
    finally:
        if owned_origins:
            subprocess.run([str(terminal_binary), "--cleanup", origins[1], origins[2]], check=True, timeout=15)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--client-source", type=Path)
    source.add_argument("--baseline-ref")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        raise SystemExit("Native CloudClient acceptance requires macOS and its Keychain")
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="zmux-cloud-client-") as directory:
        scratch = Path(directory)
        state = FixtureState()
        servers = [FixtureServer(state) for _ in range(3)]
        threads = [threading.Thread(target=server.serve_forever, daemon=True) for server in servers]
        for thread in threads:
            thread.start()
        origins = [f"http://127.0.0.1:{server.server_port}" for server in servers]
        try:
            if args.baseline_ref:
                original = subprocess.check_output(["git", "show", args.baseline_ref + ":apps/macos/Sources/CloudClient.swift"], cwd=root)
                baseline = scratch / "CloudClient-before.swift"
                baseline.write_bytes(original)
                exercise(root, baseline, scratch, origins,
                         "Native CloudClient fixture failed: Cancelled sign-in changed the committed credential")
            exercise(root, args.client_source or root / "apps/macos/Sources/CloudClient.swift", scratch, origins)
        finally:
            with state.lock:
                for gate in state.gates.values():
                    gate["release"].set()
            for server in servers:
                server.shutdown()
                server.server_close()
            for thread in threads:
                thread.join(timeout=2)


if __name__ == "__main__":
    main()
