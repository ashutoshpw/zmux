#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build the pinned accepted owner and exercise actual kernel PTY replies."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import sys
import tempfile

HERE = Path(__file__).resolve().parent
BASE = HERE.parent / "terminal-feasibility"
REPO = HERE.parent.parent


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--dependency-cache", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    lock = json.loads((HERE / "base-lock.json").read_text())
    new_inputs = ["run.py", "base-lock.json", "transport.c", "child.py", "pty.rs",
                  "pty-owner.rs", "PTY_CONTRACT.md", "construction.c", "cleanup.c"]
    document = {"schema_version": 1, "state": "starting", "phases": [], "downloads": [],
                "base_lock": lock, "source_sha256": {n: sha(HERE / n) for n in new_inputs},
                "host": {"system": platform.system(), "architecture": platform.machine(),
                         "os_release": platform.release(), "macos": platform.mac_ver()[0]},
                "dependency_cache": "explicit_existing_cache" if args.dependency_cache else "fresh_temporary_cache",
                "limits": {"read_bytes": 512, "fixed_input_bytes": 8192,
                           "reply_batch_bytes": 4096, "reply_lifetime_ms": 250,
                           "reserved_reply_payload_capacities": {"probe_unused": 4096,
                               "collector": 4096, "publication_slots": 8192,
                               "transport_input": 8192, "total": 24576},
                           "fixture_wall_seconds": 10, "retained_log_bytes_per_phase": 1048576},
                "claims": {"real_kernel_pty_child": True, "controlling_terminal": False,
                           "production_engine_changed": False, "native_app_launched": False,
                           "total_app_memory_bound": False, "cross_architecture_execution": False}}
    try:
        for name, expected in lock["harness_sha256"].items():
            if sha(BASE / name) != expected:
                raise RuntimeError("accepted metadata input drifted: " + name)
        spec = importlib.util.spec_from_file_location("accepted_runner", BASE / "run.py")
        base = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(base)
        phases = document["phases"]
        document["source_sha"] = base.run(["git", "rev-parse", "HEAD"], "source", output, phases, cwd=REPO).strip()
        pins = json.loads((BASE / "pins.json").read_text())
        document["pins"] = pins
        system, machine = platform.system(), platform.machine()
        if machine == "aarch64" and system == "Darwin":
            machine = "arm64"
        host = system + "-" + machine
        if host not in pins["zig"]["hosts"]:
            raise RuntimeError("unsupported host " + host)
        rust = base.run(["rustc", "--version"], "rust-version", output, phases).strip()
        if not rust.startswith("rustc " + pins["rust_version"] + " "):
            raise RuntimeError("Rust version differs from accepted pin")
        with tempfile.TemporaryDirectory(prefix="zmux-pty-feasibility-") as temporary:
            work = Path(temporary)
            zig_root = base.download(pins["zig"]["hosts"][host], work, document["downloads"])
            source = base.download(pins["ghostty"], work, document["downloads"])
            for directory in [zig_root, source]:
                if "MIT License" not in (directory / "LICENSE").read_text().splitlines()[0]:
                    raise RuntimeError("unexpected source root license")
            if (source / "LICENSE").read_bytes() != (BASE / "licenses/Ghostty-LICENSE").read_bytes():
                raise RuntimeError("retained upstream license mismatch")
            for name in ["resource.patch", "metadata.patch"]:
                base.run(["git", "apply", "--check", str(BASE / name)], name + "-check", output, phases, cwd=source)
                base.run(["git", "apply", str(BASE / name)], name + "-apply", output, phases, cwd=source)
            if (source / "LICENSE").read_bytes() != (BASE / "licenses/Ghostty-LICENSE").read_bytes():
                raise RuntimeError("patch changed upstream license")
            zig = zig_root / "zig"
            if base.run([str(zig), "version"], "zig-version", output, phases).strip() != pins["zig"]["version"]:
                raise RuntimeError("Zig version mismatch")
            env = os.environ.copy()
            env["ZIG_GLOBAL_CACHE_DIR"] = str(args.dependency_cache.resolve() if args.dependency_cache else work / "zig-global-cache")
            env["ZIG_LOCAL_CACHE_DIR"] = str(work / "zig-local-cache")
            sdk = None
            build = [str(zig), "build", "-Demit-lib-vt=true", "-Demit-xcframework=false",
                     "-Dapp-runtime=none", "-Doptimize=ReleaseSafe", "-Dsimd=false",
                     "-Dvt-features=-all,+render-state,+grid-introspection,+formatter,+input-encode",
                     "-j2", "--prefix", str(work / "install")]
            if system == "Darwin":
                env["MACOSX_DEPLOYMENT_TARGET"] = "14.0"
                sdk = base.run(["xcrun", "--sdk", "macosx", "--show-sdk-path"], "sdk", output, phases).strip()
                target, _ = base.mac_target(machine)
                build += ["-Dtarget=" + target]
            base.run(build, "engine-build", output, phases, timeout=600, cwd=source, env=env)
            library = work / "install/lib/libghostty-vt.a"
            document["compiled_library_sha256"] = sha(library)
            objects = []
            for name in ["probe", "resource", "collector", "integrated", "metadata"]:
                obj = work / (name + ".o")
                command = ["cc", "-std=c11", "-Werror", "-DGHOSTTY_STATIC", "-I", str(source / "include"),
                           "-I", str(BASE), "-c", str(BASE / (name + ".c")), "-o", str(obj)]
                if name != "probe":
                    command += ["-Wall", "-Wextra"]
                if sdk:
                    command += ["-arch", machine, "-isysroot", sdk]
                base.run(command, name + "-c", output, phases, env=env)
                objects.append(obj)
            transport = work / "transport.o"
            command = ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-c", str(HERE / "transport.c"), "-o", str(transport)]
            if sdk:
                command += ["-arch", machine, "-isysroot", sdk]
            base.run(command, "transport-c", output, phases, env=env)
            construction = work / "construction"
            command = ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", str(HERE / "construction.c"),
                       str(transport), "-o", str(construction)]
            if system == "Linux":
                command += ["-l", "util"]
            if sdk:
                command += ["-arch", machine, "-isysroot", sdk]
            base.run(command, "construction-c", output, phases, env=env)
            stdout = base.run([sys.executable, str(BASE / "run.py"), "--execute-fixture", str(construction)],
                              "construction-fixture", output, phases, timeout=2)
            if "construction_exhaustion=true owned_fds_closed=true child_started=false" not in stdout:
                raise RuntimeError("partial-construction cleanup did not complete")
            cleanup = work / "cleanup"
            command = ["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", str(HERE / "cleanup.c"),
                       "-o", str(cleanup)]
            if system == "Linux":
                command += ["-l", "util"]
            if sdk:
                command += ["-arch", machine, "-isysroot", sdk]
            base.run(command, "cleanup-c", output, phases, env=env)
            for name, arguments, expected in [
                ("cleanup-exited", [], "exited_child_cleanup=true no_signal_attempt=true owned_fds_closed=true"),
                ("cleanup-wait-error", ["reaped-error"], "wait_error_diagnostic=true operation=waitpid errno=ECHILD no_signal_attempt=true"),
                ("cleanup-master-first", ["master-first", str(HERE / "child.py")], "master_before_reap=true live_child_reaped=true owned_fds_closed=true"),
            ]:
                stdout = base.run([sys.executable, str(BASE / "run.py"), "--execute-fixture", str(cleanup),
                                   sys.executable, *arguments], name, output, phases, timeout=4)
                if expected not in stdout:
                    raise RuntimeError("direct-child cleanup did not complete: " + name)
            variants = [
                ("normal", None, None, None),
                ("duplicate-reply", "        self.end = n;", "        self.end = n;\n        if n > 0 { self.input[n] = b'!'; self.end += 1; }",
                 "child exact input order differs"),
                ("early-publication", "            self.start += n as usize;",
                 "            self.start += n as usize;\n            if self.start < self.end && self.staged.is_some() { let e = &mut self.engine; std::mem::swap(&mut e.last, &mut e.candidate); std::mem::swap(&mut e.state, &mut e.next_state); }",
                 "blocked/failing replies must not publish candidate"),
            ]
            for name, old, new, assertion in variants:
                rust_source = HERE / "pty.rs"
                if old:
                    original = (HERE / "pty-owner.rs").read_text()
                    if original.count(old) != 1:
                        raise RuntimeError("control mutation target is not unique: " + name)
                    mutated = work / (name + ".rs")
                    mutated.write_text(original.replace(old, new))
                    rust_source = work / (name + "-main.rs")
                    includes = [BASE / "integrated.rs", BASE / "metadata-owner.rs", mutated]
                    rust_source.write_text("#[allow(dead_code)] mod accepted {\n" +
                        "\n".join("include!(" + json.dumps(str(p)) + ");" for p in includes) +
                        "\n}\nfn main() { accepted::pty_main(); }\n")
                    document.setdefault("negative_controls", []).append({"name": name, "mutated_source_sha256": sha(mutated)})
                binary = work / (name + "-pty")
                command = ["rustc", "--edition=2024", str(rust_source)]
                for obj in [*objects, transport, library]:
                    command += ["-C", "link-arg=" + str(obj)]
                if system == "Linux":
                    command += ["-l", "util"]
                if sdk:
                    command += ["-C", "link-arg=-isysroot", "-C", "link-arg=" + sdk]
                base.run(command + ["-o", str(binary)], name + "-rust", output, phases, env=env)
                try:
                    stdout = base.run([sys.executable, str(BASE / "run.py"), "--execute-fixture", str(binary),
                                       sys.executable, str(HERE / "child.py")], name + "-fixture", output, phases, timeout=10)
                except RuntimeError:
                    phase = phases[-1]
                    log = (output / phase["log"]).read_text(errors="replace")
                    if assertion is None or phase["timed_out"] or phase["exit_code"] != 101 or assertion not in log:
                        raise
                    phase["expected_failure"] = True
                else:
                    if assertion or "pty_success=true" not in stdout:
                        raise RuntimeError("fixture did not complete the expected assertions: " + name)
                    document["observation_stdout"] = stdout[:16384]
        for name, expected in lock["harness_sha256"].items():
            if sha(BASE / name) != expected:
                raise RuntimeError("accepted input changed during run: " + name)
        if any(sha(HERE / n) != h for n, h in document["source_sha256"].items()):
            raise RuntimeError("new fixture input changed during run")
        document["state"] = "complete"
    except Exception as error:
        document.update(state="failed", error=f"{type(error).__name__}: {error}")
    finally:
        (output / "receipt.json").write_text(json.dumps(document, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"state": document["state"], "receipt": str(output / "receipt.json")}))
    return 0 if document["state"] == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())
