"""Actual owned relocated AppKit journey for the explicitly admitted readiness variant."""
import ast
import json
import os
from pathlib import Path
import shlex
import sys
import time
import uuid

from package_admission import (MAX_FILE, Rejected, digest, document, read_file,
                               safe_name, verify_payload)

LAUNCHER = '''import json,os,sys
with open(sys.argv[1], 'x') as record:
 json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'executable':sys.argv[2]},record)
os.execv(sys.argv[2],sys.argv[2:])
'''
SILENT = '''import os,select,sys,time
root=sys.argv[1]
end=time.monotonic()+45
with open(os.path.join(root,'silent-count'),'ab') as output:output.write(b'x')
for index in range(2):
 if not select.select([0],[],[],max(0,end-time.monotonic()))[0]:sys.exit(98)
 line=sys.stdin.buffer.readline(1024)
 if not line or not line.endswith(b'\\n'):sys.exit(99)
 with open(os.path.join(root,'silent-lines'),'ab') as output:output.write(line)
sys.exit(7)
'''


def check_inputs():
    ast.parse(LAUNCHER)
    ast.parse(SILENT)


def cleanup_failed_profile(root, cli, binary, execute, environment):
    """Retire only authenticated completed generations; never signal stored PIDs."""
    profile = root / 'zellij-profile'
    cleanupenv = dict(environment, ZMUX_LOCAL_ZELLIJ_ROOT=str(profile), ZMUX_LOCAL_ZELLIJ_BIN=str(binary))
    deadline = time.monotonic() + 55
    retired = []
    attempt = 0
    while True:
        attempt += 1
        registry = profile / 'registry.json'
        owners = document(read_file(registry, 262144)) if registry.exists() else []
        if not owners:
            return {'status': 'passed', 'authenticated_completed_owners_retired': retired}
        if time.monotonic() >= deadline:
            raise Rejected('failed native journey still has unretired owners; no unsafe signal fallback')
        for index, owner in enumerate(owners):
            status = json.loads(execute([str(cli), 'local-zellij', 'status', '--id', owner['id']],
                                       f'failure-owner-status-{attempt}-{index}', 8, env=cleanupenv))
            if status['completion_authenticated']:
                row = json.loads(execute([str(cli), 'local-zellij', 'cleanup', '--id', owner['id'], '--generation',
                                          owner['completion']['owner_generation']],
                                         f'failure-owner-cleanup-{attempt}-{index}', 12, env=cleanupenv))
                if row['namespace_cleaned'] is not True or row['registry_reclaimed'] is not True:
                    raise Rejected('failed native journey owner did not physically retire')
                retired.append(row)
        time.sleep(0.2)


def inventory(root):
    result, total = {}, 0
    for path in root.rglob('*'):
        if path.is_symlink():
            raise Rejected('native bundle contains an unreviewed link')
        if path.is_file():
            name = safe_name(str(path.relative_to(root)))
            raw = read_file(path, MAX_FILE)
            total += len(raw)
            if len(result) >= 3000 or total > 256 * 1024 * 1024:
                raise Rejected('native bundle inventory exceeds file/byte bound')
            result[name] = {'bytes': len(raw), 'sha256': digest(raw)}
        elif not path.is_dir():
            raise Rejected('native bundle contains a nonregular entry')
    return result


def saved_contents(value):
    if isinstance(value, dict):
        if value.get('kind') == 'completed_local_zellij_terminal':
            yield value
        else:
            for child in value.values():
                yield from saved_contents(child)
    elif isinstance(value, list):
        for child in value:
            yield from saved_contents(child)


def canonical_digest(domain, value):
    return digest(domain.encode() + b'\0' + json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode())


def process_record(value):
    return {('parent_pid' if key == 'parentPid' else key): item for key, item in value.items()}


