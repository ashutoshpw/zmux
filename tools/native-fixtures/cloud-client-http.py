#!/usr/bin/env python3
"""Run the real native CloudClient against a bounded, owned loopback HTTP server."""
import argparse
import hashlib
import http.server
import json
from pathlib import Path
import platform
import secrets
import subprocess
import sys
import tempfile
import threading


class FixtureServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.lock = threading.Lock()
        self.tokens = {}
        self.gates = {}
        self.pending = {}

    def gate(self, route, subject):
        with self.lock:
            gate = self.pending.pop((route, subject), None)
            if gate:
                gate["entered"].set()
        if gate and not gate["release"].wait(8):
            raise TimeoutError("Native cloud fixture gate was not released")
        return gate["status"] if gate else 200


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        # Headers and bodies include ephemeral credentials. Never log them.
        pass

    def reply(self, status, value):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass  # Cancellation deliberately closes an owned fixture request.

    def do_GET(self):
        self.handle_request()

    def do_POST(self):
        self.handle_request()

    def handle_request(self):
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 <= length <= 8192:
                return self.reply(400, {})
            body = json.loads(self.rfile.read(length)) if length else {}
            if self.path == "/fixture/hold":
                gate = {"entered": threading.Event(), "release": threading.Event(), "status": int(body["status"])}
                with self.server.lock:
                    self.server.gates[body["name"]] = gate
                    self.server.pending[(body["route"], body["subject"])] = gate
                return self.reply(200, {})
            if self.path in ("/fixture/entered", "/fixture/release"):
                with self.server.lock:
                    gate = self.server.gates[body["name"]]
                if self.path.endswith("release"):
                    gate["release"].set()
                return self.reply(200, {"entered": gate["entered"].is_set()})
            if self.path == "/v1/native/token":
                subject = body["code"]
                if subject == "reject":
                    return self.reply(401, {})
                status = self.server.gate("token", subject)
                if status != 200:
                    return self.reply(status, {})
                token = secrets.token_urlsafe(32)
                with self.server.lock:
                    self.server.tokens[token] = subject
                return self.reply(200, {"access_token": token, "token_type": "Bearer", "expires_in": 3600})
            authorization = self.headers.get("Authorization", "")
            with self.server.lock:
                subject = self.server.tokens.get(authorization.removeprefix("Bearer "))
            if not authorization.startswith("Bearer ") or subject is None:
                return self.reply(401, {})
            route = {"/v1/me": "me", "/v1/devices": "devices", "/v1/native/revoke": "revoke"}.get(self.path)
            if route is None:
                return self.reply(404, {})
            status = self.server.gate(route, subject)
            if route == "revoke":
                with self.server.lock:
                    self.server.tokens.pop(authorization.removeprefix("Bearer "), None)
            if status != 200:
                return self.reply(status, {})
            if route == "me":
                return self.reply(200, {"user": {"id": subject, "name": subject, "email": "fixture@example.invalid"}})
            if route == "devices":
                return self.reply(200, {"devices": [{"id": "device_" + subject, "name": subject, "platform": "macos"}]})
            return self.reply(200, {})
        except Exception as error:
            # Do not stringify exceptions containing a request or credential.
            print("Native cloud fixture request failed: " + type(error).__name__, flush=True)
            self.reply(500, {})


def exercise(root, client_source, binary, expected_failure=None):
    sys.path.insert(0, str(root / "scripts"))
    from browser_process import run_browser

    print("Native CloudClient source SHA256: " + hashlib.sha256(client_source.read_bytes()).hexdigest(), flush=True)
    subprocess.run(["xcrun", "swiftc", "-parse-as-library", "-swift-version", "6",
                    "-strict-concurrency=complete", "-warnings-as-errors", "-O",
                    str(client_source), str(root / "tools/native-fixtures/cloud-client.swift"),
                    "-framework", "Security", "-framework", "CryptoKit", "-framework", "LocalAuthentication",
                    "-o", str(binary)], check=True, timeout=60)
    server = FixtureServer()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    origin = f"http://127.0.0.1:{server.server_port}"
    owned_origin = False
    try:
        subprocess.run([str(binary), "--prepare", origin], check=True, timeout=15)
        owned_origin = True
        result = run_browser([str(binary), origin], timeout=90)
    finally:
        with server.lock:
            for gate in server.gates.values():
                gate["release"].set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        # This also runs after an assertion, crash or timeout in the probe.
        # A failed absence preflight never authorizes deleting an item.
        if owned_origin:
            subprocess.run([str(binary), "--cleanup", origin], check=True, timeout=15)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    if result.timed_out or result.output_truncated:
        raise RuntimeError("Native cloud probe timed out or exceeded its output bound")
    if expected_failure is not None:
        if result.returncode != 1 or expected_failure not in result.stderr.splitlines():
            raise RuntimeError("Pre-fix client did not fail for the intended credential race")
        print("Native CloudClient baseline race reproduced; external Keychain cleanup verified.", flush=True)
    elif result.returncode != 0:
        raise RuntimeError("Current native CloudClient fixture failed")
    else:
        print("Current native CloudClient passed; external Keychain cleanup verified.", flush=True)


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
        if args.baseline_ref:
            original = subprocess.check_output(["git", "show", args.baseline_ref + ":apps/macos/Sources/CloudClient.swift"], cwd=root)
            baseline = scratch / "CloudClient-before.swift"
            baseline.write_bytes(original)
            exercise(root, baseline, scratch / "before",
                     "Native CloudClient fixture failed: Cancelled sign-in changed the committed credential")
        exercise(root, args.client_source or root / "apps/macos/Sources/CloudClient.swift", scratch / "current")


if __name__ == "__main__":
    main()
