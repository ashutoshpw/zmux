#!/usr/bin/env python3
"""Build and observe an isolated public-AppKit/WebKit keyboard feasibility app.

Never posts global input, changes TCC, launches Zmux/cmux, or touches an existing
app. The default requires real macOS; --check validates portable runner inputs.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'scripts'))
from browser_process import run_browser  # noqa: E402

CASES = ['text', 'backspace', 'left', 'space', 'enter', 'down_up',
         'modifier_release', 'unhandled_command']


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inputs():
    names = [HERE / 'Probe.swift', HERE / 'run.py', HERE / 'README.md',
             ROOT / 'scripts/browser_process.py', ROOT / 'scripts/process_group.py',
             ROOT / '.github/workflows/browser-keyboard-feasibility.yml']
    return {str(path.relative_to(ROOT)): digest(path) for path in names}


def validate_report(report, source_sha):
    """Validate actual receipt shape; never supplies observed native success."""
    if report.get('schemaVersion') != 1 or report.get('sourceSHA') != source_sha:
        raise RuntimeError('Native receipt version/source differs from the compiled probe')
    if report.get('productionAccepted') is not False:
        raise RuntimeError('Feasibility receipt must not claim production acceptance')
    if report.get('state') != 'completed_feasibility_observation':
        raise RuntimeError('Native probe did not complete: ' + str(report.get('error')))
    rows = report.get('cases', [])
    if len(rows) != 16 or {(row.get('mode'), row.get('name')) for row in rows} != {
            (mode, name) for mode in ['visible', 'detached'] for name in CASES}:
        raise RuntimeError('Native probe omitted/duplicated a requested real case')
    if any(type(row.get('passed')) is not bool or type(row.get('focusRetained')) is not bool for row in rows):
        raise RuntimeError('Native case result/focus fields are not Boolean observations')
    if any(row.get('passed') for row in rows) and (
            report.get('activationError') is not None
            or report.get('activationRequestProcessID') != report.get('processID')):
        raise RuntimeError('Successful native case lacks exact-self Launch Services activation')
    if any(row.get('passed') and not row.get('focusRetained') for row in rows):
        raise RuntimeError('A successful native case omitted unrelated responder retention')
    detached = [row for row in rows if row['mode'] == 'detached']
    for row in detached:
        for stage in ['nativeBefore', 'nativeAfter']:
            context = row.get(stage)
            if context is not None and context.get('targetWindow') is not None:
                raise RuntimeError('Detached case acquired a window; hosting no longer matches production')
    for mode in ['visible', 'detached']:
        observed = all(row['passed'] for row in rows if row['mode'] == mode and row['name'] != 'unhandled_command')
        if report.get(mode + 'DefaultsVerified') is not observed:
            raise RuntimeError('Native defaults summary differs from actual case observations')
    async_observed = any(row.get('duringDirectDelivery') is False
                         and row.get('originPhase') in ['visible/unhandled_command/dispatch',
                                                       'detached/unhandled_command/dispatch']
                         and row.get('eventType') == 10 and row.get('keyCode') == 13
                         and row.get('command') is True for row in report.get('resends', []))
    contained = (async_observed and report.get('escapedShortcuts') == 0
                 and report.get('resendLogLimitReached') is False)
    if report.get('asyncResendObserved') is not async_observed or report.get('asyncResendContained') is not contained:
        raise RuntimeError('Native resend summary lacks actual asynchronous unhandled Command-W evidence')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    source_inputs = inputs()
    if args.check:
        print(json.dumps({'state': 'portable_inputs_verified', 'native_executed': False,
                          'source_sha256': source_inputs}, indent=2))
        return 0
    if platform.system() != 'Darwin':
        parser.error('Actual AppKit/WebKit execution requires macOS; --check is portable only')
    if args.output is None:
        parser.error('--output is required for actual execution')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_sha = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    receipt = {'schema_version': 1, 'source_sha': source_sha, 'source_sha256': source_inputs,
               'state': 'building', 'native_executed': False, 'production_accepted': False,
               'phases': [], 'architecture': platform.machine(),
               'scope': 'One new isolated probe app; no global input/TCC/production-app operations.'}

    def save():
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')

    def run(label, command, timeout=120):
        save()
        start = time.monotonic()
        result = run_browser(command, timeout=timeout)
        (output / (label + '.stdout.log')).write_text(result.stdout)
        (output / (label + '.stderr.log')).write_text(result.stderr)
        receipt['phases'].append({'label': label, 'exit_code': result.returncode,
                                  'elapsed_seconds': round(time.monotonic()-start, 3),
                                  'timed_out': result.timed_out,
                                  'output_truncated': result.output_truncated,
                                  'stdout_bytes_seen': result.stdout_bytes_seen,
                                  'stderr_bytes_seen': result.stderr_bytes_seen})
        save()
        if result.returncode or result.timed_out or result.output_truncated:
            raise RuntimeError(f'{label} failed; see retained bounded logs and partial native receipt')
        return result.stdout.strip()

    try:
        architecture = platform.machine()
        if architecture not in ['arm64', 'x86_64']:
            raise RuntimeError('Unsupported native probe architecture: ' + architecture)
        sdk = run('sdk', ['xcrun', '--sdk', 'macosx', '--show-sdk-path'], timeout=10)
        run('swift-version', ['xcrun', 'swiftc', '--version'], timeout=10)
        bundle = output / 'BrowserKeyboardProbe.app'
        executable = bundle / 'Contents/MacOS/browser-keyboard-probe'
        executable.parent.mkdir(parents=True)
        identifier = 'app.zmux.keyboard-probe.' + uuid.uuid4().hex
        plist = {'CFBundleIdentifier': identifier, 'CFBundleName': 'Zmux Keyboard Probe',
                 'CFBundleExecutable': executable.name, 'CFBundlePackageType': 'APPL',
                 'CFBundleVersion': '1', 'CFBundleShortVersionString': '1',
                 'NSPrincipalClass': 'ZmuxKeyboardProbeApplication',
                 'LSMinimumSystemVersion': '14.0', 'NSHighResolutionCapable': True}
        with (bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump(plist, stream)
        run('compile', ['xcrun', 'swiftc', '-parse-as-library', '-module-name', 'BrowserKeyboardProbe',
                        '-swift-version', '6', '-strict-concurrency=complete', '-warnings-as-errors',
                        '-Onone', '-g', '-target', architecture+'-apple-macosx14.0', '-sdk', sdk,
                        str(HERE / 'Probe.swift'), '-framework', 'AppKit', '-framework', 'WebKit',
                        '-framework', 'CoreGraphics', '-o', str(executable)])
        run('codesign', ['codesign', '--force', '--sign', '-', str(bundle)], timeout=15)
        receipt['executable_sha256'] = digest(executable)
        receipt['bundle_identifier'] = identifier
        receipt['state'] = 'running'
        save()
        # Environment is scoped to this child only. No host input state is changed.
        previous_sha = os.environ.get('ZMUX_KEYBOARD_SOURCE_SHA')
        previous_arch = os.environ.get('ZMUX_KEYBOARD_ARCH')
        os.environ['ZMUX_KEYBOARD_SOURCE_SHA'] = source_sha
        os.environ['ZMUX_KEYBOARD_ARCH'] = architecture
        try:
            run('native', [str(executable), '--output', str(output / 'native.json')], timeout=60)
        finally:
            for key, previous in [('ZMUX_KEYBOARD_SOURCE_SHA', previous_sha),
                                  ('ZMUX_KEYBOARD_ARCH', previous_arch)]:
                if previous is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = previous
        native_file = output / 'native.json'
        if native_file.stat().st_size > 1_048_576:
            raise RuntimeError('Native receipt exceeds 1 MiB')
        native = json.loads(native_file.read_text())
        rows = validate_report(native, source_sha)
        receipt['native_executed'] = True
        receipt['native_receipt_sha256'] = digest(native_file)
        receipt['visible_defaults_verified'] = native['visibleDefaultsVerified']
        receipt['detached_defaults_verified'] = native['detachedDefaultsVerified']
        receipt['async_resend_observed'] = native['asyncResendObserved']
        receipt['async_resend_contained'] = native['asyncResendContained']
        receipt['failed_cases'] = [{'mode': row['mode'], 'name': row['name'], 'error': row.get('error')}
                                   for row in rows if not row.get('passed')]
        receipt['state'] = 'observed_contracts_passed' if (
            all(row.get('passed') for row in rows) and native['asyncResendContained']) else 'native_contract_gated'
        receipt['remaining_gate'] = None if receipt['state'] == 'observed_contracts_passed' else (
            'Inspect actual case evidence. Detached native defaults or asynchronous resend containment '
            'were not established; architecture work is required before any production/parity claim.')
    except Exception as error:
        receipt['state'] = 'probe_failed'
        receipt['error'] = str(error)
    finally:
        save()
    print(json.dumps(receipt, indent=2))
    return 0 if receipt['state'] == 'observed_contracts_passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
