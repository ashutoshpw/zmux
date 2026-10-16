#!/usr/bin/env python3
"""Original MIT actual optional readiness/first-line owner; no package activation."""
import argparse
import errno
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import select
import shutil
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import termios
import time
import uuid

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("readiness_public_owner", HERE / "run.py")
owner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(owner)
ENV = "ZMUX_ZELLIJ_FRONTEND_READY"
FIELDS = {"version", "event", "attachment_id", "nonce", "sequence"}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity(path):
    value = Path(path).lstat()
    return [value.st_dev & ((1 << 64) - 1), value.st_ino]


def strict(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "duplicate readiness field")
        value[key] = item
    return value


class Attachment:
    def __init__(self, api, wrapper, binary, config, name, environment):
        self.api, self.binary = api, binary
        self.root = Path("/tmp").resolve() / ("a" + uuid.uuid4().hex)
        self.root.mkdir(mode=0o700)
        self.listener, self.connection, self.process, self.master = None, None, None, None
        self.output = bytearray()
        self.ready = False
        self.retained = None
        self.observation = {"input_writes": 0, "readiness_source": "authenticated_control_only"}
        try:
            self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.listener.bind(str(self.root / "r"))
            (self.root / "r").chmod(0o600)
            self.listener.listen(1)
            self.listener.setblocking(False)
            self.deadline = time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1_000_000 + 5000
            self.descriptor = {
                "version": 1, "attachment_id": str(uuid.uuid4()), "owner_uuid": str(uuid.uuid4()),
                "owner_generation": str(uuid.uuid4()), "parent_pid": os.getpid(), "parent_uid": os.geteuid(),
                "socket": str(self.root / "r"), "namespace_identity": identity(self.root),
                "socket_identity": identity(self.root / "r"), "nonce": uuid.uuid4().hex + uuid.uuid4().hex,
                "deadline_monotonic_ms": self.deadline,
            }
            descriptor = self.root / "r.json"
            descriptor.write_text(json.dumps(self.descriptor))
            descriptor.chmod(0o600)
            self.master, slave = os.openpty()
            try:
                fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
                self.process = subprocess.Popen([str(wrapper), str(binary), "--config", str(config), "attach", name,
                    "options", "--on-force-close", "detach", "--session-serialization", "false"],
                    env=dict(environment, **{ENV: str(descriptor)}), stdin=slave, stdout=slave,
                    stderr=slave, start_new_session=True)
            finally:
                os.close(slave)
        except BaseException:
            self.close()
            raise

    def pump(self, control=None, writing=False, deadline=None):
        deadline = deadline or self.deadline
        left = (deadline - time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1_000_000) / 1000
        require(left > 0, "actual frontend fixed readiness/owner deadline")
        require(not owner.child_exited(self.process), "actual frontend exited before readiness/owner observation")
        reads = [self.master] + ([control] if control is not None and not writing else [])
        readable, writable, _ = select.select(reads, [control] if writing else [], [], min(0.02, left))
        if self.master in readable:
            try:
                data = os.read(self.master, 16384)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            require(len(self.output) + len(data) <= 512 * 1024, "actual frontend output bound")
            self.output.extend(data)
            # Engine-owned query responses continue independently. No byte in
            # terminal output is used as a readiness or workload marker.
            for query, answer in ((b"\x1b[c", b"\x1b[?1;2c"), (b"\x1b[6n", b"\x1b[1;1R")):
                if query in data:
                    require(os.write(self.master, answer) == len(answer), "query response short write")
        return control in (writable if writing else readable)

    def transfer(self, data, writing):
        offset = 0
        while offset < len(data):
            try:
                count = self.connection.send(data[offset:]) if writing else self.connection.recv_into(memoryview(data)[offset:])
                require(count > 0, "readiness control premature EOF")
                offset += count
            except BlockingIOError:
                self.pump(self.connection, writing)
        require(time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1_000_000 < self.deadline, "fixed deadline expired during control transfer")

    def frame(self, event, sequence):
        return {"version": 1, "event": event, "sequence": sequence,
                "attachment_id": self.descriptor["attachment_id"], "nonce": self.descriptor["nonce"]}

    def send(self, event, sequence):
        body = json.dumps(self.frame(event, sequence), separators=(",", ":")).encode()
        self.transfer(struct.pack("<I", len(body)) + body, True)

    def same_child(self):
        require(self.process.returncode is None and not owner.child_exited(self.process), "retained child is no longer alive")
        current = self.api.process(self.process.pid)
        require(current["pid"] == self.process.pid and current["uid"] == os.geteuid()
                and os.path.samefile(current["executable"], self.binary), "frontend identity/image rejected")
        if self.retained is not None:
            require(owner.same_process(current, self.retained)
                    and current["executable"] == self.retained["executable"], "frontend changed during readiness handshake")
        # Popen + unreaped waitid identify this direct owned child; no adopted PID.
        self.retained = current
        return current

    def input_ready(self):
        while self.connection is None:
            try:
                self.connection, _ = self.listener.accept()
            except BlockingIOError:
                self.pump(self.listener)
        credentials = self.api.peer(self.connection)
        self.same_child()
        require(credentials["pid"] == self.process.pid and credentials["uid"] == os.geteuid(), "kernel readiness peer is not retained frontend")
        self.connection.setblocking(False)
        self.send("challenge", 1)
        header = bytearray(4)
        self.transfer(header, False)
        size = struct.unpack("<I", header)[0]
        require(1 <= size <= 1024, "readiness frame body bound")
        body = bytearray(size)
        self.transfer(body, False)
        frame = json.loads(body, object_pairs_hook=strict)
        require(set(frame) == FIELDS and frame == self.frame("input_ready", 2)
                and type(frame["version"]) is int and type(frame["sequence"]) is int, "strict InputReady2 rejected")
        self.send("accepted", 3)
        while True:
            try:
                require(self.connection.recv(1) == b"", "trailing readiness frame")
                break
            except BlockingIOError:
                self.pump(self.connection)
        self.same_child()
        require(identity(self.root) == self.descriptor["namespace_identity"]
                and identity(self.root / "r") == self.descriptor["socket_identity"], "readiness namespace replaced")
        require(time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1_000_000 < self.deadline, "readiness EOF missed fixed deadline")
        self.ready = True
        self.connection.close()
        self.connection = None
        self.observation.update(frontend=self.retained, kernel_peer=credentials, accepted_sequence=3,
            reporter_eof=True, frontend_alive_after_eof=True,
            deadline_monotonic_ms=self.deadline, input_ready_monotonic_ms=time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1_000_000)

    def line(self, payload, workload, expected):
        require(self.ready, "input forbidden before authenticated readiness")
        self.same_child()
        require(os.write(self.master, payload.encode() + b"\r") == len(payload.encode()) + 1, "single input line short write")
        self.observation["input_writes"] += 1
        deadline = time.clock_gettime_ns(time.CLOCK_MONOTONIC) // 1_000_000 + 3000
        while True:
            if workload.is_file():
                lines = json.loads(workload.read_text())
                if len(lines) >= len(expected):
                    require(lines == expected, "first line lost, duplicated, or changed")
                    break
            self.pump(deadline=deadline)
        self.observation.update(line_utf8_hex=payload.encode().hex(), workload_lines=lines)

    def close(self):
        try:
            if self.process is not None and self.process.returncode is None:
                if not owner.child_exited(self.process):
                    self.process.terminate()  # exact retained Child only
                    try:
                        self.process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        self.process.kill()
                        self.process.wait(timeout=2)
                else:
                    self.process.wait(timeout=2)
                self.observation["child_reaped_exit_code"] = self.process.returncode
        finally:
            if self.master is not None:
                os.close(self.master)
                self.master = None
            for stream in (self.connection, self.listener):
                if stream is not None:
                    stream.close()
            self.connection = self.listener = None
            if self.root.exists():
                shutil.rmtree(self.root)


