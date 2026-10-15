"""Best-effort external telemetry for one packaged macOS acceptance journey.

Never changes the app or fixture processes. Only bounded ps/lsof/status probes run;
process arguments, terminal contents, credentials and probe output are not recorded.
"""
import ctypes
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import threading
import time


INTERVAL_SECONDS = 1.0
PROBE_TIMEOUT_SECONDS = 1.0
MAX_SAMPLES = 150
MAX_DURATION_SECONDS = 155.0
MAX_PROCESSES = 8192
MAX_DESCENDANTS = 256
JOIN_TIMEOUT_SECONDS = 5.0


class ResourceUnavailable(Exception):
    pass


def cpu_seconds(value):
    days, clock = value.split("-", 1) if "-" in value else ("0", value)
    parts = [float(part) for part in clock.split(":")]
    total = 0.0
    for part in parts:
        total = total * 60 + part
    return int(days) * 86400 + total


def command_value(arguments):
    try:
        result = subprocess.run(arguments, capture_output=True, text=True,
                                timeout=PROBE_TIMEOUT_SECONDS, check=False)
        return result.stdout.strip() if result.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


class NativeResourceSampler:
    def __init__(self, executable, cli, output, source_sha):
        self.executable = str(Path(executable).resolve())
        self.cli = str(cli)
        self.output = Path(output)
        self.socket = Path(os.environ.get("ZMUX_SOCKET_PATH") or os.environ.get("ZMUX_SOCKET")
                           or Path.home() / "Library/Application Support/zmux/control.sock")
        self.stop_requested = threading.Event()
        self.thread = None
        self.started = time.monotonic()
        self.root = None
        self.existing = set()
        self.libproc = None
        self.previous = None
        self.finalized = threading.Event()
        self.publication_lock = threading.Lock()
        self.document = {
            "schema_version": 1, "source_sha": source_sha,
            "scope": "whole_packaged_smoke_journey", "state": "starting",
            "started_utc": datetime.now(timezone.utc).isoformat(),
            "host": {"system": platform.system(), "architecture": platform.machine(),
                     "os_version": platform.mac_ver()[0], "kernel": platform.release(),
                     "logical_cpus": os.cpu_count(),
                     "runner_os": os.environ.get("RUNNER_OS"),
                     "runner_arch": os.environ.get("RUNNER_ARCH"),
                     "image_os": os.environ.get("ImageOS"),
                     "image_version": os.environ.get("ImageVersion")},
            "instrumentation": {
                "interval_seconds": INTERVAL_SECONDS, "maximum_samples": MAX_SAMPLES,
                "maximum_duration_seconds": MAX_DURATION_SECONDS,
                "probe_timeout_seconds": PROBE_TIMEOUT_SECONDS,
                "sampler_join_timeout_seconds": JOIN_TIMEOUT_SECONDS,
                "maximum_cli_probes_per_second": 1,
                "latency_scope": "bundled_cli_process_launch_plus_status_round_trip",
                "rss_scope": "resident_set_not_physical_footprint",
                "cpu_percent_scale": "100_percent_is_one_logical_cpu",
                "descendants_scope": "current_ppid_tree_excludes_separately_owned_sshd_and_detached_helpers",
                "limitations": ["No workload phase labels or performance budgets",
                                "No input-to-display or main-loop latency",
                                "Sampling and CLI probes add measured observer work",
                                "Descendant RSS may double-count shared pages"]},
            "samples": [],
        }

    def start(self):
        try:
            if platform.system() != "Darwin":
                self.document.update(state="unavailable", reason="macos_required")
                self._save()
                return
            self.document["host"]["hardware_model"] = command_value(["/usr/sbin/sysctl", "-n", "hw.model"])
            self.document["host"]["memory_bytes"] = command_value(["/usr/sbin/sysctl", "-n", "hw.memsize"])
            self.libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
            self.libproc.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
            self.libproc.proc_pidpath.restype = ctypes.c_int
            self.existing = {identity for identity in self._matching(self._processes())}
            self.document["state"] = "sampling"
            self._save()
            self.started = time.monotonic()
            self.thread = threading.Thread(target=self._run, name="native-resource-sampler", daemon=True)
            self.thread.start()
        except Exception as error:
            self.document.update(state="unavailable", reason=type(error).__name__)
            self._save()

    def stop(self):
        self.stop_requested.set()
        timed_out = False
        if self.thread is not None and self.thread.ident is not None:
            # Every subprocess is time-bounded; no app/fixture process is signalled.
            self.thread.join(timeout=JOIN_TIMEOUT_SECONDS)
            timed_out = self.thread.is_alive()
        with self.publication_lock:
            # Serialize finalization against the worker's last mutation/rename.
            self.finalized.set()
            if timed_out:
                self.document.update(state="partial", reason="sampler_stop_timeout")
            self.document["finished_utc"] = datetime.now(timezone.utc).isoformat()
            self.document["elapsed_seconds"] = round(time.monotonic() - self.started, 6)
            self.document["available_resource_samples"] = sum(
                sample["state"] == "available" for sample in self.document["samples"])
            self.document["available_cli_samples"] = sum(
                sample.get("cli_status", {}).get("state") == "available" for sample in self.document["samples"])
            self.document["cli_probe_count"] = sum(
                sample.get("cli_status", {}).get("attempted", False) for sample in self.document["samples"])
            if self.document["state"] == "sampling":
                self.document["state"] = "complete" if self.document["available_resource_samples"] else "unavailable"
                if not self.document["available_resource_samples"]:
                    self.document["reason"] = "packaged_app_metrics_unavailable"
        self._save()

    def _save(self):
        worker = threading.current_thread() is self.thread
        try:
            with self.publication_lock:
                if worker and self.finalized.is_set():
                    return
                payload = json.dumps(self.document, indent=2, allow_nan=False) + "\n"
            temporary = self.output.with_suffix(f".json.{threading.get_ident()}.tmp")
            temporary.write_text(payload)
            with self.publication_lock:
                if worker and self.finalized.is_set():
                    temporary.unlink(missing_ok=True)
                    return
                temporary.replace(self.output)
        except (OSError, ValueError) as error:
            # Telemetry cannot replace or fail the native correctness gate.
            print(f"Native resource telemetry unavailable: {type(error).__name__}", flush=True)

    def _processes(self):
        output = command_value(["/bin/ps", "-axo", "pid=,ppid=,rss=,time=,lstart="])
        if output is None:
            raise ResourceUnavailable("process_metrics_unavailable")
        lines = output.splitlines()
        if len(lines) > MAX_PROCESSES:
            raise ResourceUnavailable("process_table_limit")
        result = {}
        for line in lines:
            pid, parent, rss, cpu, birth = line.split(maxsplit=4)
            result[int(pid)] = {"pid": int(pid), "parent_pid": int(parent),
                                "rss_kib": int(rss), "cpu_seconds": cpu_seconds(cpu),
                                "started": birth}
        return result

    def _path(self, pid):
        buffer = ctypes.create_string_buffer(4096)
        size = self.libproc.proc_pidpath(pid, buffer, len(buffer))
        return os.path.realpath(os.fsdecode(buffer.value)) if size > 0 else None

    def _matching(self, processes):
        return [(pid, item["started"]) for pid, item in processes.items()
                if self._path(pid) == self.executable]

    def _verify_socket(self):
        output = command_value(["/usr/sbin/lsof", "-n", "-P", "-a", "-p", str(self.root[0]), "-U", "-Fn"])
        return output is not None and any(
            line.startswith("n/") and os.path.realpath(line[1:]) == os.path.realpath(self.socket)
            for line in output.splitlines())

    def _sample(self):
        began = time.monotonic()
        item = {"elapsed_seconds": round(began - self.started, 6)}
        processes = self._processes()
        if self.root is None:
            candidates = [identity for identity in self._matching(processes) if identity not in self.existing]
            if len(candidates) != 1:
                item.update(state="unavailable", reason="app_not_found" if not candidates else "ambiguous_app_instances")
                return item
            self.root = candidates[0]
        root = processes.get(self.root[0])
        if root is None or root["started"] != self.root[1] or self._path(root["pid"]) != self.executable:
            item.update(state="unavailable", reason="identified_app_exited")
            return item
        descendants = {root["pid"]}
        for _ in range(MAX_DESCENDANTS):
            expanded = descendants | {pid for pid, process in processes.items() if process["parent_pid"] in descendants}
            if len(expanded) > MAX_DESCENDANTS:
                item.update(state="unavailable", reason="descendant_limit")
                return item
            if expanded == descendants:
                break
            descendants = expanded
        owned = [processes[pid] for pid in sorted(descendants)]
        item.update(state="available", app_pid=root["pid"], app_rss_kib=root["rss_kib"],
                    app_cpu_seconds=root["cpu_seconds"], current_tree_processes=len(owned),
                    current_tree_rss_kib=sum(process["rss_kib"] for process in owned),
                    processes=owned, app_cpu_percent=None)
        if self.previous is not None:
            when, cpu = self.previous
            delta = root["cpu_seconds"] - cpu
            if delta >= 0 and began > when:
                item["app_cpu_percent"] = round(100 * delta / (began - when), 4)
        self.previous = (began, root["cpu_seconds"])
        socket_verified = self._verify_socket()
        item["cli_status"] = {"state": "unavailable", "attempted": False, "reason": "socket_owner_unverified"}
        if socket_verified and not self.stop_requested.is_set():
            probe = time.monotonic()
            try:
                completed = subprocess.run([self.cli, "--socket", str(self.socket), "--timeout", "500", "status"],
                                           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                           stderr=subprocess.DEVNULL, timeout=PROBE_TIMEOUT_SECONDS, check=False)
                item["cli_status"] = {"state": "available" if completed.returncode == 0 else "unavailable",
                                      "attempted": True,
                                      "return_code": completed.returncode,
                                      "elapsed_ms": round((time.monotonic() - probe) * 1000, 3)}
            except (OSError, subprocess.TimeoutExpired) as error:
                item["cli_status"] = {"state": "unavailable", "attempted": True,
                                      "reason": type(error).__name__,
                                      "elapsed_ms": round((time.monotonic() - probe) * 1000, 3)}
        item["observer_wall_ms"] = round((time.monotonic() - began) * 1000, 3)
        return item

    def _run(self):
        try:
            while (not self.stop_requested.is_set() and len(self.document["samples"]) < MAX_SAMPLES
                   and time.monotonic() - self.started < MAX_DURATION_SECONDS):
                began = time.monotonic()
                sample = self._sample()
                with self.publication_lock:
                    if self.finalized.is_set():
                        return
                    sample.setdefault("observer_wall_ms", round((time.monotonic() - began) * 1000, 3))
                    self.document["samples"].append(sample)
                self._save()
                # A full interval after each probe limits the actual CLI rate to <=1Hz.
                if self.stop_requested.wait(INTERVAL_SECONDS):
                    break
            if not self.stop_requested.is_set():
                with self.publication_lock:
                    if not self.finalized.is_set():
                        self.document["state"] = "limited"
        except Exception as error:
            with self.publication_lock:
                if not self.finalized.is_set():
                    self.document.update(state="partial" if self.document["samples"] else "unavailable",
                                         reason=str(error) if isinstance(error, ResourceUnavailable) else type(error).__name__)
        finally:
            if not self.finalized.is_set():
                self._save()
