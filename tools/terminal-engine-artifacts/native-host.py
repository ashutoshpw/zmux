#!/usr/bin/env python3
"""Opt-in host app proof using only an explicit verified retained engine root."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

CHILD = '''#!/usr/bin/env python3
import hashlib,json,os,pathlib,select,time,tty
root=pathlib.Path(os.environ['ZMUX_ENGINE_SMOKE_ROOT']);tty.setraw(0)
def read(expected):
    value=b'';deadline=time.monotonic()+8
    while len(value)<len(expected):
        remaining=deadline-time.monotonic()
        assert remaining>0,'actual native child input deadline'
        assert select.select([0],[],[],remaining)[0],'actual native child input timeout'
        chunk=os.read(0,min(4096,len(expected)-len(value)))
        assert chunk,'actual native child input closed early'
        value+=chunk
    assert value==expected,repr((len(value),value[:64],value[-64:]))
    return value
os.write(1,b'\\x1b[6n');reply=read(b'\\x1b[1;1R')
os.write(1,b'\\x1b[?1h\\x1b[?2004hNATIVE_ENGINE_READY')
expected=b'\\x1bOA\\x1bOA\\x1b[200~'+b'N'*16380+b'\\r\\n Q\\x1b[201~END'
actual=read(expected)
record={'success':True,'pid':os.getpid(),'query_reply':reply.hex(),'input_bytes':len(actual),'input_sha256':hashlib.sha256(actual).hexdigest(),'special_key_prefix':actual[:6].hex(),'whole_paste_frames':actual.count(b'\\x1b[200~'),'raw_commit_suffix':actual[-3:].hex()}
(root/'child-observation.tmp').write_text(json.dumps(record));(root/'child-observation.tmp').replace(root/'child-observation.json')
os.write(1,b'\\r\\nNATIVE_ENGINE_DONE')
time.sleep(10)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    args = parser.parse_args()
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        parser.error('native host proof requires the pinned Darwin ARM64 lane')
    root = args.root.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    receipt = dict(schema_version=1, state='starting', source_sha=args.source_sha, phases=[],
                   engine_rebuilt=False, default_engine_changed=False,
                   physical_keyboard_delivery=False, full_keyboard_ime_fidelity=False)
    spec = importlib.util.spec_from_file_location('native_engine_runner', REPO / 'tools/terminal-feasibility/run.py')
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    environment = dict(os.environ)

    def run(command, label, timeout=30):
        result = runner.run(command, label, output, receipt['phases'], timeout=timeout,
                            cwd=REPO, env=environment)
        phase = receipt['phases'][-1]
        if phase['log_bytes_seen'] != phase['log_bytes_retained']:
            raise RuntimeError('native host log truncated: ' + label)
        return result.strip()

    try:
        if run(['git', 'rev-parse', 'HEAD'], 'source') != args.source_sha:
            raise RuntimeError('native host source differs from the frozen caller')
        receipt['artifact_acceptance'] = json.loads(run(
            [sys.executable, str(HERE / 'accept-linkage.py'), '--target',
             'aarch64-apple-darwin', '--root', str(root)], 'accept-explicit-root'))
        run([sys.executable, 'scripts/build-macos-dev', '--architecture', 'host',
             '--with-ghostty', '--ghostty-artifact-root', str(root)], 'build-native-host', 600)
        archive = REPO / 'dist' / ('zmux-dev-arm64-' + args.source_sha[:7] + '.zip')
        if archive.stat().st_size > 64 * 1024 * 1024:
            raise RuntimeError('native archive exceeds its retained byte bound')
        shutil.copy2(archive, output / archive.name)
        receipt['app_archive'] = dict(path=archive.name, bytes=archive.stat().st_size,
            sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
        build_info=json.loads((REPO/'dist/build-info.json').read_text())
        if (build_info.get('source_sha') != args.source_sha
                or build_info.get('optional_ghostty_linked') is not True
                or build_info.get('default_terminal_engine') != 'vt100'):
            raise RuntimeError('native build receipt lacks explicit optional linkage/default policy')
        receipt['build_info']=build_info
        shutil.copy2(REPO/'dist/build-info.json',output/'build-info.json')
        child = output / 'native-child.py'
        child.write_text(CHILD)
        child.chmod(0o700)
        # The app owns this exact PTY child. A failed fixture is finite even if
        # its parent dies before ordinary Session cleanup can run.
        environment.update(SHELL=str(child), ZMUX_ENGINE_SMOKE_ROOT=str(output),
                           ZMUX_SOCKET=str(output / 'control.socket'),
                           ZMUX_TERMINAL_ENGINE=json.dumps(dict(engine='ghostty',
                               engine_bytes=16*1024*1024, general_bytes=16*1024*1024,
                               page_bytes=16*1024*1024, reply_timeout_ms=1000)))
        run([str(REPO / 'dist/zmux.app/Contents/MacOS/zmux-app'),
             '--terminal-engine-smoke', str(output / 'native-report.json')],
            'execute-native-host', 30)
        report = json.loads((output / 'native-report.json').read_text())
        observation = json.loads((output / 'child-observation.json').read_text())
        if (not report.get('success') or report.get('source_sha') != args.source_sha
                or report.get('child_observation') != observation
                or observation.get('query_reply') != '1b5b313b3152'
                or observation.get('special_key_prefix') != '1b4f411b4f41'
                or observation.get('whole_paste_frames') != 1
                or observation.get('raw_commit_suffix') != '454e44'
                or observation.get('input_bytes') != 16405):
            raise RuntimeError('native host lacks the required exact child evidence')
        receipt['native_report'] = report
        receipt['child_observation'] = observation
        receipt['state'] = 'complete'
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(state=receipt['state'], receipt=str(output / 'receipt.json'))))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
