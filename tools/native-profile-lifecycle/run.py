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
import subprocess
import sys
import threading
import uuid
import zipfile

REPO = Path(__file__).resolve().parents[2]
INPUTS = ("tools/native-profile-lifecycle/Probe.swift", "tools/native-profile-lifecycle/run.py",
          ".github/workflows/profile-lifecycle-probe.yml", "tools/terminal-feasibility/run.py",
          "scripts/process_group.py", "LICENSE")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
               "source_sha256": frozen, "nonce": nonce, "bundle_id": bundle_id,
               "profile_a": profile_a, "profile_b": profile_b,
               "platform": platform.platform(), "architecture": platform.machine(), "phases": [],
               "production_clear_delete_admitted": False}
    spec = importlib.util.spec_from_file_location("profile_lifecycle_owned_runner", REPO / "tools/terminal-feasibility/run.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    owned_groups = []
    group_factory = runner.ProcessGroup

    def record_group(command, **options):
        owner = group_factory(command, **options)
        owned_groups.append({"pid": owner.process.pid, "command": [str(item) for item in command]})
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
        runner.run([str(executable), str(root / "native-profile-lifecycle.json"), fixture,
                    nonce, profile_a, profile_b], "native-profile-lifecycle", root,
                   receipt["phases"], timeout=45, cwd=REPO)
        report = json.loads((root / "native-profile-lifecycle.json").read_text())
        assert report["appPID"] == owned_groups[-1]["pid"]
        assert report["sourceSHA"] == source and report["bundleID"] == bundle_id and report["nonce"] == nonce
        assert report["profileA"].lower() == profile_a and report["profileB"].lower() == profile_b
        assert report["state"] == "complete" and report["stage"] == "public_cleanup_complete"
        assert all(report[key] for key in ("mountedA", "aViewReleased", "heldDataRetained", "reopenedAEmpty", "separateBRetained"))
        assert [item["phase"] for item in report["removals"]] == ["mounted_a_remove", "released_a_remove", "cleanup_a", "cleanup_b"]
        assert all(item["callbackReceived"] for item in report["removals"])
        assert report["removals"][0].get("errorDomain")
        assert all(not item.get("errorDomain") for item in report["removals"][1:])
        assert frozen == {path: digest(REPO / path) for path in INPUTS}
        receipt["native_report"] = report
        receipt["state"] = "complete"
    except Exception as error:
        receipt.update(state="failed", error=f"{type(error).__name__}: {error}")
        report_path = root / "native-profile-lifecycle.json"
        if report_path.exists():
            receipt["partial_native_report"] = json.loads(report_path.read_text())
            receipt["removal_outcome_unknown"] = receipt["partial_native_report"]["stage"].endswith("_callback_pending")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        if thread.is_alive():
            receipt.update(state="failed", error="Owned fixture server did not join")
        if any(phase["log_bytes_seen"] != phase["log_bytes_retained"] for phase in receipt["phases"]):
            receipt.update(state="failed", error="Probe log exceeded retained bound")
        (root / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"state": receipt["state"], "receipt": str(root / "receipt.json")}))
    return 0 if receipt["state"] == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
