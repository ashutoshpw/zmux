#!/usr/bin/env python3
"""Build and exercise pinned, isolated terminal prototypes; never launches an app."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request


HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(REPO / "scripts"))
from process_group import ProcessGroup

BUILD_TIMEOUT = 600
MAX_LOG_BYTES = 1024 * 1024
MAX_DOWNLOAD_BYTES = 128 * 1024 * 1024


def execute_fixture(binary, arguments=()):
    """Report requested OS containment separately from the fixture result."""
    import resource
    limits = {}
    for name, value in [("RLIMIT_CORE", 0), ("RLIMIT_CPU", 3),
                        ("RLIMIT_AS", 512 * 1024 * 1024)]:
        try:
            resource.setrlimit(getattr(resource, name), (value, value))
            limits[name] = {"state": "configured", "value": value}
        except (AttributeError, OSError, ValueError) as error:
            limits[name] = {"state": "unavailable", "reason": type(error).__name__}
    print("FIXTURE_LIMITS=" + json.dumps(limits, sort_keys=True), flush=True)
    os.execv(binary, [binary, *arguments])


def run(command, label, output, phases, timeout=60, cwd=None, env=None):
    """Drain bounded logs and stop only the newly created subprocess group."""
    began = time.monotonic()
    group = ProcessGroup(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    process = group.process
    selector = None
    total = retained = 0
    timed_out = False
    killed_at = None
    path = output / (label + ".log")
    try:
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        with path.open("wb") as log:
            # EOF is independent of process completion. A leader may close its
            # output and keep running, or exit while a child retains the pipe.
            while selector.get_map() or not group.exited():
                now = time.monotonic()
                if now - began >= timeout and killed_at is None:
                    timed_out = True
                    killed_at = now
                    group.stop()
                if killed_at is not None and now - killed_at >= 5:
                    break
                for key, _ in selector.select(timeout=0.1):
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(chunk)
                    saved = chunk[:max(0, MAX_LOG_BYTES - retained)]
                    log.write(saved)
                    retained += len(saved)
    finally:
        # Phase ownership ends here, including children that closed stdout
        # before their leader exited successfully. Never leave them running.
        try:
            if selector is not None:
                selector.close()
        finally:
            code = group.close(timeout=5, retry=True)
        timed_out = timed_out or group.reap_retried
    record = {"label": label, "command": [str(part) for part in command],
              "exit_code": code, "timed_out": timed_out,
              "wall_seconds": round(time.monotonic() - began, 3),
              "log": path.name, "log_bytes_seen": total,
              "log_bytes_retained": retained,
              "confirmed_zombie_group": group.confirmed_zombie_group}
    phases.append(record)
    if timed_out or code:
        raise RuntimeError(f"{label} failed: exit={code}, timed_out={timed_out}")
    return path.read_text(errors="replace")


def download(pin, directory, receipt):
    target = directory / (pin["root"] + ".archive")
    digest = hashlib.sha256()
    began = time.monotonic()
    count = 0
    with urllib.request.urlopen(pin["url"], timeout=20) as response, target.open("wb") as out:
        while chunk := response.read(1024 * 1024):
            count += len(chunk)
            if count > MAX_DOWNLOAD_BYTES or time.monotonic() - began > 120:
                raise RuntimeError("download exceeded byte/time bound")
            digest.update(chunk)
            out.write(chunk)
    actual = digest.hexdigest()
    if actual != pin["sha256"]:
        raise RuntimeError("download SHA-256 mismatch")
    receipt.append({"url": pin["url"], "bytes": count,
                    "sha256": actual, "verified": True})
    with tarfile.open(target) as archive:
        members = archive.getmembers()
        if len(members) > 200000 or sum(member.size for member in members) > 1024 ** 3:
            raise RuntimeError("archive exceeded extraction bound")
        if any(member.name.split("/", 1)[0] != pin["root"] for member in members):
            raise RuntimeError("unexpected archive root")
        archive.extractall(directory, members=members, filter="data")
    return directory / pin["root"]


def mac_target(architecture):
    if architecture == "arm64":
        return "aarch64-macos.14.0", "aarch64-apple-darwin"
    return "x86_64-macos.14.0", "x86_64-apple-darwin"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--macos-universal", action="store_true")
    parser.add_argument("--dependency-cache", type=Path,
                        help="Explicit Zig content-addressed cache; default is a fresh temporary cache")
    parser.add_argument("--resource-experiment", action="store_true",
                        help="Apply the original hashed allocator/exact-history patch; experiment only")
    parser.add_argument("--effects-experiment", action="store_true",
                        help="Exercise original bounded BEL/reply/RESET callback batches; experiment only")
    parser.add_argument("--integrated-experiment", action="store_true",
                        help="Exercise one quota/callback/publication owner; requires --resource-experiment")
    parser.add_argument("--metadata-experiment", action="store_true",
                        help="Apply isolated source-aware metadata extension; requires resource and integrated experiments")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    (output / "receipt.json").unlink(missing_ok=True)
    pins = json.loads((HERE / "pins.json").read_text())
    system, machine = platform.system(), platform.machine()
    if machine == "aarch64" and system == "Darwin":
        machine = "arm64"
    host = f"{system}-{machine}"
    inputs = ["run.py", "../../scripts/process_group.py", "pins.json", "probe.c", "probe.rs", "owner.rs", "frame.h",
              "compatibility.rs", "current-adapter.rs"]
    if args.resource_experiment:
        inputs += ["resource.patch", "resource.c", "resource.h", "resource.rs",
                   "RESOURCE_CONTRACT.md", "licenses/Ghostty-LICENSE", "resource-internal.h"]
    if args.effects_experiment:
        inputs += ["effects.c", "effects.h", "effects.rs"]
    if args.effects_experiment or args.integrated_experiment:
        inputs += ["collector.c", "collector.h", "effects.h"]
    if args.integrated_experiment:
        inputs += ["integrated.c", "integrated.h", "integrated.rs"]
    if args.metadata_experiment:
        inputs += ["metadata.patch", "metadata.c", "metadata.h", "metadata.rs", "metadata-owner.rs",
                   "METADATA_CONTRACT.md"]
    inputs = list(dict.fromkeys(inputs))
    document = {"schema_version": 1, "state": "starting", "pins": pins,
                "host": {"system": system, "architecture": machine,
                         "os_release": platform.release(), "macos": platform.mac_ver()[0]},
                "downloads": [], "phases": [], "targets": [],
                "dependency_cache": "explicit_existing_cache" if args.dependency_cache else "fresh_temporary_cache",
                "harness_sha256": {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
                                   for name in inputs},
                "limits": {"build_deadline_seconds": BUILD_TIMEOUT,
                           "build_jobs": 2, "retained_log_bytes_per_phase": MAX_LOG_BYTES,
                           "fixture_wall_seconds": 10, "snapshot_bytes": 512 * 1024,
                           "reply_bytes": 4096, "cell_utf8_bytes": 256},
                "upstream_patch": {"applied": False,
                    "base_commit": pins["ghostty"]["commit"],
                    "sha256": hashlib.sha256((HERE / "resource.patch").read_bytes()).hexdigest()
                              if args.resource_experiment else None},
                "resource_experiment": {"requested": args.resource_experiment,
                    "total_app_memory_bound": False, "scope": "host_architecture_only"},
                "effects_experiment": {"requested": args.effects_experiment,
                    "scope": "host_architecture_only", "total_app_memory_bound": False,
                    "real_pty_round_trip": False, "metadata_osc_notifications_supported": False,
                    "callback_reply_bytes": 4096, "callback_events": 1024},
                "integrated_experiment": {"requested": args.integrated_experiment,
                    "scope": "host_architecture_only", "total_app_memory_bound": False,
                    "real_pty_round_trip": False, "single_bounded_terminal_owner": True},
                "metadata_experiment": {"requested": args.metadata_experiment,
                    "scope": "host_architecture_only", "real_pty_round_trip": False,
                    "total_app_memory_bound": False, "production_engine_changed": False,
                    "control_string_bytes": 16384, "raw_callback_bytes": 20480,
                    "patch_sha256": hashlib.sha256((HERE / "metadata.patch").read_bytes()).hexdigest()
                                    if args.metadata_experiment else None},
                "claims": {"production_engine_changed": False, "native_app_launched": False,
                           "performance_measured": False, "graphics_tested": False,
                           "cross_architecture_execution": False,
                           "compatibility_exporter_linked_into_application": False}}
    try:
        if host not in pins["zig"]["hosts"]:
            raise RuntimeError(f"unsupported host {host}")
        if args.macos_universal and system != "Darwin":
            raise RuntimeError("universal compilation requires a macOS host and SDK")
        if args.integrated_experiment and not args.resource_experiment:
            raise RuntimeError("--integrated-experiment requires --resource-experiment")
        if args.metadata_experiment and not (args.resource_experiment and args.integrated_experiment):
            raise RuntimeError("--metadata-experiment requires --resource-experiment and --integrated-experiment")
        phases = document["phases"]
        document["source_sha"] = run(["git", "rev-parse", "HEAD"], "source", output, phases,
                                      cwd=REPO).strip()
        rust = run(["rustc", "--version"], "rust-version", output, phases).strip()
        document["rust_version"] = rust
        if not rust.startswith("rustc " + pins["rust_version"] + " "):
            raise RuntimeError("Rust version does not match the pinned toolchain")
        with tempfile.TemporaryDirectory(prefix="zmux-terminal-feasibility-") as temporary:
            work = Path(temporary)
            zig_root = download(pins["zig"]["hosts"][host], work, document["downloads"])
            source = download(pins["ghostty"], work, document["downloads"])
            for label, directory in [("ghostty", source), ("zig", zig_root)]:
                license_text = (directory / "LICENSE").read_text()
                if "MIT License" not in license_text.splitlines()[0]:
                    raise RuntimeError(f"unexpected {label} root license")
            document["root_licenses_verified"] = ["ghostty:MIT", "zig:MIT"]
            if args.resource_experiment:
                if (source / "LICENSE").read_bytes() != (HERE / "licenses/Ghostty-LICENSE").read_bytes():
                    raise RuntimeError("retained Ghostty license differs from verified source")
                patch = str(HERE / "resource.patch")
                run(["git", "apply", "--check", patch], "resource-patch-check", output,
                    phases, cwd=source)
                run(["git", "apply", patch], "resource-patch-apply", output, phases, cwd=source)
                document["upstream_patch"]["applied"] = True
            if args.metadata_experiment:
                patch = str(HERE / "metadata.patch")
                run(["git", "apply", "--check", patch], "metadata-patch-check", output, phases, cwd=source)
                run(["git", "apply", patch], "metadata-patch-apply", output, phases, cwd=source)
                if (source / "LICENSE").read_bytes() != (HERE / "licenses/Ghostty-LICENSE").read_bytes():
                    raise RuntimeError("metadata patch changed the retained upstream license")
                document["metadata_experiment"]["patch_applied"] = True
            zig = zig_root / "zig"
            version = run([str(zig), "version"], "zig-version", output, phases).strip()
            if version != pins["zig"]["version"]:
                raise RuntimeError("Zig version does not match the pin")
            environment = os.environ.copy()
            environment["ZIG_GLOBAL_CACHE_DIR"] = str(
                args.dependency_cache.resolve() if args.dependency_cache else work / "zig-global-cache")
            environment["ZIG_LOCAL_CACHE_DIR"] = str(work / "zig-local-cache")
            architectures = [machine]
            sdk = None
            if system == "Darwin":
                environment["MACOSX_DEPLOYMENT_TARGET"] = "14.0"
                sdk = run(["xcrun", "--sdk", "macosx", "--show-sdk-path"],
                          "apple-sdk", output, phases).strip()
                if args.macos_universal:
                    architectures = ["arm64", "x86_64"]
            libraries, executables = [], []
            components = {}
            for architecture in architectures:
                prefix = work / ("install-" + architecture)
                command = [str(zig), "build", "-Demit-lib-vt=true", "-Demit-xcframework=false",
                           "-Dapp-runtime=none", "-Doptimize=ReleaseSafe", "-Dsimd=false",
                           "-Dvt-features=-all,+render-state,+grid-introspection,+formatter,+input-encode",
                           "-j2", "--prefix", str(prefix)]
                triple = None
                if system == "Darwin":
                    target, triple = mac_target(architecture)
                    command += ["-Dtarget=" + target]
                run(command, "build-" + architecture, output, phases, timeout=BUILD_TIMEOUT,
                    cwd=source, env=environment)
                obj = work / ("probe-" + architecture + ".o")
                compile_c = ["cc", "-std=c11", "-Werror", "-DGHOSTTY_STATIC",
                             "-I", str(source / "include"), "-c", str(HERE / "probe.c"),
                             "-o", str(obj)]
                if system == "Darwin":
                    compile_c += ["-arch", architecture, "-isysroot", sdk]
                run(compile_c, "c-shim-" + architecture, output, phases, env=environment)
                library = prefix / "lib/libghostty-vt.a"
                components[architecture] = (obj, library)
                executable = work / ("probe-" + architecture)
                compile_rust = ["rustc", "--edition=2024", str(HERE / "probe.rs"),
                                "-C", "link-arg=" + str(obj), "-C", "link-arg=" + str(library),
                                "-o", str(executable)]
                if triple:
                    compile_rust += ["--target", triple, "-C", "link-arg=-isysroot",
                                     "-C", "link-arg=" + sdk]
                run(compile_rust, "rust-owner-" + architecture, output, phases, env=environment)
                result = {"architecture": architecture, "rust_target": triple,
                          "compile": "passed", "execution": "not_run_cross_architecture"}
                if architecture == machine:
                    stdout = run([sys.executable, str(HERE / "run.py"), "--execute-fixture",
                                  str(executable)], "fixture-" + architecture,
                                 output, phases, timeout=10)
                    if "success=true" not in stdout:
                        raise RuntimeError("native fixture returned no completed assertion receipt")
                    result.update(execution="passed", observation_stdout=stdout[:16384])
                document["targets"].append(result)
                libraries.append(str(library))
                executables.append(str(executable))
            if args.macos_universal:
                for label, inputs in [("library", libraries), ("executable", executables)]:
                    fat = work / ("universal-" + label)
                    run(["lipo", "-create", *inputs, "-output", str(fat)],
                        "universal-" + label, output, phases)
                    # -verify_arch consumes all remaining arguments as arch names.
                    run(["lipo", str(fat), "-verify_arch", "arm64", "x86_64"],
                        "verify-universal-" + label, output, phases)
                document["universal_compile"] = "passed"
            # Build the existing adapter in scratch output without changing its lockfile.
            adapter_target = work / "current-adapter-target"
            run(["cargo", "build", "--locked", "--manifest-path", str(REPO / "Cargo.toml"),
                 "-p", "zmux-terminal-adapter", "--jobs", "2", "--target-dir", str(adapter_target)],
                "current-adapter-build", output, phases, timeout=BUILD_TIMEOUT)
            matches = list((adapter_target / "debug/deps").glob("libzmux_terminal_adapter-*.rlib"))
            if len(matches) != 1:
                raise RuntimeError("ambiguous current-adapter library")
            baseline = work / "current-adapter-probe"
            run(["rustc", "--edition=2024", str(HERE / "current-adapter.rs"), "-L",
                 "dependency=" + str(adapter_target / "debug/deps"), "--extern",
                 "zmux_terminal_adapter=" + str(matches[0]), "-o", str(baseline)],
                "current-adapter-driver", output, phases)
            document["current_adapter_observation_stdout"] = run(
                [sys.executable, str(HERE / "run.py"), "--execute-fixture", str(baseline)],
                "current-adapter-fixture", output, phases, timeout=10)[:16384]
            # Public Viewport types are linked only into this separate host fixture.
            obj, library = components[machine]
            compatibility = work / "compatibility-probe"
            command = ["rustc", "--edition=2024", str(HERE / "compatibility.rs"),
                       "-L", "dependency=" + str(adapter_target / "debug/deps"),
                       "--extern", "zmux_terminal_adapter=" + str(matches[0]),
                       "-C", "link-arg=" + str(obj), "-C", "link-arg=" + str(library),
                       "-o", str(compatibility)]
            if sdk:
                command += ["-C", "link-arg=-isysroot", "-C", "link-arg=" + sdk]
            run(command, "compatibility-owner", output, phases, env=environment)
            stdout = run([sys.executable, str(HERE / "run.py"), "--execute-fixture",
                          str(compatibility)], "compatibility-fixture", output, phases, timeout=10)
            if "compatibility_success=true" not in stdout:
                raise RuntimeError("compatibility fixture returned no completed assertion receipt")
            document["compatibility_exporter"] = {"architecture": machine,
                "scope": "host_architecture_only", "compile": "passed", "execution": "passed",
                "observation_stdout": stdout[:16384]}
            if args.resource_experiment:
                resource_obj = work / "resource.o"
                command = ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-DGHOSTTY_STATIC",
                           "-I", str(source / "include"), "-c", str(HERE / "resource.c"),
                           "-o", str(resource_obj)]
                if sdk:
                    command += ["-arch", machine, "-isysroot", sdk]
                run(command, "resource-c-shim", output, phases, env=environment)
                resource_binary = work / "resource-probe"
                command = ["rustc", "--edition=2024", str(HERE / "resource.rs"),
                           "-C", "link-arg=" + str(obj), "-C", "link-arg=" + str(resource_obj),
                           "-C", "link-arg=" + str(library), "-o", str(resource_binary)]
                if sdk:
                    command += ["-C", "link-arg=-isysroot", "-C", "link-arg=" + sdk]
                run(command, "resource-rust-owner", output, phases, env=environment)
                controls = []
                for name, message in [
                    ("pages", "zero page quota must reject construction before mapping preheat"),
                    ("history", "retained 2 rows with limit1"),
                ]:
                    label = "resource-original-constructor-" + name
                    try:
                        run([sys.executable, str(HERE / "run.py"), "--execute-fixture",
                             str(resource_binary), "--original-constructor-" + name],
                            label, output, phases, timeout=10)
                    except RuntimeError:
                        record = phases[-1]
                        log = (output / record["log"]).read_text(errors="replace")
                        if record["timed_out"] or record["exit_code"] != 101 or message not in log:
                            raise RuntimeError(label + " did not fail for the intended assertion")
                        record["expected_failure"] = True
                        controls.append({"name": name, "execution": "expected_assertion_failure",
                                         "log": record["log"]})
                    else:
                        raise RuntimeError(label + " unexpectedly passed")
                stdout = run([sys.executable, str(HERE / "run.py"), "--execute-fixture",
                              str(resource_binary)], "resource-fixture", output, phases, timeout=10)
                if "resource_success=true" not in stdout:
                    raise RuntimeError("resource fixture returned no completed assertion receipt")
                document["resource_experiment"].update(
                    execution="passed", architecture=machine, negative_controls=controls,
                    observation_stdout=stdout[:16384])
            def compile_shim(c_source, c_object, label):
                command = ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-DGHOSTTY_STATIC",
                           "-I", str(source / "include"), "-I", str(HERE),
                           "-c", str(c_source), "-o", str(c_object)]
                if sdk:
                    command += ["-arch", machine, "-isysroot", sdk]
                run(command, label, output, phases, env=environment)

            def compile_owner(rust_source, objects, binary, label, adapter=False):
                command = ["rustc", "--edition=2024", str(rust_source)]
                if adapter:
                    command += ["-L", "dependency=" + str(adapter_target / "debug/deps"),
                                "--extern", "zmux_terminal_adapter=" + str(matches[0])]
                for item in [obj, *objects, library]:
                    command += ["-C", "link-arg=" + str(item)]
                command += ["-o", str(binary)]
                if sdk:
                    command += ["-C", "link-arg=-isysroot", "-C", "link-arg=" + sdk]
                run(command, label, output, phases, env=environment)

            def expect_assertion(binary, label, assertion, mutated_source):
                try:
                    run([sys.executable, str(HERE / "run.py"), "--execute-fixture", str(binary)],
                        label, output, phases, timeout=10)
                except RuntimeError:
                    record = phases[-1]
                    log = (output / record["log"]).read_text(errors="replace")
                    if record["timed_out"] or record["exit_code"] != 101 or assertion not in log:
                        raise RuntimeError(label + " did not fail for the intended assertion")
                    record["expected_failure"] = True
                    return {"name": label, "execution": "expected_assertion_failure",
                            "scope": "temporary_original_shim_mutation",
                            "mutated_source_sha256": hashlib.sha256(mutated_source.read_bytes()).hexdigest(),
                            "log": record["log"]}
                raise RuntimeError(label + " unexpectedly passed")

            if args.effects_experiment or args.integrated_experiment:
                collector_obj = work / "collector.o"
                compile_shim(HERE / "collector.c", collector_obj, "collector-c-shim")
            if args.effects_experiment:
                effects_obj = work / "effects.o"
                compile_shim(HERE / "effects.c", effects_obj, "effects-wrapper-c-shim")
                controls = []
                original = (HERE / "collector.c").read_text()
                variants = [
                    ("normal", None, None, None),
                    ("silent-reply-drop", "e->failure = ZMUX_EFFECT_ERROR_REPLY; return;", "return;",
                     "reply limit+1 must fail visibly"),
                    ("reset-order", "(ZmuxEffectEvent){ZMUX_EFFECT_RESET, 0, 0}",
                     "(ZmuxEffectEvent){ZMUX_EFFECT_BELL, 0, 0}",
                     "callback order must preserve BEL/reply/RESET"),
                ]
                for name, old, new, assertion in variants:
                    label = "effects" if name == "normal" else "effects-control-" + name
                    variant_obj = collector_obj
                    mutated = None
                    if old is not None:
                        if original.count(old) != 1:
                            raise RuntimeError(label + " mutation no longer has one exact source target")
                        mutated = work / (label + ".c")
                        mutated.write_text(original.replace(old, new))
                        variant_obj = work / (label + ".o")
                        compile_shim(mutated, variant_obj, label + "-c-shim")
                    binary = work / (label + "-probe")
                    compile_owner(HERE / "effects.rs", [effects_obj, variant_obj], binary,
                                  label + "-rust-owner", adapter=True)
                    if assertion is None:
                        stdout = run([sys.executable, str(HERE / "run.py"), "--execute-fixture", str(binary)],
                                     "effects-fixture", output, phases, timeout=10)
                        if "effects_success=true" not in stdout:
                            raise RuntimeError("effects fixture returned no completed assertion receipt")
                        document["effects_experiment"].update(execution="passed", architecture=machine,
                                                              observation_stdout=stdout[:16384])
                    else:
                        controls.append(expect_assertion(binary, label + "-fixture", assertion, mutated))
                document["effects_experiment"]["negative_controls"] = controls
            if args.integrated_experiment:
                integrated_obj = work / "integrated.o"
                compile_shim(HERE / "integrated.c", integrated_obj, "integrated-c-shim")
                binary = work / "integrated-probe"
                compile_owner(HERE / "integrated.rs", [resource_obj, collector_obj, integrated_obj], binary,
                              "integrated-rust-owner")
                stdout = run([sys.executable, str(HERE / "run.py"), "--execute-fixture", str(binary)],
                             "integrated-fixture", output, phases, timeout=10)
                if "integrated_success=true" not in stdout:
                    raise RuntimeError("integrated fixture returned no completed assertion receipt")
                original = (HERE / "integrated.c").read_text()
                old = "if (snapshot_status || fence(i, 0)) return fence(i, ZMUX_INTEGRATED_EXPORT);"
                if original.count(old) != 1:
                    raise RuntimeError("integrated render control no longer has one exact source target")
                mutated = work / "integrated-control-unchecked-render.c"
                mutated.write_text(original.replace(old, "(void)snapshot_status;"))
                control_obj = work / "integrated-control.o"
                compile_shim(mutated, control_obj, "integrated-control-c-shim")
                control_binary = work / "integrated-control-probe"
                compile_owner(HERE / "integrated.rs", [resource_obj, collector_obj, control_obj], control_binary,
                              "integrated-control-rust-owner")
                control = expect_assertion(control_binary, "integrated-control-fixture",
                                           "post-render allocation denial must fence publication", mutated)
                document["integrated_experiment"].update(execution="passed", architecture=machine,
                    observation_stdout=stdout[:16384], negative_controls=[control])
            if args.metadata_experiment:
                metadata_obj = work / "metadata.o"
                compile_shim(HERE / "metadata.c", metadata_obj, "metadata-c-shim")
                binary = work / "metadata-probe"
                compile_owner(HERE / "metadata.rs", [resource_obj, collector_obj, integrated_obj, metadata_obj],
                              binary, "metadata-rust-owner")
                stdout = run([sys.executable, str(HERE / "run.py"), "--execute-fixture", str(binary)],
                             "metadata-fixture", output, phases, timeout=10)
                if "metadata_success=true" not in stdout:
                    raise RuntimeError("metadata fixture returned no completed assertion receipt")
                controls = []
                original = (HERE / "metadata.c").read_text()
                for name, old, new, assertion in [
                    ("source", "{event->source, event->subcommand,",
                     "{event->source == 777 ? 9 : event->source, event->subcommand,",
                     "notification source must remain exact"),
                    ("arena-fence", "m->failure = 2; integrated_abort(m->integrated); return;", "return;",
                     "metadata arena overflow must fence publication"),
                ]:
                    if original.count(old) != 1:
                        raise RuntimeError("metadata control target is not unique: " + name)
                    label = "metadata-control-" + name
                    mutated = work / (label + ".c")
                    mutated.write_text(original.replace(old, new))
                    control_obj = work / (label + ".o")
                    compile_shim(mutated, control_obj, label + "-c-shim")
                    control_binary = work / (label + "-probe")
                    compile_owner(HERE / "metadata.rs", [resource_obj, collector_obj, integrated_obj, control_obj],
                                  control_binary, label + "-rust-owner")
                    controls.append(expect_assertion(control_binary, label + "-fixture", assertion, mutated))
                document["metadata_experiment"].update(execution="passed", architecture=machine,
                    observation_stdout=stdout[:16384], negative_controls=controls)
        document["state"] = "complete"
    except Exception as error:
        document.update(state="failed", error=f"{type(error).__name__}: {error}")
    finally:
        (output / "receipt.json").write_text(json.dumps(document, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"state": document["state"], "receipt": str(output / "receipt.json")}))
    return 0 if document["state"] == "complete" else 1


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "--execute-fixture":
        execute_fixture(sys.argv[2], sys.argv[3:])
    raise SystemExit(main())
