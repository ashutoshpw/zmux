#!/usr/bin/env python3
"""Retain exact-source raw child evidence and real Session shutdown test outcomes."""
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


def source_files():
    paths = {REPO / name for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
             'scripts/process_group.py', 'tools/terminal-feasibility/run.py',
             '.github/workflows/session-shutdown-diagnostics.yml')}
    for name in ('crates/session-runtime', 'crates/terminal-adapter', 'vendor/darwin-libproc',
                 'tools/session-shutdown-diagnostics'):
        paths.update(p for p in (REPO / name).rglob('*') if p.is_file() and '__pycache__' not in p.parts)
    result = {}
    for path in sorted(paths):
        data = path.read_bytes()
        if len(data) > 2 * 1024 * 1024:
            raise RuntimeError('diagnostic source exceeds bound: ' + str(path))
        result[str(path.relative_to(REPO))] = hashlib.sha256(data).hexdigest()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check-inputs', action='store_true')
    args = parser.parse_args()
    frozen = source_files()
    if args.check_inputs:
        print(json.dumps(dict(source_entries=len(frozen), execution=False, acceptance_claimed=False)))
        return 0
    if args.output is None:
        parser.error('--output is required')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    receipt = dict(state='starting', system=platform.system(), release=platform.release(),
                   machine=platform.machine(), source_sha256=frozen, phases=[], errors=[],
                   acceptance_claimed=False, product_defaults_changed=False)
    try:
        for name in frozen:
            destination = output / 'source' / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, destination)
            if hashlib.sha256(destination.read_bytes()).hexdigest() != frozen[name]:
                raise RuntimeError('diagnostic source copy changed: ' + name)
        spec = importlib.util.spec_from_file_location('shutdown_diagnostic_runner', REPO / 'tools/terminal-feasibility/run.py')
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        environment = {k: v for k, v in os.environ.items() if not k.startswith('DYLD_')
                       and k not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'ZMUX_CLEANUP_OUTCOME_CHILD')}
        environment['CARGO_TARGET_DIR'] = str(output / 'work/cargo')
        environment['MACOSX_DEPLOYMENT_TARGET'] = '14.0'

        def run(command, label, timeout=30):
            text = runner.run(command, label, output, receipt['phases'], timeout=timeout,
                              cwd=REPO, env=environment)
            phase = receipt['phases'][-1]
            if phase['log_bytes_seen'] != phase['log_bytes_retained']:
                raise RuntimeError('diagnostic log was truncated: ' + label)
            return text.strip()

        receipt['source_sha'] = run(['git', 'rev-parse', 'HEAD'], 'source-sha')
        receipt['rust_version'] = run(['rustc', '--version'], 'rust-version')
        if not receipt['rust_version'].startswith('rustc 1.95.0 '):
            raise RuntimeError('diagnostic Rust differs from pinned version')
        commands = [([sys.executable, '-B', str(HERE / 'probe.py')], 'raw-child-probe', 15),
                    (['cargo', 'test', '--locked', '-p', 'zmux-session-runtime', '--test',
                      'shutdown', '--', '--show-output'], 'shutdown-parallel', 120),
                    (['cargo', 'test', '--locked', '-p', 'zmux-session-runtime', '--test',
                      'shutdown', '--', '--test-threads=1', '--show-output'], 'shutdown-serial', 30)]
        for command, label, timeout in commands:
            try:
                result = run(command, label, timeout)
                if label == 'raw-child-probe':
                    receipt['raw_child_observations'] = json.loads(result)
            except Exception as error:
                receipt['errors'].append(dict(phase=label, error=str(error)))
        if source_files() != frozen:
            raise RuntimeError('diagnostic source changed during execution')
        receipt['state'] = 'failed' if receipt['errors'] else 'complete'
    except Exception as error:
        receipt['state'] = 'failed'
        receipt['errors'].append(dict(phase='driver', error=str(error)))
    finally:
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(state=receipt['state'], receipt=str(output / 'receipt.json'))))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
