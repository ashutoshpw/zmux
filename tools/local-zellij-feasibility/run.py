#!/usr/bin/env python3
"""Isolated public peer/process APIs and pinned zellij transport feasibility."""
import argparse
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import selectors
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import termios
import threading
import time
import traceback
import urllib.request
import uuid

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
LIMIT = 65536


class CommandFailure(RuntimeError):
    def __init__(self, argv, result):
        self.result = result
        super().__init__("Command failed: " + str(argv) + ": " + str(result))


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def wait_until(predicate, context, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise RuntimeError("Deadline: " + context)


def child_exited(process):
    # Observe without reaping: retain ownership of the PID/process group until
    # transport cleanup finishes. poll()/wait() would release that identity.
    return process.returncode is not None or os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def confirm_darwin_zombie_group(process, environment):
    # Darwin may reject killpg for a group containing only exited processes.
    # Do not accept EPERM from a live/uninspectable group. The unreaped leader
    # pins its group identity while the public ps snapshot verifies every member.
    require(platform.system() == "Darwin" and process.returncode is None
            and os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None,
            "Permission denied while a command leader may still execute")
    probe = subprocess.Popen(["/bin/ps", "-A", "-o", "pid=,pgid=,stat="],
                             env=environment, stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    selector = selectors.DefaultSelector()
    selector.register(probe.stdout, selectors.EVENT_READ)
    output = bytearray()
    deadline = time.monotonic() + 2
    try:
        while selector.get_map():
            require(time.monotonic() < deadline, "Process-group inspection timed out")
            for key, _ in selector.select(0.03):
                data = os.read(key.fileobj.fileno(), 16384)
                if not data:
                    selector.unregister(key.fileobj)
                require(len(output) + len(data) <= LIMIT, "Process-group inspection exceeded its bound")
                output.extend(data)
        require(probe.wait(timeout=1) == 0, "Public process-group inspection failed")
    finally:
        selector.close()
        probe.stdout.close()
        if probe.poll() is None:
            probe.kill()
        probe.wait(timeout=2)
    members = []
    for row in output.decode("ascii", errors="strict").splitlines():
        fields = row.split()
        require(len(fields) == 3, "Malformed public process-group snapshot")
        pid, group = int(fields[0]), int(fields[1])
        if group == process.pid:
            members.append({"pid": pid, "state": fields[2]})
    require(any(member["pid"] == process.pid for member in members),
            "Public snapshot omitted the unreaped group leader")
    require(all(member["state"].startswith("Z") for member in members),
            "Permission denied with live process-group members: " + str(members))
    return members


def command(argv, environment, timeout=10):
    process = subprocess.Popen([str(x) for x in argv], env=environment, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    kept = bytearray()
    seen = 0
    deadline = time.monotonic() + timeout
    timed_out = False
    zombie_group = None
    try:
        while selector.get_map() or not child_exited(process):
            if time.monotonic() >= deadline:
                timed_out = True
                break
            for key, _ in selector.select(0.03):
                data = os.read(key.fileobj.fileno(), 16384)
                if not data:
                    selector.unregister(key.fileobj)
                seen += len(data)
                kept.extend(data[:max(0, LIMIT - len(kept))])
    finally:
        selector.close()
        process.stdout.close()
        # The leader is still unreaped, so this group cannot have been reused.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError as error:
            if error.errno != errno.EPERM:
                raise
            zombie_group = confirm_darwin_zombie_group(process, environment)
        finally:
            code = process.wait(timeout=5)
    result = {"exit_code": code, "timed_out": timed_out, "bytes_seen": seen,
              "output": kept.decode(errors="replace"), "confirmed_zombie_group": zombie_group}
    if timed_out or code != 0:
        raise CommandFailure(argv, result)
    return result


class ProcessInfo(ctypes.Structure):
    _fields_ = [("pid", ctypes.c_int64), ("uid", ctypes.c_uint64),
                ("start_a", ctypes.c_uint64), ("start_b", ctypes.c_uint64),
                ("boot", ctypes.c_char * 80), ("executable", ctypes.c_char * 4096)]


class PeerInfo(ctypes.Structure):
    _fields_ = [("pid", ctypes.c_int64), ("uid", ctypes.c_uint64), ("gid", ctypes.c_uint64)]


class PublicAPI:
    def __init__(self, library):
        self.library = ctypes.CDLL(str(library))
        self.library.zjfx_process_size.restype = ctypes.c_size_t
        self.library.zjfx_peer_size.restype = ctypes.c_size_t
        self.library.zjfx_api.restype = ctypes.c_char_p
        self.library.zjfx_process_read.argtypes = [ctypes.c_int64, ctypes.POINTER(ProcessInfo)]
        self.library.zjfx_peer_read.argtypes = [ctypes.c_int, ctypes.POINTER(PeerInfo)]
        require(self.library.zjfx_process_size() == ctypes.sizeof(ProcessInfo), "Own process ABI mismatch")
        require(self.library.zjfx_peer_size() == ctypes.sizeof(PeerInfo), "Own peer ABI mismatch")

    @staticmethod
    def value(info):
        return {"pid": info.pid, "uid": info.uid, "start_a": info.start_a, "start_b": info.start_b,
                "boot": info.boot.decode(), "executable": info.executable.decode()}

    def process(self, pid):
        info = ProcessInfo()
        error = self.library.zjfx_process_read(pid, ctypes.byref(info))
        if error:
            raise OSError(error, "public process query: " + os.strerror(error))
        return self.value(info)

    def peer(self, connection):
        peer = PeerInfo()
        error = self.library.zjfx_peer_read(connection.fileno(), ctypes.byref(peer))
        if error:
            raise OSError(error, "public socket peer query: " + os.strerror(error))
        return {"pid": peer.pid, "uid": peer.uid, "gid": peer.gid}


def same_process(first, second):
    return all(first[key] == second[key] for key in ("pid", "uid", "start_a", "start_b", "boot"))


def isolated_environment(root):
    # Preserve HOME, but carry no CI/app credentials into the downloaded owner.
    allowed = ("PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TZ")
    environment = {key: os.environ[key] for key in allowed if key in os.environ}
    for directory in ("s", "config", "cache", "data", "runtime", "tmp"):
        (root / directory).mkdir(mode=0o700)
    environment.update(ZELLIJ_SOCKET_DIR=str(root / "s"), ZELLIJ_CONFIG_DIR=str(root / "config"),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"),
                       XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "runtime"),
                       TMPDIR=str(root / "tmp"), TERM="xterm-256color", SHELL="/bin/sh")
    return environment


def containment_fixture(api, root, environment):
    observations = []
    for inherits_pipe in (False, True):
        path = root / ("pipe-child" if inherits_pipe else "closed-child")
        code = ("import pathlib,subprocess,sys; child=subprocess.Popen([sys.executable,'-c',"
                "'import time; time.sleep(60)']" + ("" if inherits_pipe else ",stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL")
                + "); pathlib.Path(sys.argv[1]).write_text(str(child.pid))")
        try:
            result = command([sys.executable, "-c", code, path], environment, 0.5 if inherits_pipe else 5)
            require(not inherits_pipe, "Inherited pipe unexpectedly ended before deadline")
        except CommandFailure as error:
            require(inherits_pipe and error.result["timed_out"], "Containment failed for an unrelated reason")
            result = error.result
        require(path.is_file(), "Containment child was not actually started")
        descendant = int(path.read_text())
        def stopped():
            try:
                api.process(descendant)
                return False
            except OSError as error:
                if error.errno in (errno.ENOENT, errno.ESRCH):
                    return True
                raise
        wait_until(stopped, "owned descendant execution ended", 3)
        observations.append({"inherits_pipe": inherits_pipe, "process": descendant, "result": result,
                             "execution_ended": True, "expected_timeout": inherits_pipe})
    return observations


def credential_fixture(api, root, environment):
    path = root / "peer.sock"
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(str(path))
    listener.listen(2)
    listener.settimeout(5)
    children = []
    observations = []
    # Framework Python launchers on Darwin can exec a different interpreter image.
    # Spawn the independently inspected current image and verify that exact inode.
    interpreter = api.process(os.getpid())["executable"]
    try:
        for role in ("owned", "unrelated"):
            began = time.time()
            child = subprocess.Popen([interpreter, "-B", str(HERE / "run.py"),
                                      "--peer-child", str(path), role], env=environment,
                                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL, start_new_session=True)
            children.append(child)
            connection, _ = listener.accept()
            with connection:
                connection.settimeout(5)
                credentials = api.peer(connection)
                peer = {**api.process(credentials["pid"]), **credentials}
                require(peer["pid"] == child.pid and peer["uid"] == os.geteuid(), "SDK did not return real child identity")
                require(os.path.samefile(peer["executable"], interpreter),
                        "Unexpected peer executable: " + peer["executable"] + "; expected " + interpreter)
                if platform.system() == "Darwin":
                    require(began - 2 <= peer["start_a"] <= time.time() + 2, "Darwin process start fields do not match spawn time")
                    require(peer["start_b"] < 1000000, "Darwin start microseconds invalid")
                else:
                    ticks = os.sysconf("SC_CLK_TCK")
                    uptime = float(Path("/proc/uptime").read_text().split()[0])
                    require(uptime - 5 <= peer["start_a"] / ticks <= uptime + 1, "Linux process start fields invalid")
                # Authorization is anchored to the independently owned Popen PID,
                # not to a PID or nonce supplied by the connecting process.
                expected = child.pid if role == "owned" else children[0].pid
                accepted = peer["pid"] == expected
                if accepted:
                    require(connection.recv(64) == b"GENUINE_CHILD", "Real child payload changed")
                    connection.sendall(b"ACK")
                observations.append({"role": role, "actual": peer, "expected_pid": expected, "accepted": accepted})
            wait_until(lambda: child_exited(child), "peer child exit", 5)
            status = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            require(status is not None and status.si_code == os.CLD_EXITED and status.si_status == 0,
                    "Peer child observed incorrect authorization")
        require(observations[0]["accepted"] and not observations[1]["accepted"], "Unrelated peer was not rejected")
        for child in children:
            child.wait(timeout=1)
        return observations
    finally:
        listener.close()
        for child in children:
            if child.returncode is None:
                child.kill()
                child.wait(timeout=5)


class Broker:
    def __init__(self, api, root, backend, server, environment):
        self.api, self.backend, self.server = api, backend, server
        self.root = root
        metadata = root.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.geteuid()
                and stat.S_IMODE(metadata.st_mode) == 0o700, "Broker root is not private and owned")
        self.root_identity = (metadata.st_dev, metadata.st_ino)
        self.listener, self.acceptor, self.path, self.socket_inode = None, None, None, None
        self.closed = False
        self.expected = None
        self.published = threading.Event()
        self.stop = threading.Event()
        self.workers, self.records, self.rejected, self.errors, self.closes = [], [], [], [], []
        self.record_lock = threading.Lock()
        self.peak_connections = 0
        try:
            self.environment = isolated_environment(root)
            self.environment["ZELLIJ_CONFIG_DIR"] = environment["ZELLIJ_CONFIG_DIR"]
            version = root / "s/0.43.0"
            version.mkdir(mode=0o700)
            self.path = version / backend.name
            require(len(os.fsencode(self.path)) < 100, "Broker socket path exceeds portable bound")
            self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.listener.bind(str(self.path))
            self.socket_inode = self.path.lstat().st_ino
            self.listener.listen(4)
            self.listener.settimeout(0.05)
            self.acceptor = threading.Thread(target=self.accept, name="fixture-accept")
            self.acceptor.start()
        except Exception as error:
            try:
                self.close()
            except Exception as cleanup:
                raise ExceptionGroup("Broker construction and cleanup failed", [error, cleanup]) from error
            raise

    def bind_child(self, process):
        require(process.returncode is None, "Owned child was already reaped")
        self.owned_child = process
        self.expected = {"pid": process.pid, "uid": os.geteuid()}
        self.published.set()

    def accept(self):
        try:
            while not self.stop.is_set():
                try:
                    connection, _ = self.listener.accept()
                except socket.timeout:
                    continue
                retained = []
                for worker in self.workers:
                    if worker.is_alive():
                        retained.append(worker)
                    else:
                        worker.join()
                self.workers = retained
                if len(self.workers) >= 4:
                    connection.close()
                    with self.record_lock:
                        if len(self.rejected) < 16:
                            self.rejected.append({"reason": "connection_cap"})
                    continue
                worker = threading.Thread(target=self.relay, args=(connection,), name="fixture-relay")
                self.workers.append(worker)
                self.peak_connections = max(self.peak_connections, len(self.workers))
                worker.start()
        except OSError as error:
            if not self.stop.is_set():
                self.fail("accept: " + repr(error))

    def fail(self, message):
        with self.record_lock:
            if len(self.errors) < 16:
                self.errors.append(message)
        self.stop.set()

    def normal_close(self, reason):
        with self.record_lock:
            if len(self.closes) < 32:
                self.closes.append(reason)

    def relay(self, client):
        backend = None
        try:
            require(self.published.wait(2), "Owned child identity was not published")
            peer = self.api.peer(client)
            if peer["pid"] != self.expected["pid"] or peer["uid"] != self.expected["uid"]:
                with self.record_lock:
                    require(len(self.rejected) < 16, "Rejected-peer receipt bound exceeded")
                    self.rejected.append({"reason": "not_owned_child", "actual": peer})
                return
            require(self.owned_child.returncode is None, "Child was reaped before peer authorization")
            backend = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            backend.settimeout(2)
            backend.connect(str(self.backend))
            credentials = self.api.peer(backend)
            server = {**self.api.process(credentials["pid"]), **credentials}
            require(same_process(server, self.server), "Backend peer identity changed")
            record = {"client": peer, "server": server, "to_client": 0, "to_server": 0,
                      "peak_buffer_bytes": 0, "owned_child_unreaped": True}
            with self.record_lock:
                require(len(self.records) < 32, "Connection receipt bound exceeded")
                self.records.append(record)
            streams = (client, backend)
            other = {client: backend, backend: client}
            pending = {client: bytearray(), backend: bytearray()}
            eof, closed = set(), set()
            for stream in streams:
                stream.setblocking(False)
            while not self.stop.is_set():
                readable = [stream for stream in streams if stream not in eof and len(pending[other[stream]]) < LIMIT]
                writable = [stream for stream in streams if pending[stream]]
                if len(eof) == 2 and not writable:
                    break
                reads, writes, _ = select.select(readable, writable, [], 0.03)
                for stream in reads:
                    try:
                        data = stream.recv(min(16384, LIMIT - len(pending[other[stream]])))
                    except BlockingIOError:
                        continue
                    if data:
                        pending[other[stream]].extend(data)
                        record["to_server" if stream is client else "to_client"] += len(data)
                        record["peak_buffer_bytes"] = max(record["peak_buffer_bytes"], len(pending[other[stream]]))
                    else:
                        eof.add(stream)
                for stream in writes:
                    try:
                        sent = stream.send(pending[stream])
                    except BlockingIOError:
                        continue
                    require(sent > 0, "Zero-byte socket write")
                    del pending[stream][:sent]
                for stream in streams:
                    if stream in eof and not pending[other[stream]] and other[stream] not in closed:
                        other[stream].shutdown(socket.SHUT_WR)
                        closed.add(other[stream])
        except (ConnectionResetError, BrokenPipeError) as error:
            self.normal_close(type(error).__name__)
        except OSError as error:
            if error.errno == errno.ENOTCONN:
                self.normal_close("ENOTCONN")
            elif not self.stop.is_set():
                self.fail("relay: " + repr(error))
        except Exception as error:
            self.fail("relay: " + repr(error))
        finally:
            client.close()
            if backend is not None:
                backend.close()

    def close(self):
        if self.closed:
            return
        self.stop.set()
        if self.listener is not None:
            self.listener.close()
        if self.acceptor is not None and self.acceptor.ident is not None:
            self.acceptor.join(timeout=3)
        for worker in self.workers:
            worker.join(timeout=3)
        require((self.acceptor is None or not self.acceptor.is_alive())
                and not any(w.is_alive() for w in self.workers), "Broker physical workers survived cleanup")
        self.closed = True
        if self.socket_inode is not None:
            require(self.path.lstat().st_ino == self.socket_inode, "Broker socket was replaced before cleanup")
        metadata = self.root.lstat()
        require((metadata.st_dev, metadata.st_ino) == self.root_identity
                and stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.geteuid(),
                "Broker root was replaced before cleanup")
        shutil.rmtree(self.root)
        require(not self.errors, "Broker errors: " + str(self.errors))


class TerminalClient:
    def __init__(self, wrapper, binary, config, name, broker):
        self.broker = broker
        self.master, self.process = None, None
        self.output = bytearray()
        self.closed = False
        try:
            self.master, slave = os.openpty()
            try:
                fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
                argv = [str(wrapper), str(binary), "--config", str(config), "attach", name,
                        "options", "--on-force-close", "detach", "--session-serialization", "false"]
                self.process = subprocess.Popen(argv, env=broker.environment, stdin=slave, stdout=slave,
                                                stderr=slave, start_new_session=True)
            finally:
                os.close(slave)
            broker.bind_child(self.process)
        except Exception as error:
            try:
                self.close()
            except Exception as cleanup:
                raise ExceptionGroup("Terminal construction and cleanup failed", [error, cleanup]) from error
            raise

    def pump(self):
        if select.select([self.master], [], [], 0.03)[0]:
            try:
                data = os.read(self.master, 16384)
            except OSError as error:
                if error.errno == 5:
                    return
                raise
            self.output.extend(data[:max(0, LIMIT - len(self.output))])
            for query, answer in ((b"\x1b[c", b"\x1b[?1;2c"), (b"\x1b[6n", b"\x1b[1;1R")):
                if query in data:
                    os.write(self.master, answer)

    def until(self, predicate, label):
        def check():
            self.pump()
            if child_exited(self.process):
                raise RuntimeError("Terminal child exited before " + label + ": " + self.output.decode(errors="replace"))
            return predicate()
        return wait_until(check, label, 15)

    def close(self):
        if self.closed:
            return
        self.closed = True
        try:
            if self.process is not None and not child_exited(self.process):
                os.kill(self.process.pid, signal.SIGTERM)
                try:
                    wait_until(lambda: child_exited(self.process), "terminal TERM", 3)
                except RuntimeError:
                    os.kill(self.process.pid, signal.SIGKILL)
                    wait_until(lambda: child_exited(self.process), "terminal KILL", 3)
        finally:
            try:
                self.broker.close()
            finally:
                try:
                    if self.process is not None:
                        self.process.wait(timeout=3)
                finally:
                    if self.master is not None:
                        os.close(self.master)
                        self.master = None


def download_binary(pin, root):
    archive = root / "upstream.tar.gz"
    digest, count, began = hashlib.sha256(), 0, time.monotonic()
    url = "https://github.com/zellij-org/zellij/releases/download/v0.43.0/" + pin["name"]
    with urllib.request.urlopen(url, timeout=20) as response, archive.open("wb") as output:
        while data := response.read(1024 * 1024):
            count += len(data)
            require(count <= 64 * 1024 * 1024 and time.monotonic() - began < 90, "Download exceeded bounds")
            digest.update(data)
            output.write(data)
    require(digest.hexdigest() == pin["archive_sha256"], "Pinned archive hash mismatch")
    with tarfile.open(archive) as tar:
        members = tar.getmembers()
        require(len(members) == 1 and members[0].name == "zellij" and members[0].isfile(), "Unexpected archive members")
        require(members[0].size <= 160 * 1024 * 1024, "Extraction exceeded bound")
        tar.extractall(root, members=members, filter="data")
    binary = root / "zellij"
    require(hashlib.sha256(binary.read_bytes()).hexdigest() == pin["binary_sha256"], "Pinned executable hash mismatch")
    binary.chmod(0o700)
    return binary, {"url": url, "archive_sha256": digest.hexdigest(), "binary_sha256": pin["binary_sha256"], "bytes": count}


def open_descriptors():
    descriptors = set()
    for name in os.listdir("/dev/fd"):
        if not name.isdecimal():
            continue
        number = int(name)
        try:
            fcntl.fcntl(number, fcntl.F_GETFD)
            descriptors.add(number)
        except OSError as error:
            if error.errno != errno.EBADF:
                raise
    return descriptors


def failed_exec_fixture(api, binary, wrapper, config, name, backend, server, environment):
    observations = []
    for stage in ("popen", "exec"):
        before = open_descriptors()
        root = Path(tempfile.mkdtemp(prefix="zjf-", dir="/tmp")).resolve()
        broker = Broker(api, root, backend, server, environment)
        client = None
        missing = root / "nonexistent-executable"
        try:
            if stage == "popen":
                try:
                    client = TerminalClient(missing, binary, config, name, broker)
                except FileNotFoundError as error:
                    require(error.errno == errno.ENOENT and error.filename == str(missing),
                            "Failed-exec fixture rejected an unrelated path")
                else:
                    raise RuntimeError("Missing PTY executable unexpectedly started")
            else:
                client = TerminalClient(wrapper, missing, config, name, broker)
                wait_until(lambda: child_exited(client.process), "real exec failure", 5)
                client.pump()
                require(b"exec fixture:" in client.output, "PTY wrapper did not reach the failed exec")
                require(client.process.returncode is None, "Failed exec child was reaped before broker cleanup")
                client.close()
                require(client.process.returncode == 111, "PTY wrapper failed for an unrelated reason")
            require(not broker.records, "Failed executable opened a backend connection")
            require(broker.closed and not broker.acceptor.is_alive()
                    and not any(worker.is_alive() for worker in broker.workers), "Failed exec retained broker workers")
            require(not root.exists(), "Failed exec retained its private broker namespace")
            require(open_descriptors() == before, "Failed exec leaked or replaced a parent descriptor")
        finally:
            try:
                if client is not None:
                    client.close()
            finally:
                broker.close()
        observations.append({"stage": stage, "workers_joined": True, "parent_descriptors_unchanged": True,
                             "namespace_removed": True, "child_reaped": stage == "exec"})
    return observations


def zellij_fixture(api, binary, pty_wrapper, root, environment, output, observations):
    name = "zmux-" + str(uuid.uuid4())
    backend = root / "s/0.43.0" / name
    require(len(os.fsencode(backend)) < 100, "Backend socket path exceeds portable bound")
    config = root / "config/config.kdl"
    config.write_text('on_force_close "quit"\nsession_serialization false\nshow_release_notes false\nshow_startup_tips false\ndefault_shell "/bin/sh"\n')
    layout = root / "layout.kdl"
    # All fixture arguments are generated paths. JSON strings here contain no
    # arbitrary user shell/KDL input and use the compatible basic string subset.
    layout.write_text('layout {\n pane command=' + json.dumps(sys.executable)
                      + ' close_on_exit=true {\n  args ' + json.dumps(str(HERE / "shell_fixture.py"))
                      + ' ' + json.dumps(str(root)) + '\n }\n}\n')
    owner = None
    server = None
    clients = []
    brokers = []
    observations.update(name=name, semantic_gui_attachment="not_tested", clients=[])
    try:
        command([binary, "--config", config, "--layout", layout, "attach", "--create-background", name,
                 "options", "--session-serialization", "false", "--on-force-close", "detach"], environment)
        wait_until((root / "owner.json").is_file, "real foreground owner startup")
        owner = json.loads((root / "owner.json").read_text())
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(2)
            connection.connect(str(backend))
            credentials = api.peer(connection)
            candidate = {**api.process(credentials["pid"]), **credentials}
        require(candidate["pid"] == owner["server_pid"] and candidate["uid"] == os.geteuid(), "Daemon peer PID differs from actual foreground parent")
        require(os.path.samefile(candidate["executable"], binary), "Daemon peer executable differs from verified pin")
        # Cleanup authority starts only after both independent ownership checks.
        server = candidate
        observations.update(owner=owner, server=server, backend_mode=oct(stat.S_IMODE(backend.stat().st_mode)))
        observations["failed_exec_cleanup"] = failed_exec_fixture(
            api, binary, pty_wrapper, config, name, backend, server, environment)
        for index in range(2):
            proxy_root = Path(tempfile.mkdtemp(prefix="zjp-", dir="/tmp")).resolve()
            broker = Broker(api, proxy_root, backend, server, environment)
            brokers.append(broker)
            client = TerminalClient(pty_wrapper, binary, config, name, broker)
            clients.append(client)
            client.until(lambda: b"ZMUX_ZELLIJ_FEASIBILITY_READY" in client.output, "native terminal output")
            acknowledgment = root / ("input-" + str(index))
            text = "printf '%s' \"$$\" > " + str(acknowledgment) + "; printf '\\nPEER_INPUT_CONFIRMED\\n'\r"
            os.write(client.master, text.encode())
            client.until(acknowledgment.is_file, "real PTY input acknowledgement")
            require(acknowledgment.read_text() == str(owner["shell_pid"]), "Input reached the wrong foreground shell")
            actual_child = api.process(client.process.pid)
            require(os.path.samefile(actual_child["executable"], binary), "PTY child did not exec pinned zellij")
            before = len(broker.records)
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as rogue:
                rogue.settimeout(2)
                rogue.connect(str(broker.path))
                rogue.sendall(b"UNRELATED_PEER_MUST_NOT_REACH_OWNER")
                try:
                    require(rogue.recv(1) == b"", "Unrelated peer was given a response")
                except ConnectionResetError:
                    pass  # Explicit expected immediate rejection of unread input.
            wait_until(lambda: len(broker.rejected) == 1, "wrong-PID peer rejection")
            require(broker.rejected[0]["actual"]["pid"] == os.getpid(), "Rejected peer was not the independent same-UID parent")
            require(len(broker.records) == before, "Unrelated peer opened a backend connection")
            client.close()
            require(same_process(api.process(server["pid"]), server), "Client TERM changed the durable server")
            require(api.process(owner["shell_pid"])["pid"] == owner["shell_pid"], "Client TERM ended the foreground shell")
            require((root / "startup-count").read_text() == "x", "Client attachment replayed startup")
            (output / ("pty-" + str(index) + ".log")).write_bytes(client.output)
            observations["clients"].append({"process": actual_child, "connections": broker.records,
                                            "rejected": broker.rejected, "normal_closes": broker.closes,
                                            "peak_connections": broker.peak_connections, "workers_joined": True})
        require(not list((root / "cache").rglob("session-layout.kdl")), "Owner unexpectedly serialized a resurrection layout")
        return observations
    finally:
        errors = []
        for client in clients:
            try:
                client.close()
            except Exception as error:
                errors.append("client cleanup: " + repr(error))
        for broker in brokers:
            try:
                broker.close()
            except Exception as error:
                errors.append("broker cleanup: " + repr(error))
        # A helper's owned broker pins close to the original connected server.
        # No global PID kill fallback exists on Darwin.
        if server is not None:
            def original_gone():
                try:
                    return not same_process(api.process(server["pid"]), server)
                except OSError as error:
                    if error.errno in (errno.ESRCH, errno.ENOENT):
                        return True
                    raise
            closer, helper = None, None
            try:
                close_root = Path(tempfile.mkdtemp(prefix="zjc-", dir="/tmp")).resolve()
                closer = Broker(api, close_root, backend, server, environment)
                helper = subprocess.Popen([str(binary), "kill-session", name], env=closer.environment,
                                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                          stderr=subprocess.DEVNULL, start_new_session=True)
                closer.bind_child(helper)
                wait_until(lambda: child_exited(helper), "exact close helper", 5)
                # Keep the helper unreaped and broker forwarding until the owner
                # actually exits; CLI exit 0 may precede buffered command delivery.
                wait_until(original_gone, "original daemon exit through exact close", 12)
                status = os.waitid(os.P_PID, helper.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                require(status is not None and status.si_code == os.CLD_EXITED and status.si_status == 0,
                        "Exact close helper failed")
            except Exception as error:
                errors.append("owner close: " + repr(error))
            finally:
                if helper is not None and helper.returncode is None:
                    try:
                        if not child_exited(helper):
                            os.kill(helper.pid, signal.SIGKILL)
                    except Exception as error:
                        errors.append("close helper stop: " + repr(error))
                if closer is not None:
                    try:
                        closer.close()
                    except Exception as error:
                        errors.append("close broker cleanup: " + repr(error))
                if helper is not None:
                    try:
                        helper.wait(timeout=3)
                    except Exception as error:
                        errors.append("close helper reap: " + repr(error))
            try:
                wait_until(original_gone, "original daemon exit", 12)
            except Exception as error:
                errors.append("physical owner cleanup: " + repr(error))
            observations["cleanup"] = {"original_server_exited": original_gone(), "errors": errors}
        elif backend.exists():
            errors.append("unconfirmed task-owned backend remains; retained namespace for diagnosis")
        require(not errors, "; ".join(errors))


def peer_child(path, role):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(path)
        connection.sendall(b"GENUINE_CHILD" if role == "owned" else b"UNRELATED_CHILD")
        try:
            data = connection.recv(64)
        except ConnectionResetError:
            data = b""
        require(data == (b"ACK" if role == "owned" else b""), "Child observed wrong authorization outcome")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--peer-child", nargs=2, metavar=("SOCKET", "ROLE"))
    arguments = parser.parse_args()
    if arguments.peer_child:
        peer_child(*arguments.peer_child)
        return
    require(arguments.output is not None, "--output is required")
    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    output.chmod(0o700)
    receipt = {"success": False, "host": {"system": platform.system(), "machine": platform.machine(),
                                         "version": platform.platform()}, "semantic_gui_attachment": "not_tested"}
    scratch = Path(tempfile.mkdtemp(prefix="zjfx-", dir="/tmp")).resolve()
    receipt["scratch"] = str(scratch)
    try:
        require(hasattr(os, "waitid") and hasattr(os, "WNOWAIT"), "Nonreaping owned-child wait API unavailable")
        pins = json.loads((HERE / "pins.json").read_text())
        key = platform.system() + "-" + platform.machine()
        require(key in pins["assets"], "Unsupported host: " + key)
        environment = isolated_environment(scratch)
        receipt["source_command"] = command(["git", "-C", REPO, "rev-parse", "HEAD"], environment)
        receipt["source_sha"] = receipt["source_command"]["output"].strip()
        receipt["harness_sha256"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in HERE.iterdir() if p.is_file()}
        library = scratch / ("public_identity.dylib" if platform.system() == "Darwin" else "public_identity.so")
        compiler = ["xcrun", "clang"] if platform.system() == "Darwin" else ["cc"]
        receipt["compiler"] = command([*compiler, "--version"], environment)["output"]
        if platform.system() == "Darwin":
            receipt["sdk_version"] = command(["xcrun", "--show-sdk-version"], environment)["output"].strip()
        common = ["-std=c11", "-Wall", "-Wextra", "-Werror", "-Wpedantic"]
        command([*compiler, *common, "-fPIC", "-dynamiclib" if platform.system() == "Darwin" else "-shared",
                 HERE / "public_identity.c", "-o", library], environment, 30)
        wrapper = scratch / "pty-exec"
        command([*compiler, *common, HERE / "pty_exec.c", "-o", wrapper], environment, 30)
        api = PublicAPI(library)
        receipt["public_api"] = api.library.zjfx_api().decode()
        receipt["credential_fixture"] = credential_fixture(api, scratch, environment)
        receipt["process_containment"] = containment_fixture(api, scratch, environment)
        binary, receipt["download"] = download_binary(pins["assets"][key], scratch)
        receipt["version"] = command([binary, "--version"], environment)["output"].strip()
        require(receipt["version"] == "zellij " + pins["version"], "Pinned version mismatch")
        receipt["zellij"] = {}
        zellij_fixture(api, binary, wrapper, scratch, environment, output, receipt["zellij"])
        receipt["success"] = True
        receipt["scope"] = "host_public_peer_process_api_and_real_opaque_transport_only"
    except Exception as error:
        receipt["error"] = repr(error)
        (output / "failure.log").write_text(traceback.format_exc()[-LIMIT:])
        raise
    finally:
        cleanup = receipt.get("zellij", {}).get("cleanup", {})
        if cleanup.get("original_server_exited") is True and not cleanup.get("errors"):
            try:
                shutil.rmtree(scratch)
                receipt["scratch_cleanup"] = "removed_after_verified_owner_exit"
            except OSError as error:
                receipt["success"] = False
                receipt["scratch_cleanup"] = "failed_after_verified_owner_exit"
                receipt["error"] = repr(error)
        else:
            receipt["scratch_cleanup"] = "retained_for_failure_diagnosis"
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print(json.dumps({"success": receipt["success"], "receipt": str(output / "receipt.json"),
                          "scope": receipt.get("scope"), "error": receipt.get("error")}))
    require(receipt["success"], "Fixture cleanup did not complete")


if __name__ == "__main__":
    main()
