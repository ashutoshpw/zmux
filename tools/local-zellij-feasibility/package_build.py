"""Pinned plugin rebuild and compiler-unit evidence for optional Zellij packages."""
import json
from pathlib import Path
import re
import shutil

PLUGINS = ("compact-bar", "status-bar", "tab-bar", "strider", "session-manager",
           "configuration", "plugin-manager", "about", "share", "multiple-select")


def compiler_units(log, output, target, role, digest):
    """Retain actual Cargo compiler units; metadata's dev graph is not this set."""
    units = []
    scripts = []
    finished = False
    with log.open() as stream:
        for line in stream:
            if not line.startswith("{"):
                continue
            event = json.loads(line)
            if event.get("reason") == "compiler-artifact":
                filenames = event["filenames"]
                if any(not Path(name).is_relative_to(target) for name in filenames):
                    raise RuntimeError("compiler artifact escaped its nominated Cargo cache")
                scope = "wasm32-wasip1" if any(Path(name).is_relative_to(target / "wasm32-wasip1") for name in filenames) else "host"
                units.append({"package_id": event["package_id"], "target": event["target"],
                              "features": event["features"], "profile": event["profile"],
                              "scope": scope, "filenames": filenames, "role": role})
            elif event.get("reason") == "build-script-executed":
                scripts.append({key: event[key] for key in ("package_id", "linked_libs", "linked_paths", "cfgs", "env", "out_dir")})
            elif event.get("reason") == "build-finished":
                finished = event["success"]
    if not finished or not units:
        raise RuntimeError("successful Cargo compiler-unit capture missing")
    document = {"role": role, "log_sha256": digest(log), "units": units, "build_scripts": scripts}
    output.write_text(json.dumps(document, indent=2) + "\n")
    return {"path": str(output), "sha256": digest(output), "unit_count": len(units),
            "package_ids": sorted({unit["package_id"] for unit in units})}


def rebuild_plugins(source, target, cargo, environment, output, run, digest):
    consts = source / "zellij-utils/src/consts.rs"
    selected = re.findall(r'add_plugin!\(assets, "([a-z-]+)\.wasm"\)', consts.read_text())
    if tuple(selected) != PLUGINS:
        raise RuntimeError("reviewed release plugin allowlist changed")
    command = cargo + ["build", "--locked", "--release", "--target", "wasm32-wasip1",
                       "--message-format=json-render-diagnostics"]
    for name in PLUGINS:
        command += ["-p", name]
    phase = run(command, source, environment, output / "plugins-build.log")
    capture = compiler_units(output / "plugins-build.log", output / "plugins-compiler-units.json",
                             target, "embedded_wasm_and_build_support", digest)
    assets = []
    for name in PLUGINS:
        original = source / "zellij-utils/assets/plugins" / (name + ".wasm")
        built = target / "wasm32-wasip1/release" / (name + ".wasm")
        if not 8 <= built.stat().st_size <= 16 * 1024 * 1024:
            raise RuntimeError("rebuilt plugin exceeded its byte bound")
        old_sha = digest(original)
        shutil.copy2(built, original)
        # Cargo must observe the copied bytes even with a reusable native cache.
        original.touch()
        assets.append({"name": name, "path": str(original.relative_to(source)),
                       "archive_sha256": old_sha, "rebuilt_sha256": digest(original),
                       "bytes": original.stat().st_size})
    path = output / "embedded-assets-manifest.json"
    path.write_text(json.dumps({"rebuilt": True, "target": "wasm32-wasip1", "assets": assets,
                                "consts_sha256": digest(consts), "compiler_units": capture}, indent=2) + "\n")
    return {"build": phase, "compiler_units": capture, "manifest_sha256": digest(path)}


def validate_plugins(source, target, cargo, environment, output, fixture, run, digest):
    example = source / "zellij-server/examples/zmux-validate-wasm.rs"
    example.parent.mkdir(exist_ok=True)
    shutil.copy2(fixture, example)
    command = cargo + ["build", "--locked", "--release", "--no-default-features",
                       "-p", "zellij-server", "--features", "zellij-utils/vendored_curl",
                       "--example", "zmux-validate-wasm"]
    build = run(command, source, environment, output / "wasm-validator-build.log")
    arguments = [target / "release/examples/zmux-validate-wasm"]
    arguments += [source / "zellij-utils/assets/plugins" / (name + ".wasm") for name in PLUGINS]
    result = run(arguments, source, environment, output / "wasm-validation.log", 300)
    lines = (output / "wasm-validation.log").read_text().splitlines()
    if len(lines) != len(PLUGINS) or any(not line.startswith("VALIDATED ") for line in lines):
        raise RuntimeError("not every rebuilt plugin reached pinned-runtime validation")
    return {"fixture_sha256": digest(fixture), "build": build, "validation": result,
            "scope": "pinned Wasmtime 29 Cranelift module compilation; optional plugin UI behavior untested"}
