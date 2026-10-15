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


class BackendInputRejected(RuntimeError):
    def __init__(self, data, error, terminal_phase):
        self.observation = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                            "errno": error.errno, "content": "opaque_not_interpreted",
                            "requested_phase_at_rejection": terminal_phase,
                            "classification": "unconfirmed_input_loss"}
        super().__init__("Backend rejected queued client bytes: " + str(len(data)))


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


def original_process_gone(api, identity):
    try:
        return not same_process(api.process(identity["pid"]), identity)
    except OSError as error:
        if error.errno in (errno.ESRCH, errno.ENOENT):
            return True
        raise


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


def credential_fixture(api, root, environment, observations, output):
    path = root / "peer.sock"
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(str(path))
    listener.listen(2)
    listener.settimeout(5)
    children = []
    # Framework Python launchers on Darwin can exec a different interpreter image.
    # Spawn the independently inspected current image and verify that exact inode.
    interpreter = api.process(os.getpid())["executable"]
    try:
        for role in ("owned", "unrelated"):
            began = time.time()
            error_path = output / (role + "-peer.log")
            with error_path.open("wb") as error_log:
                child = subprocess.Popen([interpreter, "-B", str(HERE / "run.py"),
                                          "--peer-child", str(path), role], env=environment,
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                         stderr=error_log, start_new_session=True)
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
            observations[-1]["exit_observation"] = repr(status)
            with error_path.open("rb") as error_log:
                diagnostic = error_log.read(4096).decode(errors="replace")
            require(status is not None and status.si_code == os.CLD_EXITED and status.si_status == 0,
                    "Peer child observed incorrect authorization (" + role + "): " + repr(status) + "; " + diagnostic)
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
        self.input_rejections = []
        self.shutdown_confirmed = False
        self.terminal_phase, self.confirmed_terminal_phase = None, None
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

    def request_owner_shutdown(self):
        self.request_terminal_phase("owner_shutdown")

    def request_attachment_cancel(self):
        self.request_terminal_phase("attachment_cancel")

    def request_terminal_phase(self, phase):
        with self.record_lock:
            require(self.terminal_phase is None and not self.stop.is_set() and not self.input_rejections,
                    "Cannot change terminal phase after a prior request or transport failure")
            self.terminal_phase = phase

    def require_clean_owned_child(self):
        require(self.owned_child.returncode is None, "Terminal child was reaped before confirmation")
        status = os.waitid(os.P_PID, self.owned_child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        require(status is not None and status.si_code == os.CLD_EXITED and status.si_status == 0,
                "Terminal child did not independently exit successfully")

    def confirm_owner_shutdown(self):
        require(self.terminal_phase == "owner_shutdown", "Owner shutdown was not requested")
        require(original_process_gone(self.api, self.server), "Original backend is still live")
        self.require_clean_owned_child()
        self.confirm_terminal_phase("owner_shutdown")
        self.shutdown_confirmed = True

    def confirm_attachment_cancel(self, verify_preservation):
        require(self.terminal_phase == "attachment_cancel", "Attachment cancellation was not requested")
        require(not original_process_gone(self.api, self.server), "Attachment cancellation lost its original backend")
        self.require_clean_owned_child()
        verify_preservation()
        self.confirm_terminal_phase("attachment_cancel")

    def confirm_terminal_phase(self, phase):
        with self.record_lock:
            require(not self.errors and all(row["requested_phase_at_rejection"] == phase for row in self.input_rejections),
                    "Other or prior-phase transport failure remains")
            self.confirmed_terminal_phase = phase

    def relay(self, client):
        backend = None
        record, pending = None, None
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
                      "forwarded_to_client": 0, "forwarded_to_server": 0,
                      "discarded_to_client": 0, "discarded_to_server": 0,
                      "peak_buffer_bytes": 0, "owned_child_unreaped": True}
            with self.record_lock:
                require(len(self.records) < 32, "Connection receipt bound exceeded")
                self.records.append(record)
            streams = (client, backend)
            other = {client: backend, backend: client}
            pending = {client: bytearray(), backend: bytearray()}
            eof, closed = set(), set()
            drain_deadline = None
            for stream in streams:
                stream.setblocking(False)
            while not self.stop.is_set():
                require(drain_deadline is None or time.monotonic() < drain_deadline,
                        "Client read-side close did not drain upstream before deadline")
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
                    except ConnectionResetError:
                        self.normal_close("read_" + ("client" if stream is client else "server") + ":ECONNRESET")
                        eof.add(stream)
                        continue
                    if data:
                        pending[other[stream]].extend(data)
                        record["to_server" if stream is client else "to_client"] += len(data)
                        record["peak_buffer_bytes"] = max(record["peak_buffer_bytes"], len(pending[other[stream]]))
                    else:
                        eof.add(stream)
                for stream in writes:
                    with self.record_lock:
                        requested_before_send = self.terminal_phase
                    try:
                        sent = stream.send(pending[stream])
                    except BlockingIOError:
                        continue
                    except OSError as error:
                        if error.errno not in (errno.EPIPE, errno.ECONNRESET, errno.ENOTCONN):
                            raise
                        destination = "client" if stream is client else "server"
                        record["discarded_to_" + destination] += len(pending[stream])
                        if stream is backend:
                            raise BackendInputRejected(pending[stream], error, requested_before_send) from error
                        # The client's read half may close while its write half
                        # still supplies input. Keep that input flowing in order.
                        self.normal_close("write_client:" + errno.errorcode[error.errno])
                        pending[client].clear()
                        closed.add(client)
                        eof.add(backend)
                        drain_deadline = time.monotonic() + 2
                        continue
                    require(sent > 0, "Zero-byte socket write")
                    record["forwarded_to_client" if stream is client else "forwarded_to_server"] += sent
                    del pending[stream][:sent]
                for stream in streams:
                    if stream in eof and not pending[other[stream]] and other[stream] not in closed:
                        try:
                            other[stream].shutdown(socket.SHUT_WR)
                        except OSError as error:
                            if error.errno not in (errno.ENOTCONN, errno.EPIPE, errno.ECONNRESET):
                                raise
                            self.normal_close("shutdown_" + ("server" if stream is client else "client")
                                              + ":" + errno.errorcode[error.errno])
                        closed.add(other[stream])
        except BackendInputRejected as error:
            with self.record_lock:
                self.input_rejections.append(error.observation)
            self.stop.set()
        except OSError as error:
            self.fail("relay: " + repr(error))
        except Exception as error:
            self.fail("relay: " + repr(error))
        finally:
            if record is not None and pending is not None:
                record["pending_to_server_on_close"] = len(pending[backend])
                record["pending_to_client_on_close"] = len(pending[client])
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
        if self.input_rejections:
            require(self.confirmed_terminal_phase is not None
                    and all(row["requested_phase_at_rejection"] == self.confirmed_terminal_phase for row in self.input_rejections),
                    "Unconfirmed backend input loss: " + str(self.input_rejections))
            for row in self.input_rejections:
                row["classification"] = ("owner_exited_with_undeliverable_input" if self.confirmed_terminal_phase == "owner_shutdown"
                                         else "attachment_cancelled_with_undeliverable_input")


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

    def close(self, preserve_owner=None):
        if self.closed:
            return
        self.closed = True
        try:
            if preserve_owner is not None:
                require(self.process is not None and not child_exited(self.process), "Detach client exited before cancellation")
                self.broker.request_attachment_cancel()
            if self.process is not None and not child_exited(self.process):
                os.kill(self.process.pid, signal.SIGTERM)
                try:
                    wait_until(lambda: child_exited(self.process), "terminal TERM", 3)
                except RuntimeError:
                    os.kill(self.process.pid, signal.SIGKILL)
                    wait_until(lambda: child_exited(self.process), "terminal KILL", 3)
            if preserve_owner is not None:
                self.broker.confirm_attachment_cancel(preserve_owner)
        finally:
            try:
                self.broker.close()
                if preserve_owner is not None:
                    preserve_owner()
            finally:
                try:
                    if self.process is not None:
                        self.process.wait(timeout=3)
                finally:
                    if self.master is not None:
                        os.close(self.master)
                        self.master = None


