#!/usr/bin/env python3
"""Run bounded native selection cases against an actual packaged PTY/view."""
import argparse
import ast
import importlib.util
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from threading import Thread
import uuid
import zipfile

REPO = Path(__file__).resolve().parents[2]
OWN_INPUTS = ('tools/native-terminal-selection/run.py', '.github/workflows/macos-development.yml',
              'tools/native-terminal-selection/CopyMetadataContract.swift')
# Compile the complete real model dependency closure, including the bridge owner
# used by NotificationDelivery; never substitute test definitions for app types.
METADATA_INPUTS = tuple('apps/macos/Sources/' + name + '.swift' for name in (
    'Models', 'WindowModels', 'BrowserDownloadModels', 'BrowserAutomation',
    'NotificationDelivery', 'FileLoadGate', 'FileHostClient', 'NativeBridge',
    'NativeStoragePath', 'TerminalKeyEncoder', 'BrowserProfileLeases', 'BrowserProfileOperations')) + (
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
for key,marker in [('w','WRAP_READY_'),('v','WIDE_READY_'),('m','MARGIN_READY_'),('u','UNICODE_READY_'),('h','HARD_READY_'),('p','PARK_READY_'),('b','WORD_READY_'),('c','WORD_UNICODE_READY_'),('t','POINTER_READY_'),('y','HOVER_READY_'),('l','TERMINAL_LINK_READY_')]:
 probe_sequence=0
 while True:
  ready,_,_=select.select([0],[],[],max(0,deadline-time.monotonic()))
  if not ready:break
  command=os.read(0,1)
  if command==b'q':
   probe_sequence+=1
   rows_count,columns=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))[:2]
   if not 8<=rows_count<=120 or not 64<=columns<=240:raise RuntimeError('invalid probe PTY size')
   name='probe-'+key+'-'+str(probe_sequence)+'.json'
   payload={'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':nonce,'request_id':probe_sequence,
            'expected_next_phase':key,'rows':rows_count,'columns':columns}
   pending=root/(name+'.pending')
   descriptor=os.open(pending,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
   with os.fdopen(descriptor,'w') as out:json.dump(payload,out)
   os.replace(pending,root/name)
   continue
  assert command==key.encode()
  break
 if not ready:break
 rows_count,columns=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))[:2]
 assert 8<=rows_count<=120 and 64<=columns<=240
 name='wrap-child.json' if key=='w' else 'copy-child-'+key+'.json'
 with (root/name).open('x') as out:
  json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':nonce,'rows':rows_count,'columns':columns,'phase':key},out)
 payload='X'*(columns-1)+' EDGE'
 if key=='b':payload='word'+'x'*(columns+4)+' end\\r\\nhardleft\\r\\nhardright'
 if key=='c':payload='X'*(columns-1)+'e\\u0301tail end'
 if key=='v':payload='X'*(columns-2)+'界Z'
 if key=='m':payload='X'*(columns-1)+'界Z'
 if key=='u':payload='X'*(columns-1)+'e\\u0301🙂Z'
 if key=='h':payload='HARD_A\\r\\n\\r\\nHARD_B'
 if key in ('t','y'):payload='pointer line'+' '*(columns-13)+'Z\\r\\nsecond pointer line'
 if key=='l':
  plain_url=os.environ['ZMUX_TERMINAL_LINK_PLAIN_URL']
  osc8_url=os.environ['ZMUX_TERMINAL_LINK_OSC8_URL']
  esc=chr(27)
  osc8_start=esc+']8;;'
  osc8_end=esc+'\\\\'
  payload='PLAIN '+plain_url+'\\r\\n'+osc8_start+osc8_url+osc8_end+'OSC8 LINK'+osc8_start+osc8_end
 if key=='p':payload+='\\r\\n'+'\\r\\n'.join('FILLER_'+str(n) for n in range(rows_count+4))
 mode=('\\x1b[?1002h' if key=='t' else '\\x1b[?1003h')+'\\x1b[?1006h' if key in ('t','y') else ''
 os.write(1,(mode+'\\x1b[2J\\x1b[H'+payload+'\\r\\n'+marker+nonce+'\\r\\n').encode())
 if key in ('t','y'):
  captured=bytearray()
  while time.monotonic()<deadline and b'\\x18' not in captured:
   ready,_,_=select.select([0],[],[],min(10,max(0,deadline-time.monotonic())))
   if not ready:break
   chunk=os.read(0,1024)
   if not chunk:break
   captured.extend(chunk)
   assert len(captured)<=4096
  body,found,trailing=bytes(captured).partition(b'\\x18')
  assert found==b'\\x18'
  with (root/('pointer-child.json' if key=='t' else 'pointer-hover-child.json')).open('x') as out:
   json.dump({'pid':os.getpid(),'parent_pid':os.getppid(),'nonce':nonce,'rows':rows_count,'columns':columns,'phase':key,
              'captured_hex':body.hex(),'sentinel_hex':found.hex(),'trailing_hex':trailing.hex()},out)
  os.write(1,('\\x1b[?1002l\\x1b[?1003l\\x1b[?1006l\\r\\nPOINTER_CAPTURED_'+nonce+'\\r\\n').encode())