def workload_main(root):
    # Silent real foreground workload: records actual stdin bytes without a
    # prompt, printed marker, shell command, replay or responder fixture.
    root = Path(root)
    require(ENV not in os.environ, "descriptor environment leaked into workload")
    with (root / "startup-count").open("ab") as stream:
        stream.write(b"x")
    attributes = termios.tcgetattr(0)
    attributes[3] &= ~termios.ECHO
    termios.tcsetattr(0, termios.TCSANOW, attributes)
    (root / "owner.json").write_text(json.dumps({"pid": os.getpid(), "server_pid": os.getppid()}))
    lines, pending = [], bytearray()
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        readable, _, _ = select.select([0], [], [], max(0, deadline - time.monotonic()))
        if not readable:
            return
        data = os.read(0, 4096)
        if not data:
            return
        pending.extend(data)
        require(len(pending) <= 4096, "workload line bound")
        while b"\n" in pending:
            line, _, pending = pending.partition(b"\n")
            lines.append((line + b"\n").hex())
            require(len(lines) <= 2, "duplicate/unexpected workload input")
            target = root / "lines.json"
            temporary = root / "lines.pending"
            temporary.write_text(json.dumps(lines))
            temporary.replace(target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer-receipt", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    root = Path(tempfile.mkdtemp(prefix="zj-ready-", dir="/tmp")).resolve()
    receipt = {"status": "failed", "scope": "optional dependency actual frontend only; no native or default activation",
               "host": platform.platform(), "attachments": [], "producer_receipt_sha256": digest(args.producer_receipt)}
    clients, server, api = [], None, None
    binary, environment = None, None
    try:
        artifact = json.loads(args.producer_receipt.read_text())
        require(artifact["status"] == "passed" and artifact["input_readiness"]["built"], "real readiness producer required")
        inventory_path = HERE / "input-readiness-source.json"
        inventory = artifact["input_readiness"]["inventory"]
        encoded_inventory = (json.dumps(inventory, indent=2) + "\n").encode()
        require(hashlib.sha256(encoded_inventory).hexdigest() == artifact["input_readiness"]["inventory_sha256"], "producer inventory snapshot changed")
        current_inventory = json.loads(inventory_path.read_text())
        require(current_inventory["upstream_files_sha256"] == inventory["upstream_files_sha256"]
                and current_inventory["source_commit"] == inventory["source_commit"], "compiled readiness source inventory changed")
        for name, sha in inventory["inputs_sha256"].items():
            if name != Path(__file__).name:
                require(digest(HERE / name) == sha, "readiness build source changed: " + name)
        # This acceptance fixture is not a compiler input. Its independent hash
        # permits an honest fixture repair without rebuilding unchanged code.
        receipt.update(producer_inventory_sha256=artifact["input_readiness"]["inventory_sha256"],
                       current_inventory_sha256=digest(inventory_path),
                       fixture_sha256_at_producer=inventory["inputs_sha256"][Path(__file__).name],
                       executed_fixture_sha256=digest(Path(__file__)))
        binary = args.producer_receipt.resolve().parent / "artifacts/zellij"
        require(binary == Path(artifact["binary_path"]).resolve() and digest(binary) == artifact["binary_sha256"], "retained executable changed")
        receipt.update(binary_sha256=artifact["binary_sha256"], inputs_sha256={
            path.name: digest(path) for path in (Path(__file__), HERE / "run.py", HERE / "public_identity.c", HERE / "pty_exec.c")})
        environment = owner.isolated_environment(root)
        compiler = ["xcrun", "clang"] if platform.system() == "Darwin" else ["cc"]
        flags = ["-std=c11", "-Wall", "-Wextra", "-Werror", "-Wpedantic"]
        library = root / ("public_identity.dylib" if platform.system() == "Darwin" else "public_identity.so")
        wrapper = root / "pty-exec"
        owner.command([*compiler, *flags, "-fPIC", "-dynamiclib" if platform.system() == "Darwin" else "-shared", HERE / "public_identity.c", "-o", library], environment, 30)
        owner.command([*compiler, *flags, HERE / "pty_exec.c", "-o", wrapper], environment, 30)
        api = owner.PublicAPI(library)
        config = root / "config/config.kdl"
        config.write_text('on_force_close "detach"\nsession_serialization false\nshow_release_notes false\nshow_startup_tips false\n')
        layout = root / "layout.kdl"
        layout.write_text('layout {\n pane command=' + json.dumps(sys.executable) + ' close_on_exit=true {\n  args '
                          + json.dumps(str(Path(__file__).resolve())) + ' "--workload" ' + json.dumps(str(root)) + '\n }\n}\n')
        name = "ready-" + uuid.uuid4().hex
        owner.command([binary, "--config", config, "--layout", layout, "attach", "--create-background", name,
                       "options", "--session-serialization", "false", "--on-force-close", "detach"], environment)
        owner.wait_until((root / "owner.json").is_file, "silent workload startup", 5)
        workload = json.loads((root / "owner.json").read_text())
        server = api.process(workload["server_pid"])
        require(server["uid"] == os.geteuid() and os.path.samefile(server["executable"], binary), "silent backend owner image rejected")
        backend = root / "s/0.43.0" / name
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(1)
            connection.connect(str(backend))
            credentials = api.peer(connection)
            require(credentials["pid"] == server["pid"] and credentials["uid"] == server["uid"], "silent backend kernel peer rejected")
        workload_identity = api.process(workload["pid"])
        receipt.update(server=server, workload=workload, workload_identity=workload_identity)
        expected = []
        for index in range(2):
            client = Attachment(api, wrapper, binary, config, name, environment)
            clients.append(client)
            receipt["attachments"].append(client.observation)
            client.input_ready()
            payload = "first-line-" + str(index) + "-" + uuid.uuid4().hex
            expected.append((payload.encode() + b"\n").hex())
            client.line(payload, root / "lines.json", expected)
            require((root / "startup-count").read_bytes() == b"x", "reattach replayed workload startup")
            require(owner.same_process(api.process(server["pid"]), server), "reattach changed backend identity")
            if index == 0:
                client.close()
        receipt.update(exact_lines=expected, workload_startup_count=1)
        receipt["status"] = "passed"
    except BaseException as error:
        receipt["error"] = repr(error)
        raise
    finally:
        try:
            # Authenticated isolated backend control, never an adopted PID signal.
            if server is not None and not owner.original_process_gone(api, server):
                require(owner.same_process(api.process(server["pid"]), server), "backend changed before cleanup")
                receipt["backend_close"] = owner.command([binary, "kill-session", name], environment, 5)
                owner.wait_until(lambda: owner.original_process_gone(api, server), "isolated backend close", 5)
                owner.wait_until(lambda: owner.original_process_gone(api, workload_identity), "silent workload close", 5)
                receipt.update(backend_gone=True, workload_gone=True)
        except BaseException as error:
            receipt["status"] = "failed"
            receipt["cleanup_error"] = repr(error)
            raise
        finally:
            for index, client in enumerate(clients):
                client.close()
                (output / ("pty-" + str(index) + ".log")).write_bytes(client.output)
            receipt["retained_source_unchanged"] = (all(digest(HERE / name) == sha for name, sha in inventory["inputs_sha256"].items() if name != Path(__file__).name)
                and all(digest(HERE / name) == sha for name, sha in receipt["inputs_sha256"].items())) if "inventory" in locals() and "inputs_sha256" in receipt else False
            receipt["outputs_sha256"] = {path.name: digest(path) for path in output.iterdir() if path.is_file()}
            if not receipt["retained_source_unchanged"]:
                receipt["status"] = "failed"
            (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
            shutil.rmtree(root)
    require(receipt["status"] == "passed", "owner evidence not accepted")
    print(json.dumps({"status": receipt["status"], "receipt": str(output / "receipt.json")}))


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--workload":
        workload_main(sys.argv[2])
    else:
        main()
