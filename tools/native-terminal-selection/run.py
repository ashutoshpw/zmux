#!/usr/bin/env python3
"""Run bounded native selection cases against an actual packaged PTY/view."""
import argparse
import ast
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import uuid
import zipfile

REPO = Path(__file__).resolve().parents[2]
OWN_INPUTS = ('tools/native-terminal-selection/run.py', '.github/workflows/macos-development.yml',
              'tools/native-terminal-selection/CopyMetadataContract.swift')
# Compile the complete real model dependency closure, including the bridge owner
# used by NotificationDelivery; never substitute test definitions for app types.
METADATA_INPUTS = tuple('apps/macos/Sources/' + name + '.swift' for name in (
    'Models', 'WindowModels', 'BrowserDownloadModels', 'BrowserAutomation',
    'NotificationDelivery', 'NativeBridge', 'NativeStoragePath', 'TerminalKeyEncoder')) + (
    'crates/native-bridge/include/zmux.h', 'tools/native-terminal-selection/CopyMetadataContract.swift')
CHILD = '''#!/usr/bin/env python3
import fcntl,json,os,pathlib,select,struct,termios,time,tty
root=pathlib.Path(os.environ['ZMUX_SELECTION_ROOT'])
nonce=os.environ['ZMUX_SELECTION_NONCE']
deadline=time.monotonic()+120
tty.setraw(0)
with (root/'child.json').open('x') as out:
 json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':nonce},out)
rows=['alpha beta','alpha.beta  value_name /tmp/file','A界 e\\u0301🙂 Z','ab\\x1b[31mcd\\x1b[0mef','NATIVE_SELECTION_READY_'+nonce]
os.write(1,('\\x1b[2J\\x1b[H'+'\\r\\n'.join(rows)+'\\r\\n').encode())
for key,marker in [('w','WRAP_READY_'),('v','WIDE_READY_'),('m','MARGIN_READY_'),('u','UNICODE_READY_'),('h','HARD_READY_'),('p','PARK_READY_')]:
 ready,_,_=select.select([0],[],[],max(0,deadline-time.monotonic()))
 if not ready:break
 assert os.read(0,1)==key.encode()
 rows_count,columns=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))[:2]
 assert 8<=rows_count<=120 and 64<=columns<=240
 name='wrap-child.json' if key=='w' else 'copy-child-'+key+'.json'
 with (root/name).open('x') as out:
  json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':nonce,'rows':rows_count,'columns':columns,'phase':key},out)
 payload='X'*(columns-1)+' EDGE'
 if key=='v':payload='X'*(columns-2)+'界Z'
 if key=='m':payload='X'*(columns-1)+'界Z'
 if key=='u':payload='X'*(columns-1)+'e\\u0301🙂Z'
 if key=='h':payload='HARD_A\\r\\n\\r\\nHARD_B'
 if key=='p':payload+='\\r\\n'+'\\r\\n'.join('FILLER_'+str(n) for n in range(rows_count+4))
 os.write(1,('\\x1b[2J\\x1b[H'+payload+'\\r\\n'+marker+nonce+'\\r\\n').encode())
time.sleep(max(0,deadline-time.monotonic()))
'''


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check-inputs', action='store_true')
    args = parser.parse_args()
    native = load('selection_native_inputs', REPO / 'tools/native-idle-workloads/run.py')

    def sources():
        frozen = native.source_files()
        for name in OWN_INPUTS:
            path = REPO / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size > 131_072:
                raise RuntimeError('Selection input exceeds its byte/type bound')
            frozen[name] = native.digest(path)
        if len(frozen) > 512:
            raise RuntimeError('Selection inventory exceeds its file bound')
        return frozen

    frozen = sources()
    ast.parse(CHILD)
    if any(name not in frozen for name in METADATA_INPUTS):
        raise RuntimeError('Selection decoder dependency is absent from retained sources')
    if args.check_inputs:
        print(json.dumps({'sources': len(frozen), 'actual_native_execution': False,
                          'app_seconds': 45, 'independent_child_seconds': 120}))
        return 0
    if platform.system() != 'Darwin' or platform.machine() != 'arm64' or args.output is None:
        parser.error('Native selection execution requires Darwin ARM64 and --output')
    root = args.output.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip()
    nonce = str(uuid.uuid4())
    receipt = {'schema_version': 1, 'state': 'starting', 'source_sha': source,
               'source_sha256': frozen, 'phases': [], 'engine': 'vt100',
               'default_engine_changed': False, 'physical_mouse_or_voiceover_verified': False}
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(('ZMUX_', 'DYLD_')) and key not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')}
    environment.update(CARGO_TARGET_DIR=str(REPO / 'target/agent-validation' / ('selection-' + nonce) / 'cargo'), ZMUX_SELECTION_ROOT=str(root),
                       ZMUX_SELECTION_NONCE=nonce, ZMUX_SOCKET=str(root / 'control.socket'),
                       ZMUX_SOCKET_PATH=str(root / 'control.socket'))
    runner = load('selection_owned_process_runner', REPO / 'tools/terminal-feasibility/run.py')
    try:
        for name, wanted in frozen.items():
            target = root / 'source' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, target)
            if native.digest(target) != wanted:
                raise RuntimeError('Selection retained source differs')
        runner.run([sys.executable, 'scripts/build-macos-dev', '--architecture', 'host'],
                   'build', root, receipt['phases'], timeout=600, cwd=REPO, env=environment)
        info = native.document(REPO / 'dist/build-info.json')
        if (info['source_sha'] != source or info['optional_ghostty_linked']
                or info['default_terminal_engine'] != 'vt100'):
            raise RuntimeError('Selection native baseline build identity/default differs')
        archive = REPO / 'dist' / ('zmux-dev-arm64-' + source[:7] + '.zip')
        if archive.stat().st_size > 64 * 1024 * 1024:
            raise RuntimeError('Selection app archive exceeds its byte bound')
        shutil.copy2(archive, root / archive.name)
        receipt.update(build_info=info, archive={'name': archive.name, 'sha256': native.digest(archive)})
        decoder_probe = root / 'copy-metadata-contract'
        frameworks = REPO / 'dist/zmux.app/Contents/Frameworks'
        receipt['metadata_probe_inputs'] = {name: frozen[name] for name in METADATA_INPUTS}
        receipt['metadata_bridge_sha256'] = native.digest(frameworks / 'libzmux_native_bridge.dylib')
        with zipfile.ZipFile(archive) as packaged:
            bridge_entry = packaged.getinfo('zmux.app/Contents/Frameworks/libzmux_native_bridge.dylib')
            if bridge_entry.file_size > 64 * 1024 * 1024:
                raise RuntimeError('Selection archived decoder bridge exceeds its byte bound')
            if hashlib.sha256(packaged.read(bridge_entry)).hexdigest() != receipt['metadata_bridge_sha256']:
                raise RuntimeError('Selection decoder bridge differs from the archived app build')
        runner.run(['/usr/bin/xcrun', 'swiftc', '-parse-as-library', '-swift-version', '6', '-warnings-as-errors',
                    '-strict-concurrency=complete', '-target', 'arm64-apple-macos14.0',
                    '-import-objc-header', str(REPO / 'crates/native-bridge/include/zmux.h'),
                    *[str(REPO / name) for name in METADATA_INPUTS if name.endswith('.swift')],
                    '-L', str(frameworks), '-lzmux_native_bridge',
                    '-framework', 'AppKit', '-framework', 'WebKit', '-framework', 'UserNotifications',
                    '-Xlinker', '-rpath', '-Xlinker', str(frameworks), '-o', str(decoder_probe)],
                   'metadata-compile', root, receipt['phases'], timeout=60, cwd=REPO, env=environment)
        runner.run([str(decoder_probe)], 'metadata-contract', root, receipt['phases'],
                   timeout=5, cwd=REPO, env=environment)
        child = root / 'selection-child.py'
        child.write_text(CHILD)
        child.chmod(0o700)
        environment['SHELL'] = str(child)
        runner.run([str(REPO / 'dist/zmux.app/Contents/MacOS/zmux-app'), '--terminal-selection-smoke',
                    str(root / 'native-selection.json')], 'native-selection', root,
                   receipt['phases'], timeout=45, cwd=REPO, env=environment)
        result = native.document(root / 'native-selection.json')
        registration = native.document(root / 'child.json')
        if (result['sourceSHA'] != source or result['nonce'] != nonce or result['state'] != 'complete'
                or registration != {'pid': result['childPID'], 'parent_pid': result['appPID'], 'nonce': nonce}
                or not result['cases'] or any(case['expected'] != case['observed'] for case in result['cases'])):
            raise RuntimeError('Selection native source, real child, cases or shared shutdown differs')
        receipt['native_report'] = result
        receipt['registration'] = registration
        if sources() != frozen:
            raise RuntimeError('Selection inputs changed during the run')
        receipt['state'] = 'complete'
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        if any(phase['log_bytes_seen'] != phase['log_bytes_retained'] for phase in receipt['phases']):
            receipt.update(state='failed', error='Selection log exceeded its retained byte bound')
        native.save(root / 'receipt.json', receipt)
    print(json.dumps({'state': receipt['state'], 'receipt': str(root / 'receipt.json')}))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