def relay_half_close_fixture(api, root, environment, observations, *, backend_rejects_input=False,
                            requested_terminal_phase="owner_shutdown"):
    """Real opaque transport: a closed read half must not discard upstream input."""
    basename = "raw-rejected-" + requested_terminal_phase if backend_rejects_input else "raw-transport"
    backend = root / (basename + ".sock")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    broker, child = None, None
    received, digest = 0, hashlib.sha256()
    close_failure, confirmation_denied = None, None
    try:
        listener.bind(str(backend))
        listener.listen(1)
        listener.settimeout(5)
        proxy = Path(tempfile.mkdtemp(prefix="zjr-", dir="/tmp")).resolve()
        broker = Broker(api, proxy, backend, api.process(os.getpid()), environment)
        if backend_rejects_input:
            if requested_terminal_phase == "owner_shutdown":
                broker.request_owner_shutdown()
            else:
                broker.request_attachment_cancel()
        release = root / (basename + "-release")
        traffic = root / (basename + "-traffic")
        if not backend_rejects_input:
            traffic.write_text("positive delivery traffic may begin\n")
        # This is raw socket traffic, not a substitute zellij implementation.
        # Retain the connected peer until delivery is independently observed;
        # do not depend on kernel buffer size to keep Darwin credentials alive.
        code = ("import pathlib,socket,sys,time\n"
                "s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)\n"
                "s.connect(sys.argv[1]); s.shutdown(socket.SHUT_RD)\n"
                "deadline=time.monotonic()+5\n"
                "while not pathlib.Path(sys.argv[3]).is_file():\n"
                " if time.monotonic() >= deadline: raise RuntimeError('raw traffic was not released')\n"
                " time.sleep(0.01)\n"
                "s.sendall(bytes(range(256))*1024); s.shutdown(socket.SHUT_WR)\n"
                "deadline=time.monotonic()+5\n"
                "while not pathlib.Path(sys.argv[2]).is_file():\n"
                " if time.monotonic() >= deadline: raise RuntimeError('raw sink did not release peer')\n"
                " time.sleep(0.01)\n"
                "s.close()\n")
        interpreter = api.process(os.getpid())["executable"]
        child = subprocess.Popen([interpreter, "-c", code, str(broker.path), str(release), str(traffic)], env=broker.environment,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL, start_new_session=True)
        broker.bind_child(child)
        connection, _ = listener.accept()
        with connection:
            connection.settimeout(5)
            if backend_rejects_input:
                wait_until(lambda: len(broker.records) == 1, "raw backend peer authorization", 2)
                require(same_process(broker.records[0]["server"], broker.server), "Raw backend was not authenticated")
                observations["backend_authenticated_before_close"] = True
                # Read shutdown does not imply write rejection on Darwin. Close
                # the authenticated socket, then independently release traffic.
                # Its owning process remains live throughout this control.
                connection.close()
                require(connection.fileno() == -1, "Raw backend descriptor did not close")
                observations["backend_descriptor_closed_before_traffic"] = True
                traffic.write_text("authenticated backend descriptor closed\n")
                wait_until(lambda: broker.input_rejections, "live backend input rejection", 5)
                require(same_process(api.process(os.getpid()), broker.server), "Live raw backend identity changed")
                try:
                    if requested_terminal_phase == "owner_shutdown":
                        broker.confirm_owner_shutdown()
                    else:
                        def no_shell_proof():
                            raise RuntimeError("Raw rejection fixture has no owned-shell preservation proof")
                        broker.confirm_attachment_cancel(no_shell_proof)
                except RuntimeError as error:
                    confirmation_denied = str(error)
                require(confirmation_denied is not None and broker.confirmed_terminal_phase is None,
                        "Live backend input loss was reclassified as completed shutdown")
            else:
                connection.sendall(b"R")
                while data := connection.recv(16384):
                    received += len(data)
                    require(received <= 4 * LIMIT, "Raw transport input exceeded its bound")
                    digest.update(data)
        release.write_text("sink observed end of input\n")
        wait_until(lambda: child_exited(child), "raw transport child exit", 5)
    finally:
        try:
            if broker is not None:
                try:
                    broker.close()
                except RuntimeError as error:
                    if not backend_rejects_input or not broker.input_rejections or broker.errors:
                        raise
                    close_failure = str(error)
        finally:
            try:
                if child is not None:
                    if not child_exited(child):
                        child.kill()
                    child.wait(timeout=3)
            finally:
                listener.close()
                observations.update(received_bytes=received, received_sha256=digest.hexdigest(),
                                    child_exit_code=child.returncode if child is not None else None,
                                    records=broker.records if broker is not None else [],
                                    normal_closes=broker.closes if broker is not None else [],
                                    input_rejections=broker.input_rejections if broker is not None else [],
                                    errors=broker.errors if broker is not None else [])
    if backend_rejects_input:
        require(close_failure is not None and confirmation_denied is not None
                and all(row["classification"] == "unconfirmed_input_loss" for row in broker.input_rejections),
                "Live backend rejection did not remain a fatal input-loss outcome")
        observations.update(shutdown_confirmation_denied=confirmation_denied, close_failure=close_failure,
                            live_backend_preserved=True, workers_joined=True)
        return
    require(child.returncode == 0, "Raw transport child failed")
    require(received == 4 * LIMIT and digest.digest() == hashlib.sha256(bytes(range(256)) * 1024).digest(),
            "Read-side close lost or reordered upstream input: " + str(received))
    require(len(broker.records) == 1 and broker.records[0]["forwarded_to_server"] == received
            and broker.records[0]["discarded_to_server"] == 0
            and broker.records[0]["pending_to_server_on_close"] == 0,
            "Raw transport did not account for all upstream input")
    observations["workers_joined"] = True


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
        exec_status, diagnostic = None, None
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
                status = os.waitid(os.P_PID, client.process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                exec_status = {"code": status.si_code, "status": status.si_status} if status is not None else None
                diagnostic = bytes(client.output).decode(errors="replace")
                # The original C wrapper reserves 127 specifically for exec ENOENT;
                # arguments, terminal acquisition and other exec failures differ.
                require(status is not None and status.si_code == os.CLD_EXITED and status.si_status == 127,
                        "PTY wrapper did not reach exec ENOENT: " + repr(status)
                        + "; diagnostic=" + repr(bytes(client.output)))
                require(client.process.returncode is None, "Failed exec child was reaped before broker cleanup")
                client.close()
                require(client.process.returncode == 127, "PTY wrapper failed for an unrelated reason")
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
                             "namespace_removed": True, "child_reaped": stage == "exec",
                             "unreaped_exec_observation": exec_status, "pty_diagnostic": diagnostic})
    return observations


