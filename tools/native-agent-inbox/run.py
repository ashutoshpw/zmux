#!/usr/bin/env python3
"""Run the native Agent Inbox against real CLI persistence and independent PTYs."""
import argparse
import ast
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import uuid

REPO = Path(__file__).resolve().parents[2]
OWN_INPUTS = ('tools/native-agent-inbox/run.py', '.github/workflows/macos-development.yml',
              'scripts/check-macos-sources', 'scripts/check-native-bridge')
LAUNCHER = '''import json,os,sys
with open(sys.argv[1], 'x') as record:
 json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'executable':sys.argv[2]},record)
os.execv(sys.argv[2],sys.argv[2:])
'''
CHILD = '''#!/usr/bin/env python3
import json,os,pathlib,select,time,tty
root=pathlib.Path(os.environ['ZMUX_AGENT_INBOX_ROOT'])
nonce=os.environ['ZMUX_AGENT_INBOX_NONCE']
pid=os.getpid(); total=0; sequence=0; deadline=time.monotonic()+100
tty.setraw(0)
record={'pid':pid,'parentPID':os.getppid(),'nonce':nonce,
        'workspace':os.environ['ZMUX_WORKSPACE_ID'],
        'surface':os.environ['ZMUX_SURFACE_ID'],'inputBytes':total,'sequence':sequence}
path=root/('child-'+str(pid)+'.json')
def save():
 temporary=path.with_suffix('.tmp')
 temporary.write_text(json.dumps(dict(record,inputBytes=total,sequence=sequence)))
 temporary.chmod(0o600); temporary.replace(path)
save()
os.write(1,('AGENT_INBOX_READY_'+nonce+'_'+str(pid)+'\\r\\n').encode())
while time.monotonic()<deadline:
 ready,_,_=select.select([0],[],[],min(.05,max(0,deadline-time.monotonic())))
 if ready:
  data=os.read(0,4096)
  if not data:break
  total+=len(data)
 sequence+=1;save()
'''
LOCK_HOLDER = '''import fcntl,json,os,pathlib,sys,time
root=pathlib.Path(sys.argv[1]); profile=pathlib.Path(sys.argv[2]); deadline=time.monotonic()+100
record={'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':os.environ['ZMUX_AGENT_INBOX_NONCE']}
(root/'lock-holder.json').write_text(json.dumps(record))
while not (root/'lock-request').exists():
 if time.monotonic()>=deadline:raise RuntimeError('Lock request deadline expired')
 time.sleep(.01)
with (profile/'agent-messages.lock').open('r+') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 (root/'lock-held').write_text(json.dumps(record))
 while not (root/'lock-release').exists():
  if time.monotonic()>=deadline:raise RuntimeError('Lock release deadline expired')
  time.sleep(.01)
 fcntl.flock(lock,fcntl.LOCK_UN)
(root/'lock-ended').write_text(json.dumps(record))
'''
PREREQUISITES = ['two_real_window_ptys_and_exact_cli_queued_global_closed_records',
                 'independent_pty_input_counters_zero',
                 'other_window_key_before_explicit_source_menu']
