"""Private loopback OpenSSH fixture for the packaged native smoke journey.

Uses generated keys, an explicit sshd config and an unprivileged port. It never
touches the user's SSH daemon, authorized_keys, known_hosts or SSH configuration.
"""
from contextlib import contextmanager
import getpass
import json
import os
import signal
from pathlib import Path
import shlex
import shutil
import socket
import subprocess
import tempfile
import time


def stop_fixture_processes(daemon):
    """Freeze this fixture's process tree before closing listener/session processes.

    sshd creates separate session groups, so terminating only the listener or its
    process group does not close authenticated clients. Stopped ancestors retain
    descendant ownership until the full tree has been discovered.
    """
    if daemon.poll() is not None:
        return
    stopped = set()
    stop_order = []
    try:
        os.kill(daemon.pid, signal.SIGSTOP)
        stopped.add(daemon.pid)
        stop_order.append(daemon.pid)
        for _ in range(32):
            output = subprocess.run(["ps", "-axo", "pid=,ppid="], check=True,
                                    capture_output=True, text=True, timeout=5).stdout
            parents = {int(pid): int(parent) for pid, parent in
                       (line.split() for line in output.splitlines() if line.strip())}
            # Freeze one generation at a time. A stopped parent cannot reap a
            # child and allow its PID to be recycled between discovery and STOP.
            pending = {pid for pid, parent in parents.items() if parent in stopped} - stopped
            if not pending:
                break
            for pid in pending:
                try:
                    os.kill(pid, signal.SIGSTOP)
                    stopped.add(pid)
                    stop_order.append(pid)
                except ProcessLookupError:
                    pass
        else:
            raise RuntimeError("Private SSH fixture process tree did not quiesce")
    finally:
        # Every PID here was a live descendant of our still-stopped listener.
        # Kill children before their ancestors; SIGKILL also acts on stopped tasks.
        for pid in reversed(stop_order[1:]):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        if daemon.poll() is None:
            daemon.kill()
        daemon.wait(timeout=5)


