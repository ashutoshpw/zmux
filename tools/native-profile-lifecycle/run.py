#!/usr/bin/env python3
"""Finite original public WebKit lifecycle probe; no user stores or production actions."""
from pathlib import Path
import hashlib
import http.server
import importlib.util
import json
import os
import platform
import plistlib
import selectors
import subprocess
import sys
import threading
import time
import uuid
import zipfile

REPO = Path(__file__).resolve().parents[2]
INPUTS = ("tools/native-profile-lifecycle/Probe.swift", "tools/native-profile-lifecycle/run.py",
          ".github/workflows/profile-lifecycle-probe.yml", "tools/terminal-feasibility/run.py",
          "scripts/process_group.py", "LICENSE")
SCENARIO = "baseline"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_creation_path(report, profile_a, profile_b, prepared=None):
    assert report['aNativeCookieRetained'] and report['bNativeCookieRetained']
    expected_sets = ([{'profile': profile_a.upper(), 'value': 'A', 'callbackReceived': True},
                      {'profile': profile_b.upper(), 'value': 'B', 'callbackReceived': True}]
                     if prepared is None else [])
    assert report['nativeCookieSets'] == expected_sets
    for key, value in (('aCookieStoreObservation', 'A'), ('bCookieStoreObservation', 'B')):
        cookies = report[key]
        assert len(cookies) == 2 and {row['name'] for row in cookies} == {'probe', 'native_created'}
        by_name = {row['name']: row for row in cookies}
        for row in cookies:
            assert row['value'] == value and row['domain'] == '127.0.0.1' and row['path'] == '/'
            assert not row['sessionOnly'] and not row['secure'] and not row['httpOnly']
            assert isinstance(row['expiresAt'], (int, float)) and row['expiresAt'] > time.time()
        assert abs(by_name['probe']['expiresAt'] - by_name['native_created']['expiresAt']) < 1
        if prepared is not None:
            previous = {row['name']: row for row in prepared[key]}
            for row in cookies:
                assert abs(row['expiresAt'] - previous[row['name']]['expiresAt']) < 1


