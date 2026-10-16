#!/usr/bin/env python3
"""Observe finite, explicit vt100 native idle phases without product budgets."""
import argparse
import ast
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import selectors
import shutil
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(REPO / 'scripts'))
from process_group import ProcessGroup  # noqa: E402

MAX_LOG_BYTES = 1_048_576
MAX_REPORT_BYTES = 131_072
MAX_RESOURCES_BYTES = 8 * 1024 * 1024
MAX_APP_SECONDS = 1230
# Measurement availability, not a product CPU/RSS/latency budget: allow the
# five-second phase handoff plus bounded ps/lsof/CLI work and the one-second gap.
MAX_OBSERVATION_GAP_SECONDS = 10
CHILD = '''#!/usr/bin/env python3
import json,os,pathlib
root=pathlib.Path(os.environ['ZMUX_IDLE_WORKLOAD_ROOT'])
nonce=os.environ['ZMUX_IDLE_WORKLOAD_NONCE'];pid=os.getpid()
record={'pid':pid,'parent_pid':os.getppid(),'nonce':nonce}
with (root/('child-'+str(pid)+'.json')).open('x') as output: json.dump(record,output)
os.write(1,('IDLE_'+nonce+'_'+str(pid)+'\\r\\n').encode())
# A real interactive shell with no user startup files. TMOUT is its independent
# finite idle lifetime if the app dies before ordinary Session cleanup.
environment={key:value for key,value in os.environ.items() if key in
             ('PATH','TERM','LANG','LC_ALL','LC_CTYPE','TMPDIR','HOME','USER','LOGNAME')}
environment.update(TMOUT='1260',PS1='',PS2='',HISTFILE='/dev/null')
os.execve('/bin/bash',['/bin/bash','--noprofile','--norc','-i'],environment)
'''


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_files():
    roots = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'LICENSE',
             'THIRD_PARTY_NOTICES.md', 'config/deployment.json', 'apps/cli',
             'apps/macos', 'crates', 'vendor', 'scripts/build-macos-dev',
             'scripts/native-resource-sampler.py', 'scripts/process_group.py',
             'tools/terminal-feasibility/run.py', 'tools/native-idle-workloads',
             '.github/workflows/native-idle-workloads.yml']
    names = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others',
                                     '--exclude-standard', '--', *roots], cwd=REPO).decode().split('\0')
    names = sorted(set(name for name in names if name))
    if not 0 < len(names) <= 512:
        raise RuntimeError('Idle workload source inventory exceeds its file bound')
    result = {}
    total = 0
    for name in names:
        path = REPO / name
        if not path.is_file() or path.is_symlink() or path.stat().st_size > 2_097_152:
            raise RuntimeError('Unsupported idle workload source input: ' + name)
        total += path.stat().st_size
        if total > 16 * 1024 * 1024:
            raise RuntimeError('Idle source inventory exceeds its total byte bound')
        result[name] = digest(path)
    return result


def document(path, maximum=MAX_REPORT_BYTES):
    if path.stat().st_size > maximum:
        raise RuntimeError('Idle workload document exceeds its byte bound')
    return json.loads(path.read_text())


def save(path, value):
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')
    temporary.replace(path)


def expected_phases(repetitions):
    return [(repetition, name, stage) for repetition in range(1, repetitions + 1)
            for name in ('idle1', 'idle8') for stage in ('warmup', 'measure', 'complete')]


def validate_journal(value, source, nonce, pid, repetitions):
    if (value.get('schemaVersion') != 1 or value.get('sourceSHA') != source
            or value.get('nonce') != nonce or value.get('appPID') != pid
            or value.get('engine') != 'vt100'
            or value.get('state') not in ('running', 'completed_phases', 'complete')):
        raise RuntimeError('Native idle journal identity/default policy differs')
    phases = value.get('phases', [])
    expected = expected_phases(repetitions)
    if len(phases) > len(expected):
        raise RuntimeError('Native idle journal exceeds its phase bound')
    children = value.get('allChildren', [])
    if not len(children) <= 22 or len({row['processID'] for row in children}) != len(children):
        raise RuntimeError('Native idle child history is oversized or reuses a PID')
    for index, phase in enumerate(phases):
        if ((phase.get('repetition'), phase.get('name'), phase.get('stage')) != expected[index]
                or phase.get('sequence') != index + 1):
            raise RuntimeError('Native idle phase order differs')
        live = phase.get('children', [])
        count = 1 if phase['name'] == 'idle1' else 8
        if (len(live) != count or any(row not in children for row in live)
                or any(len({row[key] for row in live}) != count
                       for key in ('processID', 'sessionID', 'surfaceID'))):
            raise RuntimeError('Native idle phase does not own its exact terminal count')
    if value['state'] == 'complete' and (len(phases) != len(expected)
                                      or len(children) != 1 + repetitions * 7):
        raise RuntimeError('Native idle shutdown completed without all phases')
    return phases