time.sleep(max(0,deadline-time.monotonic()))
'''


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_probe_protocol():
    """Exercise the exact bounded child protocol against a real PTY."""
    import errno
    import fcntl
    import pty
    import select
    import stat
    import struct
    import termios

    nonce = str(uuid.uuid4())
    with tempfile.TemporaryDirectory(prefix='zsel-probe-', dir='/tmp') as directory:
        fixture = Path(directory)
        fixture_metadata = fixture.lstat()
        if fixture_metadata.st_uid != os.getuid() or stat.S_IMODE(fixture_metadata.st_mode) & 0o077:
            raise RuntimeError('Selection probe fixture root is not private to this user')
        master, slave = pty.openpty()
        process = None
        output = bytearray()

        def drain():
            while select.select([master], [], [], 0)[0]:
                try:
                    chunk = os.read(master, 4096)
                except BlockingIOError:
                    return
                except OSError as error:
                    if error.errno == errno.EIO:
                        return
                    raise
                if not chunk:
                    return
                output.extend(chunk)

        def wait_for_file(path, timeout):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                drain()
                if path.is_file():
                    data = path.read_bytes()
                    if len(data) > 4096:
                        raise RuntimeError('Selection probe fixture file exceeds byte bound')
                    return json.loads(data)
                if process.poll() is not None:
                    raise RuntimeError('Selection probe child exited before publishing ' + path.name)
                time.sleep(0.01)
            raise RuntimeError('Selection probe child did not publish ' + path.name)

        def wait_for_marker(marker, timeout):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                drain()
                if marker in output:
                    return
                if process.poll() is not None:
                    raise RuntimeError('Selection probe child exited before its phase marker')
                time.sleep(0.01)
            raise RuntimeError('Selection probe child did not emit its phase marker')

        def resize(rows, columns):
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack('HHHH', rows, columns, 0, 0))

        try:
            resize(24, 80)
            environment = dict(os.environ, ZMUX_SELECTION_ROOT=str(fixture), ZMUX_SELECTION_NONCE=nonce)
            process = subprocess.Popen([sys.executable, '-c', CHILD], stdin=slave, stdout=slave, stderr=slave,
                                       env=environment, start_new_session=True)
            os.close(slave)
            registration = wait_for_file(fixture / 'child.json', 5)
            if registration != {'pid': process.pid, 'parent_pid': os.getpid(), 'nonce': nonce}:
                raise RuntimeError('Selection probe child registration identity differs')

            os.write(master, b'q')
            first = wait_for_file(fixture / 'probe-w-1.json', 5)
            if first != {'pid': process.pid, 'parent_pid': os.getpid(), 'nonce': nonce, 'request_id': 1,
                         'expected_next_phase': 'w', 'rows': 24, 'columns': 80}:
                raise RuntimeError('First read-only probe did not report the original PTY geometry')
            if stat.S_IMODE((fixture / 'probe-w-1.json').stat().st_mode) & 0o077:
                raise RuntimeError('First read-only probe acknowledgment is not private')

            resize(31, 103)
            os.write(master, b'q')
            second = wait_for_file(fixture / 'probe-w-2.json', 5)
            if second != {'pid': process.pid, 'parent_pid': os.getpid(), 'nonce': nonce, 'request_id': 2,
                          'expected_next_phase': 'w', 'rows': 31, 'columns': 103}:
                raise RuntimeError('Fresh read-only probe did not observe the real PTY resize')
            if stat.S_IMODE((fixture / 'probe-w-2.json').stat().st_mode) & 0o077:
                raise RuntimeError('Second read-only probe acknowledgment is not private')

            marker = ('WRAP_READY_' + nonce).encode()
            os.write(master, b'w')
            wait_for_marker(marker, 5)
            phase = json.loads((fixture / 'wrap-child.json').read_text())
            if (phase != {'pid': process.pid, 'parent_pid': os.getpid(), 'nonce': nonce,
                          'rows': 31, 'columns': 103, 'phase': 'w'}
                    or output.count(marker) != 1 or process.poll() is not None):
                raise RuntimeError('One post-probe data phase did not use the resized PTY geometry')
        finally:
            if process is not None:
                process.terminate()
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=2)
            os.close(master)
            try:
                os.close(slave)
            except OSError:
                pass


def start_terminal_link_fixture(nonce):
    token = nonce.replace('-', '')[:16]
    plain_path = '/p-' + token
    osc8_path = '/o-' + token
    titles = {
        plain_path: 'ZMUX_TERMINAL_LINK_PLAIN_' + nonce,
        osc8_path: 'ZMUX_TERMINAL_LINK_OSC8_' + nonce,
    }
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            title = titles.get(self.path)
            if title is None:
                self.send_error(404)
                return
            requests.append(self.path)
            body = ('<!doctype html><title>' + title + '</title><p>' + self.path + '</p>').encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Connection', 'close')
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format, *_args):
            return

    server = HTTPServer(('127.0.0.1', 0), Handler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = 'http://127.0.0.1:' + str(server.server_port)
    return server, thread, base + plain_path, base + osc8_path, requests


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check-inputs', action='store_true')
    parser.add_argument('--skip-build', action='store_true',
                        help='Exercise the existing packaged app and archive without rebuilding')
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
        verify_probe_protocol()
        print(json.dumps({'sources': len(frozen), 'actual_native_execution': False,
                          'app_seconds': 45, 'independent_child_seconds': 120,
                          'pty_probe_protocol': 'passed'}))
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
    link_server = None
    link_thread = None
    link_requests = []
    try:
        for name, wanted in frozen.items():
            target = root / 'source' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, target)
            if native.digest(target) != wanted:
                raise RuntimeError('Selection retained source differs')
        runner.run([str(REPO / 'scripts/check-macos-sources')], 'swift-source-check', root,
                   receipt['phases'], timeout=240, cwd=REPO, env=environment)
        if args.skip_build:
            receipt['build_mode'] = 'reused_existing_package'
        else:
            runner.run([sys.executable, 'scripts/build-macos-dev', '--architecture', 'host'],
                       'build', root, receipt['phases'], timeout=600, cwd=REPO, env=environment)
            receipt['build_mode'] = 'host_cold_build'
        info = native.document(REPO / 'dist/build-info.json')
        if (info['source_sha'] != source or info['optional_ghostty_linked']
                or info['default_terminal_engine'] != 'vt100'):
            raise RuntimeError('Selection native baseline build identity/default differs')
        architectures = info.get('architectures')
        if architectures == ['arm64', 'x86_64']:
            archive_architecture = 'universal'
        elif architectures in (['arm64'], ['x86_64']):
            archive_architecture = architectures[0]
        else:
            raise RuntimeError('Selection package has unsupported architecture metadata')
        if not args.skip_build and archive_architecture != 'arm64':
            raise RuntimeError('Selection host build did not produce the runner-architecture package')
        archive = REPO / 'dist' / ('zmux-dev-' + archive_architecture + '-' + source[:7] + '.zip')
        bundle = REPO / 'dist/zmux.app'
        app_binary = bundle / 'Contents/MacOS/zmux-app'
        bridge_binary = bundle / 'Contents/Frameworks/libzmux_native_bridge.dylib'
        bundled_info = bundle / 'Contents/Resources/build-info.json'
        if any(path.is_symlink() or not path.is_file() for path in (app_binary, bridge_binary, bundled_info)):
            raise RuntimeError('Selection package is missing a regular app, bridge or build receipt')
        if json.loads(bundled_info.read_text()) != info:
            raise RuntimeError('Selection bundle build receipt differs from dist/build-info.json')
        expected_architectures = set(architectures)
        for path in (app_binary, bridge_binary):
            actual_architectures = set(subprocess.check_output(
                ['/usr/bin/lipo', '-archs', str(path)], text=True, timeout=10).split())
            if actual_architectures != expected_architectures:
                raise RuntimeError('Selection app or native bridge architectures differ from build-info.json')
        if archive.stat().st_size > 64 * 1024 * 1024:
            raise RuntimeError('Selection app archive exceeds its byte bound')
        shutil.copy2(archive, root / archive.name)
        receipt.update(build_info=info, archive={'name': archive.name, 'architecture': archive_architecture,
                                                 'sha256': native.digest(archive)})
        decoder_probe = root / 'copy-metadata-contract'
        frameworks = bundle / 'Contents/Frameworks'
        receipt['metadata_probe_inputs'] = {name: frozen[name] for name in METADATA_INPUTS}
        receipt['metadata_bridge_sha256'] = native.digest(frameworks / 'libzmux_native_bridge.dylib')
        with zipfile.ZipFile(archive) as packaged:
            for path, entry_name in (
                (app_binary, 'zmux.app/Contents/MacOS/zmux-app'),
                (bridge_binary, 'zmux.app/Contents/Frameworks/libzmux_native_bridge.dylib'),
                (bundled_info, 'zmux.app/Contents/Resources/build-info.json'),
            ):
                entry = packaged.getinfo(entry_name)
                if entry.file_size > 64 * 1024 * 1024:
                    raise RuntimeError('Selection archive member exceeds its byte bound: ' + entry_name)
                archived_sha = hashlib.sha256(packaged.read(entry)).hexdigest()
                if archived_sha != native.digest(path):
                    raise RuntimeError('Selection bundle member differs from its archive: ' + entry_name)
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
        link_server, link_thread, plain_url, osc8_url, link_requests = start_terminal_link_fixture(nonce)
        environment['ZMUX_TERMINAL_LINK_PLAIN_URL'] = plain_url
        environment['ZMUX_TERMINAL_LINK_OSC8_URL'] = osc8_url
        child = root / 'selection-child.py'
        child.write_text(CHILD)
        child.chmod(0o700)
        environment['SHELL'] = str(child)
        runner.run([str(app_binary), '--terminal-selection-smoke',
                    str(root / 'native-selection.json')], 'native-selection', root,
                   receipt['phases'], timeout=45, cwd=REPO, env=environment)
        result = native.document(root / 'native-selection.json')
        registration = native.document(root / 'child.json')
        if (result['sourceSHA'] != source or result['nonce'] != nonce or result['state'] != 'complete'
                or registration != {'pid': result['childPID'], 'parent_pid': result['appPID'], 'nonce': nonce}
                or not result['cases'] or any(case['expected'] != case['observed'] for case in result['cases'])):
            raise RuntimeError('Selection native source, real child, cases or shared shutdown differs')
        required_copy_cases = {'pointer_drag_copy_without_shift', 'pointer_reverse_drag_copy_without_shift',
                               'pointer_coalesced_drag_copy_without_shift',
                               'pointer_control_c_clears_copy_candidate', 'pointer_click_clears_copy_candidate',
                               'pointer_paste_clears_copy_candidate', 'pointer_right_edge_copy_without_shift'}
        if not required_copy_cases.issubset({case['name'] for case in result['cases']}):
            raise RuntimeError('Selection report omitted direct-copy or stale-candidate acceptance')
        link_token = nonce.replace('-', '')[:16]
        if not all(path in link_requests for path in ('/p-' + link_token, '/o-' + link_token)):
            raise RuntimeError('Embedded browser did not request both loopback-only terminal-link pages')
        for filename, phase, field in [('pointer-child.json', 't', 'pointerCapture'),
                                       ('pointer-hover-child.json', 'y', 'hoverPointerCapture')]:
            pointer = native.document(root / filename)
            if (pointer.get('pid') != result['childPID'] or pointer.get('parent_pid') != result['appPID']
                or pointer.get('nonce') != nonce or pointer.get('phase') != phase
                or pointer.get('sentinel_hex') != '18' or pointer.get('trailing_hex') != ''
                or result.get(field) != pointer):
                raise RuntimeError('Independent pointer child capture or exact native identity differs')
            receipt[field] = pointer
        receipt['native_report'] = result
        receipt['registration'] = registration
        receipt['terminal_link_requests'] = link_requests
        if sources() != frozen:
            raise RuntimeError('Selection inputs changed during the run')
        receipt['state'] = 'complete'
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        if link_server is not None:
            link_server.shutdown()
            link_server.server_close()
        if link_thread is not None:
            link_thread.join(timeout=2)
        if any(phase['log_bytes_seen'] != phase['log_bytes_retained'] for phase in receipt['phases']):
            receipt.update(state='failed', error='Selection log exceeded its retained byte bound')
        native.save(root / 'receipt.json', receipt)
    print(json.dumps({'state': receipt['state'], 'receipt': str(root / 'receipt.json')}))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