def main():
    if sys.platform != "darwin":
        raise SystemExit("Public WebKit lifecycle feasibility requires macOS")
    root = Path(os.environ["PROBE_OUTPUT"]).resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    frozen = {path: digest(REPO / path) for path in INPUTS}
    for path in INPUTS:
        target = root / "source" / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((REPO / path).read_bytes())
    nonce = str(uuid.uuid4())
    profile_a, profile_b = str(uuid.uuid4()), str(uuid.uuid4())
    bundle_id = "app.w3dev.zmux.profile-lifecycle-probe." + uuid.uuid4().hex
    receipt = {"schema_version": 1, "state": "starting", "source_sha": source,
               "scenario": SCENARIO,
               "termination": "nsapplication_terminate",
               "cookie_creation_discriminator": "js_probe_and_acknowledged_native_created",
               "set_cookie_callback_is_flush_receipt": False,
               "source_sha256": frozen, "nonce": nonce, "bundle_id": bundle_id,
               "profile_a": profile_a, "profile_b": profile_b,
               "platform": platform.platform(), "architecture": platform.machine(), "phases": [],
               "production_clear_delete_admitted": False}
    spec = importlib.util.spec_from_file_location("profile_lifecycle_owned_runner", REPO / "tools/terminal-feasibility/run.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    owned_groups = []
    owned_processes = []
    group_factory = runner.ProcessGroup
    receipt["process_observations"] = []

    def observe_processes(stage, app_pid=None, leader_exited=None):
        observation = {"stage": stage, "app_pid": app_pid, "leader_exited": leader_exited,
                       "scope": "read_only_relationships_not_signal_authority"}
        probe = None
        selector = selectors.DefaultSelector()
        try:
            if len(receipt["process_observations"]) >= 12:
                raise RuntimeError("Process observation count exceeds bound")
            probe = subprocess.Popen(["/bin/ps", "-A", "-o", "pid=,ppid=,pgid=,stat=,comm="],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            selector.register(probe.stdout, selectors.EVENT_READ)
            data = bytearray()
            deadline = time.monotonic() + 2
            while selector.get_map():
                if time.monotonic() >= deadline:
                    raise RuntimeError("Read-only process observation timed out")
                for key, _ in selector.select(0.03):
                    chunk = os.read(key.fileobj.fileno(), 16384)
                    if not chunk:
                        selector.unregister(key.fileobj)
                    if len(data) + len(chunk) > 262144:
                        raise RuntimeError("Read-only process observation exceeds byte bound")
                    data.extend(chunk)
            if probe.wait(timeout=max(0.01, deadline - time.monotonic())) != 0:
                raise RuntimeError("Read-only process observation failed")
            rows = []
            for line in data.decode("utf-8", errors="strict").splitlines():
                fields = line.split(maxsplit=4)
                if len(fields) != 5 or len(rows) >= 4096:
                    raise RuntimeError("Read-only process observation row/type bound failed")
                pid, parent, group = map(int, fields[:3])
                rows.append({"pid": pid, "ppid": parent, "pgid": group, "state": fields[3], "comm": fields[4][:1024]})
            descendants = {app_pid} if app_pid is not None else set()
            for _ in range(len(rows)):
                found = {row["pid"] for row in rows if row["ppid"] in descendants}
                if found <= descendants:
                    break
                descendants |= found
            retained = []
            for row in rows:
                relationships = []
                if app_pid is not None and row["pgid"] == app_pid:
                    relationships.append("observed_owned_group")
                if row["pid"] in descendants:
                    relationships.append("observed_app_or_descendant")
                if "webkit" in row["comm"].lower():
                    relationships.append("observed_webkit_candidate_relationship_unproven")
                if relationships:
                    retained.append(dict(row, relationships=relationships))
            observation["processes"] = retained
        except Exception as error:
            observation["error"] = f"{type(error).__name__}: {error}"[:2048]
        finally:
            selector.close()
            if probe is not None:
                try:
                    if probe.stdout is not None:
                        probe.stdout.close()
                    if probe.returncode is None:
                        # Only this unreaped direct ps child is stopped, never an observed PID.
                        probe.kill()
                    probe.wait(timeout=2)
                except Exception as error:
                    observation["cleanup_error"] = f"{type(error).__name__}: {error}"[:2048]
            receipt["process_observations"].append(observation)

    def record_group(command, **options):
        native = Path(str(command[0])).name == "profile-lifecycle-probe"
        if native:
            observe_processes("before_native_spawn")
        owner = group_factory(command, **options)
        owned_groups.append({"pid": owner.process.pid, "command": [str(item) for item in command]})
        owned_processes.append(owner.process)
        if native:
            original_stop, original_close = owner.stop, owner.close

            def observed_stop():
                try:
                    observe_processes("before_owned_cleanup_signal", owner.process.pid, owner.exited())
                except Exception as error:
                    receipt["process_observations"].append({"stage": "before_owned_cleanup_signal",
                        "app_pid": owner.process.pid, "scope": "read_only_relationships_not_signal_authority",
                        "error": f"{type(error).__name__}: {error}"[:2048]})
                return original_stop()

            def observed_close(**options):
                try:
                    return original_close(**options)
                finally:
                    observe_processes("after_owned_cleanup", owner.process.pid)

            owner.stop = observed_stop
            owner.close = observed_close
        return owner

    # Delegate the unchanged lifecycle owner; retain its actual spawned identity.
    runner.ProcessGroup = record_group
    receipt["spawned_process_groups"] = owned_groups
    content = ("<!doctype html><title>Isolated lifecycle " + nonce + "</title><p>Disposable profile fixture</p>").encode()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path != "/fixture/" + nonce:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(content)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(content)

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        bundle = root / "LifecycleProbe.app/Contents"
        executable = bundle / "MacOS/profile-lifecycle-probe"
        executable.parent.mkdir(parents=True)
        info = {"CFBundleIdentifier": bundle_id, "CFBundleExecutable": executable.name,
                "CFBundleName": "zmux isolated lifecycle probe", "CFBundlePackageType": "APPL",
                "NSPrincipalClass": "NSApplication", "LSMinimumSystemVersion": "14.0",
                "NSAppTransportSecurity": {"NSAllowsArbitraryLoadsInWebContent": True},
                "ProbeSourceSHA": source}
        with (bundle / "Info.plist").open("wb") as output:
            plistlib.dump(info, output)
        runner.run(["xcrun", "swiftc", "-parse-as-library", "-swift-version", "6",
                    "-strict-concurrency=complete", "-warnings-as-errors", "-O", "-g",
                    "-target", f"{platform.machine()}-apple-macosx14.0",
                    str(REPO / "tools/native-profile-lifecycle/Probe.swift"), "-framework", "AppKit",
                    "-framework", "WebKit", "-o", str(executable)],
                   "build", root, receipt["phases"], timeout=60, cwd=REPO)
        runner.run(["codesign", "--force", "--sign", "-", str(bundle.parent)],
                   "sign", root, receipt["phases"], timeout=15, cwd=REPO)
        receipt["executable_sha256"] = digest(executable)
        archive = root / ("profile-lifecycle-" + source[:7] + ".zip")
        files = [path for path in sorted(bundle.parent.rglob("*")) if path.is_file()]
        assert len(files) <= 32 and sum(path.stat().st_size for path in files) <= 32 * 1024 * 1024
        assert not any(path.is_symlink() for path in files)
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for path in files:
                output.write(path, path.relative_to(root))
        receipt["archive"] = {"name": archive.name, "sha256": digest(archive)}
        fixture = f"http://127.0.0.1:{server.server_port}/fixture/{nonce}"
        preparation_path = root / "native-profile-prepare.json"
        preparation_log = runner.run([str(executable), str(preparation_path), fixture,
                    nonce, profile_a, profile_b, "prepare", str(preparation_path), SCENARIO], "native-profile-prepare", root,
                   receipt["phases"], timeout=45, cwd=REPO)
        prepared = json.loads(preparation_path.read_text())

        def validate_identity(report, pid):
            assert report["appPID"] == pid
            assert report["sourceSHA"] == source and report["bundleID"] == bundle_id and report["nonce"] == nonce
            assert report["profileA"].lower() == profile_a and report["profileB"].lower() == profile_b
            assert report["scenario"] == SCENARIO
            assert report["termination"] == receipt["termination"]

        def validate_termination(log, pid, phase):
            prefix = "ZMUX_APPKIT_TERMINATION "
            records = [json.loads(line[len(prefix):]) for line in log.splitlines() if line.startswith(prefix)]
            expected = {"event": "application_will_terminate", "appPID": pid, "phase": phase}
            assert records == [expected]
            return records[0]

        validate_identity(prepared, owned_groups[-1]["pid"])
        receipt["preparation_termination"] = validate_termination(preparation_log, prepared["appPID"], "prepare")
        assert prepared["phase"] == "prepare" and prepared["state"] == "prepared" and prepared["stage"] == "prepared_for_fresh_process"
        assert all(prepared[key] for key in ("mountedA", "aViewReleased", "heldDataRetained", "separateBRetained"))
        if SCENARIO == "baseline":
            assert prepared["removals"] == [] and not prepared.get("aDataRemoval")
            assert prepared["aCookieStoreObservation"] and prepared["bCookieStoreObservation"]
            validate_creation_path(prepared, profile_a, profile_b)
        else:
            assert prepared["reopenedAEmpty"]
            assert prepared["aDataRemoval"]["scope"] == "profile_a_all_website_data"
            assert prepared["aDataRemoval"]["callbackReceived"] and prepared["aDataRemoval"]["dataTypes"]
            assert [item["phase"] for item in prepared["removals"]] == ["mounted_a_remove", "released_a_remove"]
            assert all(item["callbackReceived"] for item in prepared["removals"])
            assert prepared["removals"][0].get("errorDomain") and not prepared["removals"][1].get("errorDomain")
        # The unchanged owned runner closes/reaps each phase before returning.
        # Inspect the retained child handle, never infer exit from a reused PID.
        assert owned_processes[-1].returncode == 0 and owned_processes[-1].poll() == 0
        receipt["preparation_exit"] = {"pid": prepared["appPID"], "return_code": owned_processes[-1].returncode,
                                       "owned_child_reaped_before_reopen": True}
        receipt["native_preparation"] = prepared
        receipt["preparation_sha256"] = digest(preparation_path)
        receipt["same_process_acceptance"] = True
        assert digest(executable) == receipt["executable_sha256"]
        assert digest(archive) == receipt["archive"]["sha256"]
        reopen_log = runner.run([str(executable), str(root / "native-profile-lifecycle.json"), fixture,
                    nonce, profile_a, profile_b, "reopen", str(preparation_path), SCENARIO], "native-profile-reopen", root,
                   receipt["phases"], timeout=45, cwd=REPO)
        report = json.loads((root / "native-profile-lifecycle.json").read_text())
        validate_identity(report, owned_groups[-1]["pid"])
        receipt["reopen_termination"] = validate_termination(reopen_log, report["appPID"], "reopen")
        assert report["appPID"] != prepared["appPID"] and report["preparerPID"] == prepared["appPID"]
        assert report["phase"] == "reopen" and report["state"] == "complete" and report["stage"] == "public_cleanup_complete"
        assert report["separateBRetained"] and report["aViewReleased"]
        if SCENARIO == "baseline":
            assert report["heldDataRetained"]
            validate_creation_path(report, profile_a, profile_b, prepared)
        else:
            assert report["reopenedAEmpty"]
        assert [item["phase"] for item in report["removals"]] == ["cleanup_a", "cleanup_b"]
        assert all(item["callbackReceived"] and not item.get("errorDomain") for item in report["removals"])
        assert owned_processes[-1].returncode == 0 and owned_processes[-1].poll() == 0
        assert digest(preparation_path) == receipt["preparation_sha256"]
        assert digest(executable) == receipt["executable_sha256"] and digest(archive) == receipt["archive"]["sha256"]
        assert frozen == {path: digest(REPO / path) for path in INPUTS}
        receipt["native_report"] = report
        receipt["fresh_process_acceptance"] = True
        receipt["state"] = "complete"
    except Exception as error:
        receipt.update(state="failed", error=f"{type(error).__name__}: {error}")
        receipt["removal_outcome_unknown"] = False
        receipt["cookie_set_outcome_unknown"] = False
        for name, key in (("native-profile-prepare.json", "partial_native_preparation"),
                          ("native-profile-lifecycle.json", "partial_native_report")):
            report_path = root / name
            if report_path.exists():
                receipt[key] = json.loads(report_path.read_text())
                stage = receipt[key]["stage"]
                receipt["cookie_set_outcome_unknown"] |= stage.startswith("native_cookie_") and stage.endswith("_set_callback_pending")
                receipt["removal_outcome_unknown"] |= (stage.endswith("_callback_pending")
                    and not stage.startswith("native_cookie_"))
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        if thread.is_alive():
            receipt.update(state="failed", error="Owned fixture server did not join")
        if any(phase["log_bytes_seen"] != phase["log_bytes_retained"] for phase in receipt["phases"]):
            receipt.update(state="failed", error="Probe log exceeded retained bound")
        if any(item.get("error") or item.get("cleanup_error") for item in receipt["process_observations"]):
            receipt.update(state="failed", process_observation_incomplete=True)
        (root / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"state": receipt["state"], "receipt": str(root / "receipt.json")}))
    return 0 if receipt["state"] == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