def verify_report(root, source, executable, expected, parent_pid, admission):
    result = document(read_file(root / 'native-readiness.json', 65536))
    launch = document(read_file(root / 'owned-launch.json', 2048))
    if (result['state'] != 'complete' or result['scope'] != 'actual_relocated_AppKit_readiness_start_attach_completion'
            or result['sourceSHA'] != source or result['appPID'] != launch['pid']
            or result['appPID'] != result['activationPID'] or launch['parent_pid'] != parent_pid
            or launch['executable'] != str(executable) or result['bundlePath'] != str(executable.parents[2])
            or result['binarySHA256'] != admission['binary_sha256']
            or result['manifestSHA256'] != admission['manifest_sha256']):
        raise Rejected('actual native launch/activation/bundle identity differs')
    operations = result['operations']
    stages = ['first_typed_ready', 'first_frontend_joined', 'second_typed_ready',
              'silent_workload_completed', 'terminal_before_ready_0', 'terminal_before_ready_7']
    if [row['stage'] for row in operations] != stages:
        raise Rejected('native readiness operation stages missing or changed')
    first, detached, second, completed, zero, seven = [row['operation'] for row in operations]
    if (first['operationId'] != detached['operationId'] or first['operationId'] == second['operationId']
            or completed['operationId'] != second['operationId']
            or first['attachmentId'] == second['attachmentId'] or first['frontendPid'] == second['frontendPid']
            or first['ownerUuid'] != second['ownerUuid'] or len({row['epoch'] for row in (first, second, zero, seven)}) != 1):
        raise Rejected('native attach did not retain owner with a distinct physical frontend')
    for row in (first, second):
        if (row['inputReady'] is not True or row['state'] != 'input_ready' or row.get('error')
                or not row.get('checkpointRevision') or row['executionContractVersion'] != 2
                or row['profileId'].lower() != result['profileID'].lower()):
            raise Rejected('actual native first input was not admitted by typed Ready/checkpoint')
    for row in (detached, completed, zero, seven):
        if (any(row[key] is not True for key in ('sessionJoined', 'monitorJoined', 'namespaceCleaned'))
                or row.get('error')):
            raise Rejected('actual native frontend physical join or cleanup missing')
    for row, code in ((completed, 7), (zero, 0), (seven, 7)):
        if (row['workloadExit'] != {'kind': 'exited', 'value': code}
                or not row.get('completionSavedRevision') or not row.get('receiptDigest')
                or row['executionContractVersion'] != 2 or row['finalFrameReceived'] is not True):
            raise Rejected('actual native saved terminal receipt or exact exit differs')
    if zero['inputReady'] or seven['inputReady']:
        raise Rejected('completion-before-Ready was admitted interactively')
    attachments = result['attachments']
    if (len(attachments) != 2 or [row['line'] for row in attachments] != [expected['firstLine'], expected['secondLine']]
            or [row['operationID'] for row in attachments] != [first['operationId'], second['operationId']]
            or [row['surfaceUUID'].lower() for row in attachments] != [first['surface'].lower(), second['surface'].lower()]
            or any(row['actualInputResponder'] is not True or not row['sessionPID'] or not row['windowNumber'] for row in attachments)
            or len({row['sessionID'] for row in attachments}) != 2
            or len({row['sessionPID'] for row in attachments}) != 2
            or len({row['surfaceID'] for row in attachments}) != 2
            or len({row['windowID'] for row in attachments}) != 1):
        raise Rejected('actual AppKit responder/Session/CLI model correlation differs')
    owners = result['owners']
    if len(owners) != 4 or owners[0] != owners[1] or len({row['id'] for row in owners}) != 3:
        raise Rejected('silent reattachment replaced original owner/process identities')
    if (read_file(root / 'silent-count', 4) != b'x'
            or read_file(root / 'silent-lines', 1024) != (expected['firstLine'] + '\n' + expected['secondLine'] + '\n').encode()
            or any(read_file(root / f'finite-{code}-count', 4) != b'x' for code in (0, 7))):
        raise Rejected('native actual workload startup or exactly-once line oracle differs')
    completions = result['completions']
    if len(completions) != 3 or [row['exitCode'] for row in completions] != [7, 0, 7]:
        raise Rejected('native saved completion cases missing')
    for mounted, observed in zip(completions, (completed, zero, seven)):
        if (mounted['readOnlyMounted'] is not True or mounted['typingRejected'] is not True
                or mounted['statusLabel'] != f"Process exited with status {mounted['exitCode']}"
                or mounted['operationID'] != observed['operationId'] or mounted['owner'].lower() != observed['ownerUuid'].lower()):
            raise Rejected('native completion is not exact mounted read-only presentation')
        saved = document(read_file(root / safe_name(mounted['savedDocument']), 4 * 1024 * 1024))
        matches = [row for row in saved_contents(saved) if row['owner_uuid'].lower() == mounted['owner'].lower()]
        if len(matches) != 1 or matches[0]['profile_id'].lower() != result['profileID'].lower():
            raise Rejected('actual durable session did not save the completed native owner')
        envelope = matches[0]['history']['receipt']
        terminal = envelope['receipt']
        binding = terminal['binding']
        actual_owner = next(row for row in owners if row['id'].lower() == mounted['owner'].lower())
        if (envelope['execution_contract'] != 'admitted_lifetime_v2' or terminal['version'] != 2
                or terminal['domain'] != 'zmux.local-zellij.terminal.v2' or terminal['child_reaped'] is not True
                or terminal['terminal_sequence'] != 1 or terminal['workload_exit'] != observed['workloadExit']
                or binding['owner_uuid'].lower() != actual_owner['id'].lower()
                or binding['owner_generation'].lower() != actual_owner['completion']['ownerGeneration'].lower()
                or binding['server'] != process_record(actual_owner['server'])
                or binding['supervisor'] != process_record(actual_owner['supervisor'])
                or binding['workload_birth'] != process_record(actual_owner['workload'])
                or canonical_digest('zmux.local-zellij.execution.v2', binding) != terminal['binding_digest']
                or canonical_digest('zmux.local-zellij.terminal.v2', terminal) != observed['receiptDigest']):
            raise Rejected('native durable terminal receipt does not bind original processes and operation digest')
    return result


