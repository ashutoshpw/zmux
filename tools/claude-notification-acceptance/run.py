#!/usr/bin/env python3
"""Authenticated SSH provider-notification journey through the actual Rust ABI.

Uses a finite controlled provider that executes Claude's injected command-hook
contract. Never invokes Claude's account/API or reads user provider configuration.
This proves real SSH/PTY/inbox routing, not that macOS displayed a banner.
"""
import argparse
import ctypes as c
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import shlex
import sys
from itertools import islice

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('zmux_ssh_fixture', REPO / 'scripts/native-ssh-fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)

class Result(c.Structure):
    _fields_ = [('status', c.c_int32), ('value', c.c_uint64)]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--via-tmux', action='store_true', help='Exercise remote multiplexer PTY routing')
    args = parser.parse_args()
    library = c.CDLL(str(args.library.resolve(strict=True)))
    signatures = [
        ('zmux_desktop_create', [], Result),
        ('zmux_desktop_snapshot', [c.c_uint64, c.c_uint64], Result),
        ('zmux_desktop_dispatch', [c.c_uint64, c.c_char_p, c.c_size_t], Result),
        ('zmux_buffer_length', [c.c_uint64], Result),
        ('zmux_buffer_copy', [c.c_uint64, c.c_void_p, c.c_size_t], c.c_int32),
        ('zmux_buffer_release', [c.c_uint64], c.c_int32),
        ('zmux_desktop_destroy', [c.c_uint64], c.c_int32),
    ]
    for name, inputs, output in signatures:
        operation = getattr(library, name)
        operation.argtypes, operation.restype = inputs, output

    def consume(token):
        try:
            length = library.zmux_buffer_length(token)
            assert length.status == 0 and length.value <= 8_388_608
            data = c.create_string_buffer(length.value)
            assert library.zmux_buffer_copy(token, data, length.value) == 0
            return json.loads(data.raw)
        finally:
            assert library.zmux_buffer_release(token) == 0

    with tempfile.TemporaryDirectory(prefix='zmux-claude-acceptance-') as temporary:
        root = Path(temporary)
        home = root / 'home'
        home.mkdir(mode=0o700)
        provider = root / 'claude'
        provider.write_text('''#!/usr/bin/python3
import json,os,pathlib,signal,subprocess,sys,time
args=sys.argv[1:]
assert args[-1]=='literal $HOME ; text',args
plugins=[args[i+1] for i,arg in enumerate(args[:-1]) if arg=='--plugin-dir']
assert len(plugins)==1,args
pathlib.Path(os.environ['HOME'],'plugin-path').write_text(plugins[0])
pathlib.Path(os.environ['HOME'],'provider-process.json').write_text(json.dumps({
 'pid':os.getpid(),'parent_pid':os.getppid(),'process_group':os.getpgrp(),
 'session':os.getsid(0),'tty_foreground_group':os.tcgetpgrp(0),
 'hup_ignored':signal.getsignal(signal.SIGHUP)==signal.SIG_IGN,
 'int_ignored':signal.getsignal(signal.SIGINT)==signal.SIG_IGN}))
for entry in json.loads((pathlib.Path(plugins[0])/'hooks/hooks.json').read_text())['hooks']['Stop']:
 for hook in entry['hooks']:
  subprocess.run(hook['command'],shell=True,input=b'{"hook_event_name":"Stop","session_id":"remote-fixture"}',check=True,timeout=3)
print('REMOTE_PROVIDER_STOPPED_BUT_ALIVE',flush=True)
# Finite even if the driver fails, without needing an account or API call.
time.sleep(30)
''')
        provider.chmod(0o700)
        tmux_socket = root / 'tmux.sock'
        (home / '.bashrc').write_text("export PATH=" + str(root) + ':"$PATH"\nPS1=\nTMOUT=35\nprintf REMOTE_SHELL_BOOT_READY\\\\n\n')
        (home / '.bash_profile').write_text('. "$HOME/.bashrc"\n')
        environment = {key: value for key, value in os.environ.items() if not key.startswith(('ZMUX_', 'CMUX_'))}
        environment.update(HOME=str(home), SHELL='/bin/bash', ZMUX_SOCKET=str(root / 'control.sock'))
        # The ctypes host, not any user's app, owns these process-local variables.
        for key in list(os.environ):
            if key.startswith(('ZMUX_', 'CMUX_')):
                del os.environ[key]
        os.environ.update(HOME=str(home), SHELL='/bin/bash', ZMUX_SOCKET=str(root / 'control.sock'))
        desktop = library.zmux_desktop_create()
        assert desktop.status == 0
        request_id = 0
        latest = {}

        def wait(predicate):
            nonlocal latest
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                result = library.zmux_desktop_snapshot(desktop.value, 0)
                if result.status == -4 or (result.status == 0 and result.value == 0):
                    time.sleep(.02)
                    continue
                assert result.status == 0 and result.value
                latest = consume(result.value)
                if predicate(latest):
                    return latest
                time.sleep(.02)
            raise RuntimeError('SSH notification journey timed out; last_error=' + str(latest.get('last_error')))

        def dispatch(payload):
            nonlocal request_id
            request_id += 1
            encoded = json.dumps(dict(version=1, request_id=request_id, payload=payload)).encode()
            admitted = library.zmux_desktop_dispatch(desktop.value, encoded, len(encoded))
            assert admitted.status == 0 and consume(admitted.value)['ok']
            return wait(lambda value: value['last_request_id'] == request_id)

        def cli(*arguments):
            result = subprocess.run([str(args.cli.resolve()), '--socket', str(root / 'control.sock'), *arguments],
                                    env=environment, stdin=subprocess.DEVNULL,
                                    capture_output=True, text=True, timeout=10)
            if result.returncode != 0:
                raise RuntimeError('Acceptance CLI failed: ' + result.stderr[-1000:])
            return json.loads(result.stdout)

        try:
            initial = wait(lambda s: bool(s['frames']) and s['frames'][0].get('snapshot') is not None)
            original_workspace = initial['model']['focused_workspace']
            with fixture.native_ssh_fixture(root / 'ssh.json', root / 'sshd.log',
                    session_environment={'HOME': str(home), 'SHELL': '/bin/bash'}):
                configuration = json.loads((root / 'ssh.json').read_text())
                cli('ssh', 'fixture', '--ssh-config', configuration['clientConfig'],
                    '--identity', configuration['identity'], '--name', 'Claude SSH acceptance')
                remote = wait(lambda s: s['model']['focused_workspace'] != original_workspace
                              and any(f.get('snapshot') and 'REMOTE_SHELL_BOOT_READY' in f['snapshot']['viewport']['contents'] for f in s['frames']))
                session = remote['frames'][0]['session_id']
                workspace = remote['model']['focused_workspace']
                owner = next(w for w in remote['model']['workspaces'] if w['id'] == workspace)['root']['pane']['selected_surface']
                dispatch({'command':'attention', 'action':{'operation':'app_active','active':False}})
                invocation = "claude 'literal $HOME ; text'"
                if args.via_tmux:
                    invocation = ('tmux -S ' + shlex.quote(str(tmux_socket))
                                  + ' -f /dev/null new-session /bin/bash -c ' + shlex.quote(invocation))
                dispatch({'command':'input', 'session_id':session, 'text':invocation+'\n'})
                stopped = wait(lambda s: any(f.get('snapshot') and 'REMOTE_PROVIDER_STOPPED_BUT_ALIVE' in f['snapshot']['viewport']['contents'] for f in s['frames']))
                notified = wait(lambda s: len(s['notifications']) == 1)
                notice = notified['notifications'][0]
                assert notice['title'] == 'Claude' and notice['body'] == 'Claude finished its turn'
                assert notice['workspace_id'] == workspace and notice['surface_id'] == owner and not notice['read']
                assert stopped['frames'][0]['snapshot']['state'] == 'running'
                plugin = Path((home / 'plugin-path').read_text())
                assert plugin.is_dir()
                # Closing the actual remote surface must not retarget its notice;
                # its session-scoped hook files must be cleaned by remote hangup.
                cli('close-workspace', str(workspace))
                deadline = time.monotonic() + 10
                while plugin.exists() and time.monotonic() < deadline:
                    time.sleep(.05)
                cleaned = not plugin.exists()
                if not cleaned:
                    # Only this finite fixture's known process IDs and private
                    # lease filenames; no command arguments or file contents.
                    identity = json.loads((home / 'provider-process.json').read_text())
                    ids = {int(value) for key, value in identity.items()
                           if key not in ('hup_ignored', 'int_ignored') and int(value) > 1}
                    local_ssh = remote['frames'][0].get('process_id')
                    if local_ssh:
                        ids.add(int(local_ssh))
                    states = subprocess.run(['ps','-p',','.join(map(str,sorted(ids))),
                                             '-o','pid=,ppid=,pgid=,stat=,comm='],
                                            capture_output=True,text=True,timeout=3,check=False)
                    masks = subprocess.run(['ps','-p',','.join(map(str,sorted(ids))),
                                            '-o','pid=,pending=,ignored=,caught='],
                                           capture_output=True,text=True,timeout=3,check=False)
                    result = library.zmux_desktop_snapshot(desktop.value, 0)
                    runtime = {'status':result.status}
                    if result.status == 0 and result.value:
                        state = consume(result.value)
                        runtime.update({key:state.get(key) for key in
                                        ('cleanup_error','last_error','pending_launches')})
                    print(json.dumps({'fixture_cleanup_failure':True,'provider':identity,
                                      'local_ssh_pid':local_ssh,
                                      'lease_path':str(plugin.parent),
                                      'lease_entries':[p.relative_to(plugin.parent).as_posix()
                                                       for p in islice(plugin.parent.rglob('*'),32)],
                                      'owned_process_states':states.stdout[:4000],
                                      'owned_signal_masks':masks.stdout[:4000],
                                      'signal_probe_error':masks.stderr[:500],
                                      'desktop_cleanup':runtime,
                                      'process_probe_error':states.stderr[:500]}),file=sys.stderr)
                assert cleaned, 'Remote session hook directory survived SSH close'
                print(json.dumps({'authenticated_ssh':True, 'plain_claude_plugin':True,
                                  'via_tmux':args.via_tmux,
                                  'completion_while_provider_alive':True, 'exact_surface_attention':True,
                                  'remote_hook_cleanup':True, 'actual_claude_api':False,
                                  'os_banner_observed':False}))
        finally:
            if args.via_tmux and tmux_socket.exists():
                subprocess.run(['tmux','-S',str(tmux_socket),'kill-server'],
                               stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=5,check=False)
            assert library.zmux_desktop_destroy(desktop.value) == 0

if __name__ == '__main__':
    main()