@contextmanager
def native_ssh_fixture(configuration, diagnostic, *, session_environment=None,
                       scheduler_rpc_request_log=None,
                       drop_scheduler_rpc_response_after_execution=False):
    root = Path(tempfile.mkdtemp(prefix="zns-", dir="/tmp"))
    root.chmod(0o700)
    daemon = None
    log = None
    try:
        for name in ["host", "client key"]:
            subprocess.run(["/usr/bin/ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(root / name)], check=True, capture_output=True, timeout=15)
        shutil.copyfile(root / "client key.pub", root / "authorized_keys")
        (root / "authorized_keys").chmod(0o600)
        uploads = root / "uploads '画像"
        uploads.mkdir(mode=0o700)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        quoted = shlex.quote(str(root))
        session_environment = session_environment or {}
        if set(session_environment) - {'HOME', 'SHELL', 'PATH', 'ZMUX_SCHEDULER_ROOT', 'ZMUX_SCHEDULER_TMUX_SOCKET'}:
            raise ValueError('Unsupported private SSH fixture environment')
        exports = ''.join('export ' + key + '=' + shlex.quote(str(value)) + '\n'
                          for key, value in session_environment.items())
        scheduler_rpc_capture = ''
        scheduler_request_log = None
        scheduler_drop_marker = None
        if scheduler_rpc_request_log is not None:
            if scheduler_rpc_request_log is True:
                scheduler_request_log = root / "scheduler-rpc-requests.jsonl"
            else:
                scheduler_request_log = Path(scheduler_rpc_request_log).resolve()
            scheduler_request_log.touch(mode=0o600, exist_ok=False)
            scheduler_request_log.chmod(0o600)
            request_log = shlex.quote(str(scheduler_request_log))
            scheduler_drop = ''
            if drop_scheduler_rpc_response_after_execution:
                scheduler_drop_marker = root / "drop-next-scheduler-rpc-response"
                quoted_marker = shlex.quote(str(scheduler_drop_marker))
                request_template = shlex.quote(str(root / ".scheduler-request.XXXXXX"))
                response_template = shlex.quote(str(root / ".scheduler-response.XXXXXX"))
                scheduler_drop = f'''if [ -e {quoted_marker} ]; then
    request_file=$(/usr/bin/mktemp {request_template}) || exit 76
    response_file=$(/usr/bin/mktemp {response_template}) || exit 76
    /bin/cat > "$request_file" || exit 76
    /bin/cat "$request_file" >> {request_log} || exit 76
    /bin/sh -c "$SSH_ORIGINAL_COMMAND" < "$request_file" > "$response_file" 2>/dev/null
    result=$?
    /usr/bin/printf '\\n' >> {request_log}
    /bin/rm -f -- "$request_file" "$response_file" {quoted_marker}
    /bin/kill -KILL "$$"
    exit 77
  fi
'''
            scheduler_rpc_capture = f'''  *zmux-scheduler*rpc*)
    {scheduler_drop}    /usr/bin/tee -a {request_log} | /bin/sh -c "$SSH_ORIGINAL_COMMAND"
    result=$?
    /usr/bin/printf '\\n' >> {request_log}
    exit "$result"
  ;;
'''
        shell = root / "shell"
        shell.write_text(f'''#!/bin/sh
cd {quoted} || exit 79
export TMPDIR={shlex.quote(str(uploads))}
{exports}
case "$SSH_ORIGINAL_COMMAND" in
{scheduler_rpc_capture}  *zmux-upload*)
    /bin/sh -c "$SSH_ORIGINAL_COMMAND" > {quoted}/ack
    result=$?
    [ "$result" -eq 0 ] || exit "$result"
    touch {quoted}/ack-ready
    # Hold the real worker open until native acceptance observes its source.
    attempts=0
    while [ ! -f {quoted}/release-ack ]; do
      attempts=$((attempts+1))
      [ "$attempts" -lt 400 ] || exit 78
      sleep 0.05
    done
    cat {quoted}/ack
    exit "$result"
  ;;
esac
if [ -n "$SSH_ORIGINAL_COMMAND" ]; then exec /bin/sh -c "$SSH_ORIGINAL_COMMAND"; else exec /bin/sh -i; fi
''')
        shell.chmod(0o700)
        server = root / "server"
        server.write_text(f'''Port {port}
ListenAddress 127.0.0.1
HostKey {root}/host
AuthorizedKeysFile {root}/authorized_keys
PidFile {root}/pid
UsePAM no
PasswordAuthentication no
KbdInteractiveAuthentication no
StrictModes no
LogLevel VERBOSE
PrintMotd no
PrintLastLog no
ForceCommand {shell}
''')
        client = root / "client"
        client.write_text(f'''Host fixture
 HostName 127.0.0.1
 Port {port}
 User {getpass.getuser()}
 UserKnownHostsFile {root}/known
 GlobalKnownHostsFile /dev/null
 StrictHostKeyChecking accept-new
 BatchMode yes
 IdentitiesOnly yes
 ConnectTimeout 2
 ConnectionAttempts 1
''')
        log = diagnostic.open("w")
        subprocess.run(["/usr/sbin/sshd", "-t", "-f", str(server)], check=True, stdout=log, stderr=log, timeout=10)
        daemon = subprocess.Popen(["/usr/sbin/sshd", "-D", "-e", "-f", str(server)], stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        deadline = time.monotonic() + 5
        while True:
            if daemon.poll() is not None:
                raise RuntimeError(f"Private fixture sshd exited ({daemon.returncode}); see {diagnostic}")
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("Private fixture sshd did not listen on loopback")
                time.sleep(0.02)
        config = {"root": str(root), "clientConfig": str(client), "identity": str(root / "client key"), "uploads": str(uploads)}
        if scheduler_request_log is not None:
            config["schedulerRpcRequestLog"] = str(scheduler_request_log)
        if scheduler_drop_marker is not None:
            config["schedulerRpcDropResponseMarker"] = str(scheduler_drop_marker)
        configuration.write_text(json.dumps(config))
        configuration.chmod(0o600)
        yield
    finally:
        if daemon is not None:
            stop_fixture_processes(daemon)
        if log is not None:
            log.close()
        configuration.unlink(missing_ok=True)
        shutil.rmtree(root)