def run_relocated(bundle, output, accepted, source, execute, environment):
    if accepted.get('variant') != 'input-readiness-v1':
        raise Rejected('actual readiness native journey requires the explicit reviewed variant')
    before = inventory(bundle)
    relocated = output / 'relocated with spaces' / 'zmux.app'
    relocated.parent.mkdir(mode=0o700)
    execute(['ditto', str(bundle), str(relocated)], 'relocate-native-bundle', 60)
    if inventory(relocated) != before:
        raise Rejected('relocation changed the signed final bundle')
    # Remove the original build path, making source-location use fail visibly.
    bundle.rename(output / 'original-built-zmux.app')
    dependency = relocated / 'Contents/Resources/Dependencies/local-zellij'
    if verify_payload(dependency, 'input-readiness-v1')['binary_sha256'] != accepted['binary_sha256']:
        raise Rejected('relocated bundle dependency differs')
    execute(['codesign', '--verify', '--deep', '--strict', '--verbose=2', str(relocated)], 'relocated-bundle-signature')
    binary = dependency / 'bin/zellij'
    cli = relocated / 'Contents/Resources/bin/zmux'
    executable = relocated / 'Contents/MacOS/zmux-app'
    execute(['codesign', '--verify', '--strict', '--verbose=2', str(binary)], 'relocated-dependency-signature')
    protocol = document(read_file(dependency / 'SOURCES/input-readiness-v1/protocol.json', 65536))
    if execute([str(binary), '--zmux-input-readiness-protocol'], 'relocated-readiness-capability') != protocol['capability_stdout'].strip():
        raise Rejected('relocated dependency readiness capability differs')
    root = output / 'appkit'
    root.mkdir(mode=0o700)
    for name in ('home', 'state', 'zellij-profile'):
        (root / name).mkdir(mode=0o700)
    root = root.resolve()
    report = root / 'native-readiness.json'
    script = root / 'silent.py'
    script.write_text(SILENT)
    oracle = {'silentCommand': 'exec ' + shlex.quote(sys.executable) + ' -I ' + shlex.quote(str(script)) + ' ' + shlex.quote(str(root)),
              'firstLine': 'native-first-' + uuid.uuid4().hex, 'secondLine': 'native-second-' + uuid.uuid4().hex}
    report.with_suffix('.json.input.json').write_text(json.dumps(oracle))
    launcher = root / 'owned-app-launch.py'
    launcher.write_text(LAUNCHER)
    socket = '/tmp/zn' + uuid.uuid4().hex
    profile_id = str(uuid.uuid4())
    appenv = {key: value for key, value in environment.items() if not key.startswith(('ZMUX_', 'DYLD_'))}
    appenv.update(HOME=str(root / 'home'), SHELL='/bin/bash', TERM='xterm-256color',
                  ZMUX_SOCKET=socket, ZMUX_SOCKET_PATH=socket)
    try:
        execute([sys.executable, str(launcher), str(root / 'owned-launch.json'), str(executable),
                 '--local-zellij-readiness-profile', profile_id, str(root / 'zellij-profile'),
                 '--local-zellij-readiness-smoke', str(report), str(root / 'state')], 'actual-appkit-readiness', 150, env=appenv)
        result = verify_report(root, source, executable, oracle, os.getpid(), accepted)
    except Exception as primary:
        try:
            cleanup = cleanup_failed_profile(root, cli, binary, execute, appenv)
            (root / 'failure-cleanup.json').write_text(json.dumps(cleanup, indent=2) + '\n')
        except Exception as cleanup_error:
            raise Rejected(str(primary) + '; explicit failed-journey cleanup: ' + str(cleanup_error)) from primary
        raise
    if result['cliSHA256'] != digest(read_file(cli, MAX_FILE)) or result['cliPath'] != str(cli) or result['zellijPath'] != str(binary):
        raise Rejected('AppKit configured a different final bundled executable')
    # The final bundled CLI queries/authenticates and retires only the exact
    # completed owner generations; registry bytes alone never grant cleanup.
    cleanupenv = dict(appenv, ZMUX_LOCAL_ZELLIJ_ROOT=str(root / 'zellij-profile'), ZMUX_LOCAL_ZELLIJ_BIN=str(binary))
    cleanup = []
    for index, owner in enumerate((result['owners'][0], result['owners'][2], result['owners'][3])):
        status = json.loads(execute([str(cli), 'local-zellij', 'status', '--id', owner['id']], f'native-completion-status-{index}', 10, env=cleanupenv))
        if (status['id'].lower() != owner['id'].lower() or status['completion_authenticated'] is not True
                or status['consumption_confirmed'] is not True or status['completion_error'] is not None):
            raise Rejected('final bundled CLI did not authenticate saved native completion')
        row = json.loads(execute([str(cli), 'local-zellij', 'cleanup', '--id', owner['id'], '--generation',
                                 owner['completion']['ownerGeneration']], f'native-completion-cleanup-{index}', 12, env=cleanupenv))
        if row['namespace_cleaned'] is not True or row['registry_reclaimed'] is not True:
            raise Rejected('final bundled CLI failed exact completed owner retirement')
        cleanup.append(row)
    if json.loads(execute([str(cli), 'local-zellij', 'list'], 'native-final-empty-registry', 10, env=cleanupenv)) != []:
        raise Rejected('native fixture retained unexpected owners after explicit retirement')
    if (inventory(relocated) != before or verify_payload(dependency, 'input-readiness-v1')['binary_sha256'] != accepted['binary_sha256']
            or Path(socket).exists()):
        raise Rejected('native execution changed bundle bytes or retained control socket')
    execute(['codesign', '--verify', '--deep', '--strict', '--verbose=2', str(relocated)], 'post-appkit-bundle-signature')
    return {'native_report': result, 'owned_launch': document(read_file(root / 'owned-launch.json', 2048)),
            'bundle_inventory_sha256': digest(json.dumps(before, sort_keys=True).encode()),
            'bundle_files': len(before), 'original_build_path_absent': not bundle.exists(),
            'exact_owner_cleanup': cleanup, 'actual_final_bundled_cli': True,
            'dependency_resigned_or_rebuilt': False}
