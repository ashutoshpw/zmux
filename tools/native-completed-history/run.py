#!/usr/bin/env python3
"""Exercise native saved-output presentation using an isolated generated restore document.

The historical receipt in this fixture is generated input. It does not represent
an authenticated live supervisor or establish the core no-launch contract.
"""
import argparse
import ast
import hashlib
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
OWN_INPUTS = ('tools/native-completed-history/run.py', '.github/workflows/macos-development.yml')
LAUNCHER = '''import json,os,sys
with open(sys.argv[1], 'x') as record:
 json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'executable':sys.argv[2]},record)
os.execv(sys.argv[2],sys.argv[2:])
'''


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fixture():
    namespace, owner, profile, generation, boot = (str(uuid.uuid4()) for _ in range(5))

    def identity(pid, parent):
        # Deliberately fixture-only historical values, never live process evidence.
        return {'pid': pid, 'uid': os.getuid(), 'parent_pid': parent, 'start': [1, 1],
                'boot': boot, 'executable': '/fixture-only/retired-zellij'}

    binding = {'version': 1, 'owner_uuid': owner, 'owner_generation': generation,
               'namespace': {'root': [1, 2], 'socket': [1, 3]}, 'completion_socket': [1, 4],
               'server': identity(4_294_967_000, 1),
               'supervisor': identity(4_294_967_001, 4_294_967_000),
               'workload': identity(4_294_967_002, 4_294_967_001)}
    # Wire order follows the actual CommittedBinding serde contract.
    digest = hashlib.sha256(json.dumps(binding, separators=(',', ':')).encode()).hexdigest()
    receipt = {'version': 1, 'domain': 'zmux.local-zellij.terminal.v1', 'binding': binding,
               'binding_digest': digest, 'terminal_sequence': 1,
               'workload_exit': {'kind': 'exited', 'value': 7}, 'child_reaped': True}
    text = ''.join(f'row {row:03d} · α界 e\u0301🙂  \n' for row in range(64))
    content = {'kind': 'completed_local_zellij_terminal', 'profile_id': profile, 'owner_uuid': owner,
               'history': {'receipt': receipt, 'viewport': {'contents': text, 'truncated': True}}}
    document = {'format': 'zmux.session', 'version': 7, 'identity_namespace': namespace,
                'next_id': 10, 'focused_workspace': 1,
                'workspaces': [{'id': 1, 'name': 'Fixture saved output', 'focused_pane': 2, 'todos': [],
                                'root': {'kind': 'leaf', 'pane': {'id': 2, 'selected_surface': 3,
                                         'surfaces': [{'id': 3, 'title': 'Fixture completed history', 'content': content}]}}}],
                'window_layout': {'windows': [{'id': 4, 'workspace_ids': [1], 'selected_workspace': 1},
                                              {'id': 5, 'workspace_ids': [], 'selected_workspace': None}],
                                  'focused_window': 4}}
    assert len(text.encode()) <= 16_384 and len(json.dumps(document).encode()) <= 65_536
    return document, text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check-inputs', action='store_true')
    args = parser.parse_args()
    native = load('completed_history_native_inputs', REPO / 'tools/native-idle-workloads/run.py')

    def sources():
        result = native.source_files()
        for name in OWN_INPUTS:
            path = REPO / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size > 131_072:
                raise RuntimeError('Completed-history fixture input exceeds its type/byte bound')
            result[name] = native.digest(path)
        if len(result) > 512:
            raise RuntimeError('Completed-history retained source inventory exceeds its file bound')
        return result

    frozen = sources()
    ast.parse(LAUNCHER)
    document, text = fixture()
    if args.check_inputs:
        print(json.dumps({'sources': len(frozen), 'actual_native_execution': False,
                          'fixture_generated_saved_history': True, 'app_seconds': 60}))
        return 0
    if platform.system() != 'Darwin' or platform.machine() != 'arm64' or args.output is None:
        parser.error('Native completed-history execution requires Darwin ARM64 and --output')
    root = args.output.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    profile = root / 'profile'
    profile.mkdir(mode=0o700)
    saved = profile / 'session.json'
    saved.write_text(json.dumps(document, ensure_ascii=False, separators=(',', ':')))
    saved.chmod(0o600)
    shutil.copy2(saved, root / 'fixture-session-input.json')
    report = root / 'native-completed-history.json'
    oracle = report.with_suffix(report.suffix + '.expected.txt')
    oracle.write_text(text)
    oracle.chmod(0o600)
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip()
    receipt = {'schema_version': 1, 'state': 'starting', 'source_sha': source,
               'source_sha256': frozen, 'phases': [],
               'scope': 'fixture_generated_saved_history_native_presentation',
               'supervisor_authentication_verified': False, 'core_no_launch_verified': False,
               'fixture_session_sha256': native.digest(saved), 'expected_output_sha256': native.digest(oracle)}
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(('ZMUX_', 'DYLD_')) and key not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')}
    environment.update(CARGO_TARGET_DIR=str(REPO / 'target/agent-validation' / ('completed-history-' + str(uuid.uuid4())) / 'cargo'),
                       ZMUX_SOCKET=str(root / 'control.socket'), ZMUX_SOCKET_PATH=str(root / 'control.socket'))
    runner = load('completed_history_owned_process_runner', REPO / 'tools/terminal-feasibility/run.py')
    try:
        for name, wanted in frozen.items():
            target = root / 'source' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, target)
            if native.digest(target) != wanted:
                raise RuntimeError('Completed-history retained source differs')
        runner.run([sys.executable, 'scripts/build-macos-dev', '--architecture', 'host'],
                   'build', root, receipt['phases'], timeout=600, cwd=REPO, env=environment)
        info = native.document(REPO / 'dist/build-info.json')
        if info['source_sha'] != source or info['optional_ghostty_linked'] or info['default_terminal_engine'] != 'vt100':
            raise RuntimeError('Completed-history build identity/default differs')
        archive = REPO / 'dist' / ('zmux-dev-arm64-' + source[:7] + '.zip')
        if archive.stat().st_size > 64 * 1024 * 1024:
            raise RuntimeError('Completed-history app archive exceeds its byte bound')
        shutil.copy2(archive, root / archive.name)
        receipt.update(build_info=info, archive={'name': archive.name, 'sha256': native.digest(archive)})
        launcher = root / 'owned-app-launch.py'
        launcher.write_text(LAUNCHER)
        executable = str(REPO / 'dist/zmux.app/Contents/MacOS/zmux-app')
        runner.run([sys.executable, str(launcher), str(root / 'owned-launch.json'), executable,
                    '--completed-history-smoke', str(report), str(profile)], 'native-completed-history', root,
                   receipt['phases'], timeout=60, cwd=REPO, env=environment)
        result = native.document(report)
        owned = native.document(root / 'owned-launch.json')
        required_checks = [
            'actual_restore_model_and_mounted_saved_output',
            'actual_select_all_and_exact_utf8_copy',
            'native_typing_and_menu_paste_leave_output_read_only',
            'canonical_cross_window_move_retains_selection_scroll_and_focus',
            'source_window_close_retains_saved_output_state']
        selection = 'row 020 · α界 e\u0301🙂  '
        # NSRange is UTF-16, independent of Python code point or UTF-8 offsets.
        start = len(text[:text.index(selection)].encode('utf-16-le')) // 2
        length = len(selection.encode('utf-16-le')) // 2
        positions = result['positions']
        expected_copies = [('select_all', text), ('unicode_selection', selection),
                           ('after_move', selection), ('after_source_close', selection)]
        if (result['sourceSHA'] != source or result['state'] != 'complete'
                or result['scope'] != receipt['scope'] or result['activationPID'] != result['appPID']
                or result['appPID'] != owned['pid'] or owned['parent_pid'] != os.getpid()
                or owned['executable'] != executable
                or result['checks'] != required_checks
                or [(row['name'], row['expected']) for row in result['copies']] != expected_copies
                or any(row['expected'].encode() != row['observed'].encode() for row in result['copies'])
                or [row['stage'] for row in positions] != ['before_move', 'after_move', 'after_source_close']
                or any((row['location'], row['length']) != (start, length) for row in positions)
                or positions[0]['scrollY'] <= 20
                or any(abs(row['scrollY'] - positions[0]['scrollY']) >= 0.5 for row in positions)):
            raise RuntimeError('Actual restored history native report, copy or shutdown differs')
        receipt['native_report'] = result
        receipt['owned_launch'] = owned
        if sources() != frozen:
            raise RuntimeError('Completed-history inputs changed during the run')
        receipt['state'] = 'complete'
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        if any(phase['log_bytes_seen'] != phase['log_bytes_retained'] for phase in receipt['phases']):
            receipt.update(state='failed', error='Completed-history log exceeded its retained byte bound')
        native.save(root / 'receipt.json', receipt)
    print(json.dumps({'state': receipt['state'], 'receipt': str(root / 'receipt.json')}))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