def zellij_fixture(api, binary, pty_wrapper, root, environment, output, observations):
    name = "zmux-" + str(uuid.uuid4())
    backend = root / "s/0.43.0" / name
    require(len(os.fsencode(backend)) < 100, "Backend socket path exceeds portable bound")
    config = root / "config/config.kdl"
    config.write_text('on_force_close "quit"\nsession_serialization false\nshow_release_notes false\nshow_startup_tips false\ndefault_shell "/bin/sh"\n'
                      'keybinds clear-defaults=true {\n normal {\n  bind "Ctrl q" { Quit; }\n }\n}\n')
    layout = root / "layout.kdl"
    # All fixture arguments are generated paths. JSON strings here contain no
    # arbitrary user shell/KDL input and use the compatible basic string subset.
    layout.write_text('layout {\n pane command=' + json.dumps(sys.executable)
                      + ' close_on_exit=true {\n  args ' + json.dumps(str(HERE / "shell_fixture.py"))
                      + ' ' + json.dumps(str(root)) + '\n }\n}\n')
    owner = None
    server = None
    foreground = {"shell": None, "wrapper": None}
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
        shell_identity = api.process(owner["shell_pid"])
        foreground["shell"] = shell_identity
        foreground["wrapper"] = api.process(owner["wrapper_pid"])
        observations.update(owner=owner, server=server, shell=shell_identity, wrapper=foreground["wrapper"],
                            backend_mode=oct(stat.S_IMODE(backend.stat().st_mode)))
        def preserve_owner():
            require(same_process(api.process(server["pid"]), server), "Client TERM changed the durable server")
            require(same_process(api.process(owner["shell_pid"]), shell_identity), "Client TERM changed the original foreground shell")
            require((root / "startup-count").read_text() == "x", "Client attachment replayed startup")
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
            client.close(preserve_owner)
            (output / ("pty-" + str(index) + ".log")).write_bytes(client.output)
            observations["clients"].append({"process": actual_child, "connections": broker.records,
                                            "rejected": broker.rejected, "normal_closes": broker.closes,
                                            "input_rejections": broker.input_rejections,
                                            "confirmed_terminal_phase": broker.confirmed_terminal_phase,
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
        # A retained PTY client keeps credentials available through authorization.
        # Darwin cannot return LOCAL_PEERPID after a short helper disconnects.
        # No global PID kill fallback or unauthenticated transport is allowed.
        if server is not None:
            def original_gone():
                return original_process_gone(api, server)
            closer, helper = None, None
            try:
                close_root = Path(tempfile.mkdtemp(prefix="zjc-", dir="/tmp")).resolve()
                closer = Broker(api, close_root, backend, server, environment)
                helper = TerminalClient(pty_wrapper, binary, config, name, closer)
                helper.until(lambda: b"ZMUX_ZELLIJ_FEASIBILITY_READY" in helper.output,
                             "close client real terminal readiness")
                actual_child = api.process(helper.process.pid)
                require(os.path.samefile(actual_child["executable"], binary), "Close client did not exec pinned zellij")
                acknowledgment = root / "close-input"
                text = "printf '%s' \"$$\" > " + str(acknowledgment) + "; printf '\\nCLOSE_INPUT_CONFIRMED\\n'\r"
                os.write(helper.master, text.encode())
                helper.until(acknowledgment.is_file, "close client original shell acknowledgement")
                require(acknowledgment.read_text() == str(owner["shell_pid"]), "Close input reached a different shell")
                require(same_process(api.process(server["pid"]), server), "Original server changed before Quit")
                require(not child_exited(helper.process), "Close client exited before Quit")
                require(any(record["client"]["pid"] == helper.process.pid
                            and same_process(record["server"], server) and record["forwarded_to_server"] > 0
                            for record in closer.records), "Close client has no authenticated live owner transport")
                observations["close_control"] = {"method": "retained_authenticated_terminal_quit",
                                                  "client": actual_child, "shell_pid": owner["shell_pid"],
                                                  "original_shell_acknowledged": True, "ready_before_quit": True}
                # Explicit fixture-only normal-mode binding. Unlike detach, Quit
                # ends this pin's server when the last active client leaves.
                closer.request_owner_shutdown()
                require(os.write(helper.master, b"\x11") == 1, "Quit input was not accepted by the PTY")
                observations["close_control"]["quit_input_written"] = True
                def close_completed():
                    helper.pump()
                    return original_gone()
                wait_until(close_completed, "original daemon exit through authenticated Quit", 12)
                wait_until(lambda: all(identity is not None and original_process_gone(api, identity)
                                       for identity in foreground.values()),
                           "original shell and wrapper exit after authenticated Quit", 5)
                observations["close_control"]["original_foreground_identities_gone"] = True
                wait_until(lambda: child_exited(helper.process), "close terminal child exit", 5)
                status = os.waitid(os.P_PID, helper.process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                require(status is not None and status.si_code == os.CLD_EXITED and status.si_status == 0,
                        "Authenticated Quit client failed")
                closer.confirm_owner_shutdown()
                observations["close_control"]["original_owner_and_unreaped_child_confirmed"] = True
            except Exception as error:
                errors.append("owner close: " + repr(error))
            finally:
                if helper is not None:
                    try:
                        helper.close()
                    except Exception as error:
                        errors.append("close client cleanup: " + repr(error))
                if closer is not None:
                    try:
                        closer.close()
                    except Exception as error:
                        errors.append("close broker cleanup: " + repr(error))
                if helper is not None:
                    (output / "pty-close.log").write_bytes(helper.output)
            try:
                wait_until(original_gone, "original daemon exit", 12)
            except Exception as error:
                errors.append("physical owner cleanup: " + repr(error))
            observations["close_helper"] = {
                "method": "retained_authenticated_terminal_quit",
                "exit_code": helper.process.returncode if helper is not None and helper.process is not None else None,
                "records": closer.records if closer is not None else [],
                "input_rejections": closer.input_rejections if closer is not None else [],
                "shutdown_confirmed": closer.shutdown_confirmed if closer is not None else False,
                "rejected": closer.rejected if closer is not None else [],
                "errors": closer.errors if closer is not None else [],
                "normal_closes": closer.closes if closer is not None else []}
            if errors:
                try:
                    observations["server_diagnostic"] = command(
                        ["/bin/ps", "-p", str(server["pid"]), "-o", "pid=,ppid=,pgid=,stat="], environment)
                except Exception as diagnostic:
                    observations["server_diagnostic_error"] = repr(diagnostic)
            foreground_exited = {}
            for role, identity in foreground.items():
                try:
                    foreground_exited[role] = identity is not None and original_process_gone(api, identity)
                except Exception as error:
                    foreground_exited[role] = False
                    errors.append(role + " exit inspection: " + repr(error))
                if not foreground_exited[role]:
                    errors.append("original " + role + " exit remains unconfirmed")
                    if identity is not None:
                        # Diagnostics distinguish a still-visible zombie from a
                        # running process; neither is silently called absent.
                        try:
                            probe = command(["/bin/ps", "-p", str(identity["pid"]), "-o", "pid=,stat="],
                                            environment, 2)
                            fields = probe["output"].split()
                            still_same = same_process(api.process(identity["pid"]), identity)
                            state = ("zombie" if len(fields) == 2 and fields[0] == str(identity["pid"])
                                     and fields[1].startswith("Z") and still_same else "present_or_changed")
                            observations[role + "_exit_diagnostic"] = {"state": state, "same_full_identity": still_same,
                                                                      "public_ps": probe}
                        except Exception as diagnostic:
                            observations[role + "_exit_diagnostic"] = {"error": repr(diagnostic)}
            observations["cleanup"] = {"original_server_exited": original_gone(),
                                       "original_shell_exited": foreground_exited["shell"],
                                       "original_wrapper_exited": foreground_exited["wrapper"], "errors": errors}
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
        receipt["credential_fixture"] = []
        credential_fixture(api, scratch, environment, receipt["credential_fixture"], output)
        receipt["process_containment"] = containment_fixture(api, scratch, environment)
        receipt["relay_half_close"] = {}
        relay_half_close_fixture(api, scratch, environment, receipt["relay_half_close"])
        receipt["relay_live_backend_rejection"] = {}
        relay_half_close_fixture(api, scratch, environment, receipt["relay_live_backend_rejection"],
                                backend_rejects_input=True)
        receipt["relay_unconfirmed_cancel_rejection"] = {}
        relay_half_close_fixture(api, scratch, environment, receipt["relay_unconfirmed_cancel_rejection"],
                                backend_rejects_input=True, requested_terminal_phase="attachment_cancel")
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
        if all(cleanup.get(key) is True for key in ("original_server_exited", "original_shell_exited", "original_wrapper_exited")) and not cleanup.get("errors"):
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
