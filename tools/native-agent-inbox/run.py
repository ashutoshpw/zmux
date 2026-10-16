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
LOCK_HOLDER = '''import fcntl,json,os,pathlib,socket,struct,sys,time,uuid
root=pathlib.Path(sys.argv[1]); profile=pathlib.Path(sys.argv[2]); deadline=time.monotonic()+100
record={'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':os.environ['ZMUX_AGENT_INBOX_NONCE']}
(root/'lock-holder.json').write_text(json.dumps(record))
def await_file(name):
 while not (root/name).exists():
  if time.monotonic()>=deadline:raise RuntimeError('Independent owner deadline: '+name)
  time.sleep(.01)
def save(name,value):
 path=root/name; temporary=path.with_suffix('.tmp'); temporary.write_text(json.dumps(value));temporary.replace(path)
def exact(connection,count):
 data=b''
 while len(data)<count:
  chunk=connection.recv(count-len(data))
  if not chunk:raise RuntimeError('Public control response truncated')
  data+=chunk
 return data
wire_id=0
def rpc(command):
 global wire_id
 wire_id+=1
 owned=json.loads((root/'owned-launch.json').read_text())
 with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as connection:
  connection.settimeout(3);connection.connect(os.environ['ZMUX_SOCKET_PATH'])
  if sys.platform=='darwin':peer=struct.unpack('i',connection.getsockopt(0,2,4))[0]
  else:peer=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))[0]
  if peer!=owned['pid']:raise RuntimeError('Public control peer is not the owned app')
  request={'version':2,'request_id':wire_id,'command':command}
  data=json.dumps(request).encode();connection.sendall(struct.pack('!I',len(data))+data)
  count=struct.unpack('!I',exact(connection,4))[0]
  if not 0<count<=1048576:raise RuntimeError('Public control response exceeds bound')
  response=json.loads(exact(connection,count))
 if response.get('version')!=2 or response.get('request_id')!=wire_id or response.get('ok') is not True:
  raise RuntimeError('Public control admission refused: '+json.dumps(response))
 return {'peerPID':peer,'request':request,'response':response}
def status(identifier):return rpc({'method':'agent_messages_status','request_id':identifier})
await_file('lock-request')
with (profile/'agent-messages.lock').open('r+') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX);save('lock-held',record)
 await_file('lock-release');fcntl.flock(lock,fcntl.LOCK_UN)
save('lock-ended',record)
await_file('pending-lock-request')
with (profile/'agent-messages.lock').open('r+') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX);save('pending-lock-held.json',record)
 controls=[]
 # Five real read admissions plus the native reply leave two of eight slots
 # for the controller's bounded list/settings queries. No mutation is replayed.
 for index in range(5):
  identifier=str(uuid.uuid4())
  observed=rpc({'method':'agent_messages','request':{'request_id':identifier,
   'caller':{},'action':{'action':'list','surface':None,'states':[],'limit':1,'cursor':None}}})
  receipt=observed['response']['result']
  if receipt.get('request_id')!=identifier or receipt.get('state')!='pending':
   raise RuntimeError('Independent queued read was not admitted pending')
  controls.append(observed)
 save('pending-controls-admitted.json',dict(record,controls=controls,ownedRequests=6,workerCapacity=8))
 await_file('pending-operation.json')
 operation=json.loads((root/'pending-operation.json').read_text())['requestId']
 original=status(operation)
 statuses=[status(row['response']['result']['request_id']) for row in controls]
 if original['response']['result'].get('request_id')!=operation or original['response']['result'].get('state')!='pending':
  raise RuntimeError('Original native operation was not publicly pending')
 if sum(row['response']['result'].get('state')=='pending' for row in statuses)<2:
  raise RuntimeError('Independent public queue controls did not retain pending headroom')
 save('pending-public-receipts.json',dict(record,requestId=operation,original=original,controls=statuses))
 await_file('pending-lock-release');fcntl.flock(lock,fcntl.LOCK_UN)
save('pending-lock-ended.json',record)
'''

PREREQUISITES = ['two_real_window_ptys_and_exact_cli_queued_global_closed_records',
                 'independent_pty_input_counters_zero',
                 'other_window_key_before_explicit_source_menu']