def verify_live_children(sampler, phase, root, nonce, app_pid, deadline):
    wanted = {row['processID'] for row in phase['children']}
    for pid in wanted:
        record = document(root / ('child-' + str(pid) + '.json'))
        if record != {'pid': pid, 'parent_pid': app_pid, 'nonce': nonce}:
            raise RuntimeError('Native idle PTY child differs from its actual registration')
    while time.monotonic() < deadline:
        observed = sampler.latest_observation()
        if (observed is not None and observed['app_pid'] == app_pid
                and observed['sample_wall_unix_seconds'] >= phase['wallUnixSeconds']):
            children = {row['pid']: row for row in observed['processes']
                        if row['parent_pid'] == app_pid}
            if set(children) == wanted:
                return {str(pid): children[pid]['started'] for pid in sorted(wanted)}
        time.sleep(0.05)
    raise RuntimeError('Actual app process tree did not confirm the phase child identities')


def execute_app(command, environment, root, sampler, receipt, source, nonce, repetitions):
    group = ProcessGroup(command, cwd=REPO, env=environment, stdin=subprocess.DEVNULL,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    selector = selectors.DefaultSelector()
    selector.register(group.process.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + MAX_APP_SECONDS
    retained = 0
    consumed = 0
    journal_mtime = None
    report = root / 'native-idle.json'
    identities = {}
    try:
        with (root / 'app.log').open('wb') as log:
            while True:
                if time.monotonic() >= deadline:
                    raise RuntimeError('Native idle app exceeded its fixed outer deadline')
                for key, _ in selector.select(0.1):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    retained += len(data)
                    if retained > MAX_LOG_BYTES:
                        raise RuntimeError('Native idle app log exceeds its byte bound')
                    log.write(data)
                report_mtime = report.stat().st_mtime_ns if report.exists() else None
                if report_mtime is not None and report_mtime != journal_mtime:
                    value = document(report)
                    phases = validate_journal(value, source, nonce, group.process.pid, repetitions)
                    for phase in phases[consumed:]:
                        if phase['stage'] != 'complete':
                            if phase is not phases[-1]:
                                raise RuntimeError('External observer missed an active idle phase transition')
                            observed = verify_live_children(sampler, phase, root, nonce, group.process.pid,
                                                            min(deadline, time.monotonic() + 5))
                            for pid, started in observed.items():
                                if pid in identities and identities[pid] != started:
                                    raise RuntimeError('Idle workload child PID identity changed')
                                identities[pid] = started
                        sampler.set_phase({key: phase[key] for key in ('name', 'repetition', 'stage', 'sequence')})
                        receipt['phase_handoffs'].append({
                            'sequence': phase['sequence'], 'native_wall_unix_seconds': phase['wallUnixSeconds'],
                            'observed_wall_unix_seconds': time.time(),
                            'handoff_wall_ms': (time.time() - phase['wallUnixSeconds']) * 1000,
                        })
                        consumed += 1
                    # Remember the identity observed before reading. An atomic
                    # publication during validation must trigger another read.
                    journal_mtime = report_mtime
                    save(root / 'receipt.json', receipt)
                if group.exited() and not selector.get_map():
                    break
        code = group.close(timeout=3)
        receipt['app_exit_code'] = code
        if code != 0:
            raise RuntimeError('Native idle app failed; see retained app log/journal')
        value = document(report)
        validate_journal(value, source, nonce, group.process.pid, repetitions)
        if value['state'] != 'complete':
            raise RuntimeError('Native idle app did not confirm shared shutdown')
        # The retained app was the exact parent. A reused PID is not followed or signalled.
        processes = sampler.process_identities()
        if any(int(pid) in processes and processes[int(pid)]['started'] == started
               for pid, started in identities.items()):
            raise RuntimeError('Native idle app shutdown left an observed child identity alive')
        receipt['native_report'] = value
        receipt['observed_child_start_identities'] = identities
        receipt['shared_shutdown_confirmed'] = True
    finally:
        selector.close()
        if not group.closed:
            group.close(timeout=3)


def summarize(resources, repetitions, native, identities):
    result = []
    for repetition in range(1, repetitions + 1):
        for name in ('idle1', 'idle8'):
            measure = next(phase for phase in native['phases'] if phase['name'] == name
                           and phase['repetition'] == repetition and phase['stage'] == 'measure')
            complete = next(phase for phase in native['phases'] if phase['name'] == name
                            and phase['repetition'] == repetition and phase['stage'] == 'complete')
            wanted = {row['processID'] for row in measure['children']}
            observations = [row for row in resources['samples']
                    if row.get('phase', {}).get('name') == name
                    and row['phase']['repetition'] == repetition and row['phase']['stage'] == 'measure'
                    and row['sample_wall_unix_seconds'] >= measure['wallUnixSeconds']
                    and row['observer_finished_wall_unix_seconds'] < complete['wallUnixSeconds']]
            rows = [row for row in observations if row['state'] == 'available']
            if any({process['pid'] for process in row['processes']
                    if process['parent_pid'] == native['appPID']} != wanted for row in rows):
                raise RuntimeError('Measured idle phase lost or gained an actual terminal child')
            if any(process['started'] != identities[str(process['pid'])]
                   for row in rows for process in row['processes']
                   if process['parent_pid'] == native['appPID']):
                raise RuntimeError('Measured idle phase changed an exact terminal child start identity')
            unavailable = [row for row in observations if row['state'] != 'available']
            reasons = {}
            for row in unavailable:
                reason = row.get('reason', row['state'])
                reasons[reason] = reasons.get(reason, 0) + 1
            starts = [row['sample_wall_unix_seconds'] for row in observations]
            gaps = ([starts[0] - measure['wallUnixSeconds']]
                    + [later - earlier for earlier, later in zip(starts, starts[1:])]
                    + [complete['wallUnixSeconds'] - observations[-1]['observer_finished_wall_unix_seconds']]
                    if observations else [complete['wallUnixSeconds'] - measure['wallUnixSeconds']])
            maximum_gap = max(gaps)
            coverage_complete = (len(rows) >= 2 and not unavailable
                                 and all(0 <= gap <= MAX_OBSERVATION_GAP_SECONDS for gap in gaps))
            cli = [row['cli_status']['elapsed_ms'] for row in rows
                   if row.get('cli_status', {}).get('state') == 'available']
            elapsed = rows[-1]['elapsed_seconds'] - rows[0]['elapsed_seconds'] if len(rows) >= 2 else 0
            cpu = rows[-1]['app_cpu_seconds'] - rows[0]['app_cpu_seconds'] if len(rows) >= 2 else 0
            result.append({'name': name, 'repetition': repetition,
                           'state': 'complete' if coverage_complete else 'partial',
                           'measurement_window_seconds': complete['wallUnixSeconds'] - measure['wallUnixSeconds'],
                           'observations': len(observations), 'available_samples': len(rows),
                           'unavailable_samples': len(unavailable), 'unavailable_reasons': reasons,
                           'maximum_observation_gap_seconds': maximum_gap,
                           'maximum_allowed_observation_gap_seconds': MAX_OBSERVATION_GAP_SECONDS,
                           'observed_interval_seconds': elapsed,
                           'app_cpu_percent_over_observed_interval': 100 * cpu / elapsed if elapsed > 0 and cpu >= 0 else None,
                           'app_rss_kib_range': [min(row['app_rss_kib'] for row in rows), max(row['app_rss_kib'] for row in rows)] if rows else None,
                           'current_tree_rss_kib_range': [min(row['current_tree_rss_kib'] for row in rows), max(row['current_tree_rss_kib'] for row in rows)] if rows else None,
                           'available_cli_status_probes': len(cli),
                           'cli_status_ms_range': [min(cli), max(cli)] if cli else None})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--repetitions', type=int, choices=(1, 2, 3), default=1)
    parser.add_argument('--check-inputs', action='store_true')
    args = parser.parse_args()
    frozen = source_files()
    ast.parse(CHILD)
    if args.check_inputs:
        print(json.dumps({'sources': len(frozen), 'repetitions': args.repetitions,
                          'warmup_seconds': 30, 'measure_seconds': 120, 'execution': False}))
        return 0
    if platform.system() != 'Darwin' or platform.machine() != 'arm64' or args.output is None:
        parser.error('Idle workload execution requires Darwin ARM64 and --output')
    root = args.output.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip()
    nonce = str(uuid.uuid4())
    receipt = {'schema_version': 1, 'state': 'starting', 'source_sha': source, 'source_sha256': frozen,
               'default_engine_changed': False, 'engine': 'vt100', 'performance_budgets_accepted': False,
               'repetitions': args.repetitions, 'warmup_seconds': 30, 'measure_seconds': 120,
               'app_outer_deadline_seconds': MAX_APP_SECONDS, 'phase_handoffs': [], 'phases': [],
               'limits': ['Named CI observations, not dedicated physical-Mac budgets',
                          'RSS is not physical footprint; PPID tree excludes detached helpers',
                          'CLI timing includes process launch; no physical input/display or main-actor latency',
                          'Wall-clock phase handoff timing can be affected by clock adjustment']}
    environment = {k: v for k, v in os.environ.items() if not k.startswith(('DYLD_', 'ZMUX_'))
                   and k not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')}
    environment.update(CARGO_TARGET_DIR=str(root / 'work/cargo'),
                       ZMUX_IDLE_WORKLOAD_ROOT=str(root), ZMUX_IDLE_WORKLOAD_NONCE=nonce,
                       ZMUX_SOCKET=str(root / 'control.socket'), ZMUX_SOCKET_PATH=str(root / 'control.socket'))
    sampler = None
    sampler_stopped = False
    try:
        for name in frozen:
            target = root / 'source' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / name, target)
            if digest(target) != frozen[name]:
                raise RuntimeError('Idle retained source differs')
        runner = load_module('idle_build_runner', REPO / 'tools/terminal-feasibility/run.py')
        runner.run([sys.executable, 'scripts/build-macos-dev', '--architecture', 'host'],
                   'build', root, receipt['phases'], timeout=600, cwd=REPO, env=environment)
        if any(p['log_bytes_seen'] != p['log_bytes_retained'] for p in receipt['phases']):
            raise RuntimeError('Idle native build log was truncated')
        info = document(REPO / 'dist/build-info.json')
        if info['source_sha'] != source or info['optional_ghostty_linked'] or info['default_terminal_engine'] != 'vt100':
            raise RuntimeError('Idle baseline build/default policy differs')
        archive = REPO / 'dist' / ('zmux-dev-arm64-' + source[:7] + '.zip')
        if archive.stat().st_size > 64 * 1024 * 1024:
            raise RuntimeError('Idle archive exceeds its byte bound')
        shutil.copy2(archive, root / archive.name)
        receipt['archive'] = {'name': archive.name, 'bytes': archive.stat().st_size, 'sha256': digest(archive)}
        receipt['build_info'] = info
        save(root / 'configuration.json', {'sourceSHA': source, 'nonce': nonce, 'repetitions': args.repetitions})
        child = root / 'idle-child.py'
        child.write_text(CHILD)
        child.chmod(0o700)
        environment['SHELL'] = str(child)
        module = load_module('idle_resource_sampler', REPO / 'scripts/native-resource-sampler.py')
        sampler = module.NativeResourceSampler(REPO / 'dist/zmux.app/Contents/MacOS/zmux-app',
                    REPO / 'dist/zmux.app/Contents/Resources/bin/zmux', root / 'resources.json', source,
                    maximum_samples=1250, maximum_duration=1250, maximum_descendants=9,
                    probe_environment=environment)
        # The sampler inherits this process's environment, separately from app launch.
        sampler.socket = root / 'control.socket'
        sampler.start()
        execute_app([str(REPO / 'dist/zmux.app/Contents/MacOS/zmux-app'), '--idle-workload-smoke',
                     str(root / 'native-idle.json')], environment, root, sampler, receipt, source, nonce, args.repetitions)
        sampler.stop()
        sampler_stopped = True
        resources = document(root / 'resources.json', MAX_RESOURCES_BYTES)
        if resources['source_sha'] != source:
            raise RuntimeError('Idle resource source identity differs')
        receipt['resource_state'] = resources['state']
        receipt['measurements'] = summarize(resources, args.repetitions, receipt['native_report'],
                                             receipt['observed_child_start_identities'])
        if source_files() != frozen:
            raise RuntimeError('Idle source inputs changed during the run')
        receipt['state'] = ('complete' if resources['state'] == 'complete'
                            and all(row['state'] == 'complete' for row in receipt['measurements']) else 'partial')
    except Exception as error:
        receipt.update(state='failed', error=f'{type(error).__name__}: {error}')
    finally:
        if sampler is not None and not sampler_stopped:
            sampler.stop()
        save(root / 'receipt.json', receipt)
    print(json.dumps({'state': receipt['state'], 'receipt': str(root / 'receipt.json')}))
    return 0 if receipt['state'] == 'complete' else 1


if __name__ == '__main__':
    sys.exit(main())