SUCCESS_CHECKS = PREREQUISITES + [
    'actual_source_owned_inbox_exact_body_and_reply_responder',
    'draft_survives_real_arrival_refresh_hide_and_foreign_focus',
    'button_and_submit_key_save_one_exact_reply_and_keep_later_edit',
    'explicit_read_leaves_later_arrival_queued',
    'native_receiving_settings_refuse_send_preserve_draft_and_never_resurrect',
    'status_transport_capacity_refusal_preserves_original_uncertain_request',
    'real_journal_lock_uncertainty_retains_uuid_and_check_status_saves_once',
    'independent_pty_progress_after_all_ui_actions_zero_input']


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_report(result, owned, source, nonce, root, native, executable):
    if (result['sourceSHA'] != source or result['nonce'] != nonce
            or result['scope'] != 'native_agent_inbox_menu_and_durable_messages'
            or result['appPID'] != owned['pid'] or owned['parent_pid'] != os.getpid()
            or owned['executable'] != executable):
        raise RuntimeError('Native inbox app/source/process identity differs')
    children = result['children']
    if (len(children) != 3 or len({row['pid'] for row in children}) != 3
            or len({row['surface'] for row in children}) != 3
            or len(set(result['messageIDs'])) != 3
            or result['sourceWindow'] == result['otherWindow']):
        raise RuntimeError('Native inbox did not observe distinct real window/message/PTY owners')
    for row in children:
        observed = native.document(root / ('child-' + str(row['pid']) + '.json'), maximum=2048)
        if (any(observed[key] != value for key, value in row.items() if key not in ('sequence', 'workspace', 'surface'))
                or any(uuid.UUID(observed[key]) != uuid.UUID(row[key]) for key in ('workspace', 'surface'))
                or observed['sequence'] < row['sequence'] or row['parentPID'] != owned['pid']
                or row['nonce'] != nonce or row['inputBytes'] != 0):
            raise RuntimeError('Independent native inbox PTY input/identity differs')
    if result.get('activationPID') != owned['pid'] or result['checks'][:3] != PREREQUISITES:
        raise RuntimeError('Native inbox behavioral prerequisites did not pass')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check-inputs', action='store_true')
    args = parser.parse_args()
    native = load('agent_inbox_native_inputs', REPO / 'tools/native-idle-workloads/run.py')

    def sources():
        frozen = native.source_files()
        for name in OWN_INPUTS:
            path = REPO / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size > 131_072:
                raise RuntimeError('Native inbox input exceeds type/byte bound')
            frozen[name] = native.digest(path)
        if len(frozen) > 512:
            raise RuntimeError('Native inbox retained source inventory exceeds bound')
        return frozen

    frozen = sources()
    ast.parse(CHILD); ast.parse(LAUNCHER); ast.parse(LOCK_HOLDER)
    if args.check_inputs:
        print(json.dumps({'sources': len(frozen), 'actual_native_execution': False,
                          'app_seconds': 75, 'independent_child_seconds': 100,
                          'native_case': 'exact_source_menu_with_other_window_key'}))
        return 0
    if platform.system() != 'Darwin' or platform.machine() != 'arm64' or args.output is None:
        parser.error('Native inbox execution requires Darwin ARM64 and --output')
    root = args.output.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    source = subprocess.check_output(['git', '-C', str(REPO), 'rev-parse', 'HEAD'], text=True).strip()
    nonce = str(uuid.uuid4())
    receipt = {'schema_version': 1, 'state': 'starting', 'source_sha': source,
               'source_sha256': frozen, 'phases': [], 'behavioral_red': False,
               'external_accessibility_input_verified': False}
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(('ZMUX_', 'DYLD_')) and key not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')}
    environment.update(CARGO_TARGET_DIR=str(REPO / 'target/agent-validation' / ('agent-inbox-' + nonce) / 'cargo'),
                       ZMUX_AGENT_INBOX_ROOT=str(root), ZMUX_AGENT_INBOX_NONCE=nonce,
                       ZMUX_SOCKET=str(root / 'control.socket'), ZMUX_SOCKET_PATH=str(root / 'control.socket'))
    runner = load('agent_inbox_owned_process_runner', REPO / 'tools/terminal-feasibility/run.py')
    try:
        for name, wanted in frozen.items():
            target = root / 'source' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, target)
            if native.digest(target) != wanted:
                raise RuntimeError('Native inbox retained source differs')
        runner.run(['scripts/check-macos-sources'], 'swift-source-check', root, receipt['phases'],
                   timeout=120, cwd=REPO, env=environment)
        runner.run([sys.executable, 'scripts/build-macos-dev', '--architecture', 'host'],
                   'build', root, receipt['phases'], timeout=600, cwd=REPO, env=environment)
        info = native.document(REPO / 'dist/build-info.json')
        if info['source_sha'] != source or info['optional_ghostty_linked'] or info['default_terminal_engine'] != 'vt100':
            raise RuntimeError('Native inbox build identity/default differs')
        archive = REPO / 'dist' / ('zmux-dev-arm64-' + source[:7] + '.zip')
        if archive.stat().st_size > 64 * 1024 * 1024:
            raise RuntimeError('Native inbox archive exceeds bound')
        shutil.copy2(archive, root / archive.name)
        receipt.update(build_info=info, archive={'name': archive.name, 'sha256': native.digest(archive)})
        child = root / 'inbox-child.py'; child.write_text(CHILD); child.chmod(0o700)
        environment['SHELL'] = str(child)
        profile = root / 'profile'; profile.mkdir(mode=0o700)
        launcher = root / 'owned-app-launch.py'; launcher.write_text(LAUNCHER)
        report = root / 'native-agent-inbox.json'
        executable = str(REPO / 'dist/zmux.app/Contents/MacOS/zmux-app')
        lock_script = root / 'lock-holder.py'; lock_script.write_text(LOCK_HOLDER)
        lock_log = (root / 'lock-holder.log').open('wb')
        lock_group = runner.ProcessGroup([sys.executable, str(lock_script), str(root), str(profile)],
                                         cwd=REPO, env=environment, stdin=subprocess.DEVNULL,
                                         stdout=lock_log, stderr=subprocess.STDOUT)
        app_failure = None
        try:
            runner.run([sys.executable, str(launcher), str(root / 'owned-launch.json'), executable,
                        '--agent-inbox-smoke', str(report), str(profile)], 'native-agent-inbox', root,
                       receipt['phases'], timeout=75, cwd=REPO, env=environment)
        except RuntimeError as error:
            app_failure = str(error)
        finally:
            try:
                lock_code = lock_group.close(timeout=5, retry=True)
                receipt['lock_holder'] = {'pid': lock_group.process.pid, 'exit_code': lock_code,
                                          'requested': (root / 'lock-request').exists()}
            finally:
                lock_log.close()
        if report.exists():
            result = native.document(report)
            owned = native.document(root / 'owned-launch.json')
            receipt.update(native_report=result, owned_launch=owned)
            verify_report(result, owned, source, nonce, root, native, executable)
            receipt['behavioral_red'] = (result['state'] == 'failed' and result['stage'] == 'entrypoint'
                                         and result.get('failure') == 'Native agent inbox: Agent Inbox entrypoint unavailable'
                                         and result['checks'] == PREREQUISITES
                                         and receipt['phases'][-1]['exit_code'] == 1
                                         and not receipt['phases'][-1]['timed_out'])
        if app_failure is not None:
            raise RuntimeError(app_failure)
        if (not report.exists() or result['state'] != 'complete' or result['stage'] != 'complete'
                or result['checks'] != SUCCESS_CHECKS or not (root / 'lock-ended').exists()):
            raise RuntimeError('Native inbox did not complete its actual UI case and shared shutdown')
        lock_record = native.document(root / 'lock-ended', maximum=2048)
        if (lock_record['pid'] != lock_group.process.pid or lock_record['parent_pid'] != os.getpid()
                or lock_record['nonce'] != nonce):
            raise RuntimeError('Independent real journal lock holder identity differs')
        receipt['lock_holder']['completed'] = lock_record
        receipt['state'] = 'complete'
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        if sources() != frozen:
            receipt.update(state='failed', behavioral_red=False, error='Native inbox source changed during run')
        if any(phase['log_bytes_seen'] != phase['log_bytes_retained'] for phase in receipt['phases']):
            receipt.update(state='failed', behavioral_red=False, error='Native inbox logs exceeded retained byte bound')
        native.save(root / 'receipt.json', receipt)
    print(json.dumps({'state': receipt['state'], 'behavioral_red': receipt['behavioral_red'], 'receipt': str(root / 'receipt.json')}))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