SUCCESS_CHECKS = PREREQUISITES + [
    'actual_source_owned_inbox_exact_body_and_reply_responder',
    'independent_cli_canonical_equivalent_bodies_have_distinct_utf8',
    'native_row_selection_preserves_distinct_canonical_equivalent_utf8_bodies',
    'draft_survives_real_arrival_refresh_hide_and_foreign_focus',
    'button_and_submit_key_save_one_exact_reply_and_keep_later_edit',
    'explicit_read_leaves_later_arrival_queued',
    'native_receiving_settings_refuse_send_preserve_draft_and_never_resurrect',
    'real_journal_lock_known_refusal_preserves_exact_draft',
    'public_owner_pending_original_and_queued_controls',
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
               'source_sha256': frozen, 'phases': [], 'behavioral_red': False, 'unicode_behavioral_red': False,
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
                   timeout=300, cwd=REPO, env=environment)
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
            receipt['unicode_behavioral_red'] = (
                result['state'] == 'failed' and result['stage'] == 'unicode_body'
                and result.get('failure') == 'Native agent inbox: Selected canonical-equivalent message did not preserve exact UTF-8 body'
                and result['checks'] == PREREQUISITES + [
                    'actual_source_owned_inbox_exact_body_and_reply_responder',
                    'independent_cli_canonical_equivalent_bodies_have_distinct_utf8']
                and receipt['phases'][-1]['exit_code'] == 1 and not receipt['phases'][-1]['timed_out'])
            if receipt['unicode_behavioral_red']:
                proofs = result.get('unicodeBytes', [])
                receipt['unicode_behavioral_red'] = (
                    [row['name'] for row in proofs] == ['body_decomposed', 'body_precomposed']
                    and proofs[0]['expected'] == proofs[0]['observed']
                    and proofs[1]['expected'] != proofs[1]['observed']
                    and proofs[1]['observed'] == proofs[0]['observed']
                    and proofs[0]['expected'] != proofs[1]['expected'])
        if app_failure is not None:
            raise RuntimeError(app_failure)
        if (not report.exists() or result['state'] != 'complete' or result['stage'] != 'complete'
                or result['checks'] != SUCCESS_CHECKS or not (root / 'lock-ended').exists()):
            raise RuntimeError('Native inbox did not complete its actual UI case and shared shutdown')
        proofs = result.get('unicodeBytes', [])
        if ([row['name'] for row in proofs] != ['body_decomposed', 'body_precomposed', 'editor_precomposed', 'saved_precomposed']
                or any(row['expected'] != row['observed'] for row in proofs)
                or proofs[0]['expected'] == proofs[1]['expected']):
            raise RuntimeError('Native mounted/editor/saved Unicode bytes differ from independent literals')
        known = result.get('knownLockRefusal', {})
        capacity = result.get('capacityRefusal', {})
        later_draft = proofs[2]['expected'] + list(b'edited while saving')
        if (not 0 <= result.get('knownStatusChecks', -1) <= 128
                or not 1 <= result.get('capacityStatusChecks', -1) <= 128
                or not 1 <= result.get('recoveryStatusChecks', -1) <= 128
                or known.get('operationStatus') != 'another instance is reserving session generations; retry'
                or known.get('sendEnabled') is not True or known.get('checkStatusEnabled') is not False
                or known.get('replyUTF8') != later_draft
                or capacity.get('sendEnabled') is not False or capacity.get('checkStatusEnabled') is not True
                or capacity.get('replyUTF8') != later_draft
                or '(-5)' not in capacity.get('operationStatus', '')
                or result['pendingRequestId'].lower() not in capacity.get('operationStatus', '')):
            raise RuntimeError('Mounted known refusal or uncertain capacity refusal contract differs')
        lock_record = native.document(root / 'lock-ended', maximum=2048)
        if (lock_record['pid'] != lock_group.process.pid or lock_record['parent_pid'] != os.getpid()
                or lock_record['nonce'] != nonce):
            raise RuntimeError('Independent real journal lock holder identity differs')
        pending_lock = native.document(root / 'pending-lock-ended.json', maximum=2048)
        admissions = native.document(root / 'pending-controls-admitted.json')
        public = native.document(root / 'pending-public-receipts.json')
        if (pending_lock != lock_record
                or any(public.get(key) != lock_record[key] or admissions.get(key) != lock_record[key] for key in lock_record)
                or admissions['ownedRequests'] != 6 or admissions['workerCapacity'] != 8
                or len(admissions['controls']) != 5 or len(public['controls']) != 5
                or uuid.UUID(public['requestId']) != uuid.UUID(result['pendingRequestId'])):
            raise RuntimeError('Independent pending owner/queue identity differs')
        identifiers = [row['response']['result']['request_id'] for row in admissions['controls']]
        if (len(set(identifiers)) != 5
                or any(row['response']['result']['state'] != 'pending' for row in admissions['controls'])
                or [row['response']['result']['request_id'] for row in public['controls']] != identifiers
                or public['original']['response']['result']['request_id'] != public['requestId']
                or public['original']['response']['result']['state'] != 'pending'
                or sum(row['response']['result']['state'] == 'pending' for row in public['controls']) < 2
                or any(row['peerPID'] != owned['pid'] for row in admissions['controls'] + public['controls'] + [public['original']])):
            raise RuntimeError('Public native pending/admission receipts do not establish the actual queue')
        receipt['lock_holder']['completed'] = lock_record
        receipt['lock_holder']['pending_completed'] = pending_lock
        receipt['state'] = 'complete'
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        if sources() != frozen:
            receipt.update(state='failed', behavioral_red=False, unicode_behavioral_red=False, error='Native inbox source changed during run')
        if any(phase['log_bytes_seen'] != phase['log_bytes_retained'] for phase in receipt['phases']):
            receipt.update(state='failed', behavioral_red=False, unicode_behavioral_red=False, error='Native inbox logs exceeded retained byte bound')
        native.save(root / 'receipt.json', receipt)
    print(json.dumps({'state': receipt['state'], 'behavioral_red': receipt['behavioral_red'], 'unicode_behavioral_red': receipt['unicode_behavioral_red'], 'receipt': str(root / 'receipt.json')}))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
