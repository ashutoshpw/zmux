#!/usr/bin/env python3
"""Observe real owned-child waits and PTY cleanup without changing Session policy."""
import errno
import json
import os
from pathlib import Path
import platform
import pty
import signal
import subprocess
import sys
import tempfile
import time


def observed(pid):
    result = {}
    try:
        value = os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        result['waitid'] = None if value is None else dict(pid=value.si_pid, code=value.si_code, status=value.si_status)
    except AttributeError as error:
        result['waitid_unavailable'] = str(error)
    except OSError as error:
        result['waitid_error'] = error.errno
    try:
        result['pgid'] = os.getpgid(pid)
    except OSError as error:
        result['getpgid_error'] = error.errno
    try:
        result['waitpid'] = list(os.waitpid(pid, os.WNOHANG))
    except OSError as error:
        result['waitpid_error'] = error.errno
    return result


def exited(sample, pid):
    return sample.get('waitpid', [0])[0] == pid or sample.get('waitpid_error') == errno.ECHILD


def observe_until_exit(child, seconds=1):
    samples = []
    deadline = time.monotonic() + seconds
    while True:
        sample = observed(child.pid)
        samples.append(sample)
        if exited(sample, child.pid):
            return samples, True
        if time.monotonic() >= deadline:
            return samples, False
        time.sleep(.02)


def direct_cleanup(child, done):
    if not done:
        # No reap occurred, so this physical child's PID is still retained.
        child.kill()
        child.wait(timeout=2)


def ignored_case():
    child = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(.05)'],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, start_new_session=True)
    done = False
    try:
        samples, done = observe_until_exit(child)
        return dict(pid=child.pid, sigchld_disposition=int(signal.getsignal(signal.SIGCHLD)),
                    samples=samples, terminal_observation=done)
    finally:
        if not done:
            # SIG_IGN may release the PID automatically. Never signal its
            # numeric identity; this fixed finite child can only be waited for.
            child.wait(timeout=2)


def flood_case():
    if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        raise RuntimeError('flood probe requires default SIGCHLD disposition to retain its child PID')
    with tempfile.TemporaryDirectory(prefix='zmux-exit-probe-') as temporary:
        marker = Path(temporary) / 'ready'
        master, slave = pty.openpty()
        duplicate = os.dup(master)
        child = None
        done = False
        try:
            script = ('import fcntl,os,pathlib,signal,sys,termios;'
                      'fcntl.ioctl(0,termios.TIOCSCTTY,0);'
                      'signal.signal(signal.SIGTERM,signal.SIG_IGN);'
                      'pathlib.Path(sys.argv[1]).write_text("ready");'
                      '\nwhile True:os.write(1,b"x"*4096)')
            child = subprocess.Popen([sys.executable, '-c', script, str(marker)],
                                     stdin=slave, stdout=slave, stderr=slave,
                                     start_new_session=True, close_fds=True)
            os.close(slave)
            slave = -1
            deadline = time.monotonic() + 2
            while not marker.exists():
                if time.monotonic() >= deadline:
                    raise RuntimeError('actual flood child did not become ready')
                time.sleep(.005)
            os.set_blocking(master, False)
            read_bytes = 0
            read_would_block = False
            try:
                read_bytes = len(os.read(master, 4096))
            except BlockingIOError:
                read_would_block = True
            os.killpg(child.pid, signal.SIGTERM)
            time.sleep(.1)
            after_term = observed(child.pid)
            done = exited(after_term, child.pid)
            kill_result = 'not_attempted_after_terminal_observation'
            if not done:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                    kill_result = 'sent'
                except OSError as error:
                    kill_result = dict(errno=error.errno)
            os.close(master)
            master = -1
            os.close(duplicate)
            duplicate = -1
            samples = []
            if not done:
                samples, done = observe_until_exit(child)
            return dict(pid=child.pid, initial_read_bytes=read_bytes,
                        initial_read_would_block=read_would_block, after_term=after_term,
                        kill=kill_result, both_parent_masters_closed=True,
                        after_close=samples, terminal_observation=done)
        finally:
            for fd in (master, slave, duplicate):
                if fd >= 0:
                    os.close(fd)
            if child is not None:
                direct_cleanup(child, done)


if __name__ == '__main__':
    if sys.argv[1:] == ['--install-ignore']:
        signal.signal(signal.SIGCHLD, signal.SIG_IGN)
        os.execv(sys.executable, [sys.executable, str(Path(__file__).resolve()), '--ignored-exec'])
    elif sys.argv[1:] == ['--ignored-exec']:
        print(json.dumps(ignored_case()))
    else:
        ignored = subprocess.run([sys.executable, str(Path(__file__).resolve()), '--install-ignore'],
                                 capture_output=True, text=True, timeout=5, check=True)
        print(json.dumps(dict(system=platform.system(), release=platform.release(),
                              waitid_api='Python os.waitid, not the Session rustix wait ABI',
                              ignored_sigchld_after_exec=json.loads(ignored.stdout),
                              output_flood_cleanup=flood_case(), acceptance_claimed=False), indent=2))
