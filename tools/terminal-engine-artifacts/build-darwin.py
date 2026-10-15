#!/usr/bin/env python3
"""Build two retained macOS slices; never selects or links an application engine."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import sys

from darwin_platform import inspect_darwin_archive, inspect_macho
from verify import (HERE, REPO, Rejected, checked_file, digest, document,
                    read_bounded, verify_notice_review, verify_notices)

BASE = REPO / "tools/terminal-feasibility"
WORKFLOW = REPO / ".github/workflows/terminal-engine-artifacts.yml"
MAX_BUNDLE_BYTES = 256 * 1024 * 1024
REQUIRED_SYMBOLS = {
    "ghostty_terminal_new", "ghostty_terminal_free", "ghostty_terminal_vt_write",
    "zmux_ghostty_terminal_new_bounded", "zmux_ghostty_terminal_clone_storage",
    "zmux_ghostty_terminal_new_metadata_bounded_v1",
}


def sha(path):
    return digest(read_bounded(path, 128 * 1024 * 1024))


def sources():
    candidate = document(HERE / "candidate.json")
    pins = document(BASE / "pins.json")
    recipe = document(HERE / "darwin-recipe.json")
    if recipe["recipe_version"] != 1 or (recipe["xcode_version"], recipe["sdk_version"], recipe["minimum_macos"]) != ("16.4", "15.5", "14.0"):
        raise Rejected("unsupported Xcode/SDK/minimum macOS recipe")
    if recipe["zig_version"] != pins["zig"]["version"] or recipe["rust_version"] != pins["rust_version"]:
        raise Rejected("recipe toolchain differs from reviewed pins")
    expected_targets = [
        {"architecture": "arm64", "zig": "aarch64-macos.14.0", "rust": "aarch64-apple-darwin", "execute": True},
        {"architecture": "x86_64", "zig": "x86_64-macos.14.0", "rust": "x86_64-apple-darwin", "execute": False},
    ]
    if recipe["targets"] != expected_targets or recipe["build_jobs"] != 2 or recipe["optimize"] != "ReleaseSafe" or recipe["simd"] is not False:
        raise Rejected("unsupported slice count, architecture, execution or resource recipe")
    if recipe["vt_features"] != "-all,+render-state,+grid-introspection,+formatter,+input-encode":
        raise Rejected("recipe changes the reviewed minimal VT feature graph")
    for name, expected in candidate["repository_inputs_sha256"].items():
        checked_file(REPO, name, expected)
    checked_file(HERE, "upstream-inputs.json", candidate["upstream_inputs_sha256"])
    verify_notices()
    verify_notice_review(candidate)
    symbols = document(HERE / "linux-runtime-symbols.json")
    if symbols["artifact_sha256"] != candidate["artifact"]["sha256"]:
        raise Rejected("Linux symbol baseline does not bind the retained reviewed artifact")
    paths = [p for p in HERE.rglob("*") if p.is_file() and "__pycache__" not in p.parts]
    paths += [REPO / name for name in candidate["repository_inputs_sha256"]]
    paths.append(WORKFLOW)
    frozen = {str(p.relative_to(REPO)): sha(p) for p in sorted(set(paths))}
    return recipe, pins, frozen, symbols["symbols"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--reuse-artifacts", type=Path,
                        help="Explicit prior output bundle/receipt; reuse only matching retained slices")
    parser.add_argument("--check-recipe", action="store_true",
                        help="Verify source/notice/recipe inputs without downloads or compilation")
    args = parser.parse_args()
    if args.check_recipe:
        recipe, _, frozen, _ = sources()
        print(json.dumps({"state": "recipe_verified", "recipe": recipe,
                          "frozen_inputs": len(frozen), "engine_built": False,
                          "darwin_execution_verified": False}, indent=2))
        return 0
    if args.output is None:
        parser.error("--output is required for a build")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    receipt = {"schema_version": 1, "state": "starting", "phases": [], "downloads": [],
               "targets": [], "claims": {"engine_selected": False, "app_linked": False,
                   "universal_assembled": False, "intel_execution_verified": False,
                   "minimum_os_execution_verified": False, "bit_identical_rebuild_verified": False,
                   "performance_measured": False}}
    try:
        recipe, pins, frozen, linux_symbols = sources()
        receipt.update(recipe=recipe, pins=pins, source_sha256=frozen)
        if platform.system() != "Darwin" or platform.machine() != "arm64":
            raise Rejected("this recipe requires a macOS arm64 host; no cross-runtime substitution")
        spec = importlib.util.spec_from_file_location("accepted_runner", BASE / "run.py")
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        phases = receipt["phases"]
        environment = os.environ.copy()
        environment["DEVELOPER_DIR"] = recipe["developer_dir"]
        environment["MACOSX_DEPLOYMENT_TARGET"] = recipe["minimum_macos"]

        def run(command, label, timeout=60, cwd=None):
            return runner.run(command, label, output, phases, timeout=timeout, cwd=cwd, env=environment).strip()

        receipt["source_sha"] = run(["git", "rev-parse", "HEAD"], "source", cwd=REPO)
        xcode = run(["xcodebuild", "-version"], "xcode")
        if xcode.splitlines()[0] != "Xcode " + recipe["xcode_version"]:
            raise Rejected("selected Xcode differs from the pinned recipe")
        sdk = Path(run(["xcrun", "--sdk", "macosx", "--show-sdk-path"], "sdk-path"))
        sdk_version = run(["xcrun", "--sdk", "macosx", "--show-sdk-version"], "sdk-version")
        if sdk_version != recipe["sdk_version"]:
            raise Rejected("selected SDK differs from the pinned recipe")
        clang = run(["xcrun", "--find", "clang"], "clang-path")
        clang_version = run([clang, "--version"], "clang-version")
        rust = run(["rustc", "--version"], "rust-version")
        if not rust.startswith("rustc " + recipe["rust_version"] + " "):
            raise Rejected("Rust version differs from the reviewed pin")
        sdk_settings = sdk / "SDKSettings.json"
        receipt["developer"] = {"xcode": xcode, "sdk_path": str(sdk), "sdk_version": sdk_version,
            "sdk_settings_sha256": sha(sdk_settings), "clang": clang_version, "rust": rust,
            "host_macos": platform.mac_ver()[0]}
        work = output / "work"
        work.mkdir()
        zig_pin = pins["zig"]["hosts"]["Darwin-arm64"]
        zig_root = runner.download(zig_pin, work, receipt["downloads"])
        source = runner.download(pins["ghostty"], work, receipt["downloads"])
        if run([str(zig_root / "zig"), "version"], "zig-version") != recipe["zig_version"]:
            raise Rejected("Zig version differs from the reviewed pin")
        # A nested Git root prevents git apply from silently skipping paths when
        # output is beneath an ignored directory of the caller's checkout.
        run(["git", "init", "--quiet"], "isolate-patch-root", cwd=source)
        for patch in ("resource.patch", "metadata.patch"):
            run(["git", "apply", "--check", str(BASE / patch)], patch + "-check", cwd=source)
            run(["git", "apply", str(BASE / patch)], patch + "-apply", cwd=source)
        upstream = document(HERE / "upstream-inputs.json")
        for name, expected in upstream["patched_header_sha256"].items():
            checked_file(source, name, expected)
        if (source / "LICENSE").read_bytes() != (HERE / "licenses/Ghostty-MIT").read_bytes():
            raise Rejected("upstream source license changed")
        for name, recorded in upstream["zig"]["referenced_runtime_source"].items():
            checked_file(zig_root, name, recorded["sha256"])
        receipt["runtime_notice_sources_matched"] = len(upstream["zig"]["referenced_runtime_source"])
        bundle = output / "bundle"
        bundle.mkdir()
        retained_sources = bundle / "sources/repository"
        retained_sources.mkdir(parents=True)
        for name, expected in frozen.items():
            relative = (REPO / name).resolve().relative_to(REPO)
            target_path = retained_sources / relative
            target_path.parent.mkdir(parents=True, exist_ok=True)
            target_path.write_bytes(checked_file(REPO, name, expected))
        (bundle / "sources/manifest.json").write_text(json.dumps({
            "schema_version": 1, "source_sha": receipt["source_sha"],
            "repository_inputs_sha256": frozen,
            "scope": "reviewed tooling, source pins, raw patches and original C/Rust fixture inputs; upstream full source/toolchain archives are referenced by URL/hash"
        }, indent=2) + "\n")
        shutil.copytree(source / "include", bundle / "include")
        notice_inventory = document(HERE / "notices.json")
        for row in notice_inventory["notices"]:
            target = bundle / row["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(HERE / row["path"], target)
        for name in ("notices.json", "upstream-inputs.json", "linux-runtime-symbols.json"):
            shutil.copyfile(HERE / name, bundle / name)
        shutil.copyfile(HERE / "evidence/notice-coverage-review.json", bundle / "notice-coverage-review.json")
        receipt["headers_sha256"] = {str(p.relative_to(bundle)): sha(p) for p in sorted((bundle / "include").rglob("*.h"))}
        receipt["library_recipe"] = {"recipe": recipe, "ghostty": pins["ghostty"], "zig": zig_pin,
            "patches_sha256": {name: sha(BASE / name) for name in ("resource.patch", "metadata.patch")},
            "developer": receipt["developer"], "headers_sha256": receipt["headers_sha256"]}
        prior = None
        if args.reuse_artifacts:
            prior = document(args.reuse_artifacts.resolve() / "receipt.json")
            if prior["library_recipe"] != receipt["library_recipe"]:
                raise Rejected("retained build recipe/SDK/header identity differs; reuse refused")
            receipt["reused_receipt_sha256"] = sha(args.reuse_artifacts.resolve() / "receipt.json")
        environment["ZIG_GLOBAL_CACHE_DIR"] = str(work / "zig-global-cache")
        environment["ZIG_LOCAL_CACHE_DIR"] = str(work / "zig-local-cache")
        for target in recipe["targets"]:
            arch = target["architecture"]
            directory = bundle / arch
            directory.mkdir()
            result = {"architecture": arch, "zig_target": target["zig"], "rust_target": target["rust"],
                      "execution": "not_run_cross_architecture", "state": "starting"}
            receipt["targets"].append(result)
            library = directory / "libghostty-vt.a"
            retained = next((item for item in prior["targets"] if item["architecture"] == arch and "library" in item), None) if prior else None
            if retained:
                raw = checked_file(args.reuse_artifacts.resolve(), retained["library"]["path"], retained["library"]["sha256"], 64 * 1024 * 1024)
                library.write_bytes(raw)
                result["library_reused"] = True
            else:
                prefix = work / ("install-" + arch)
                command = [str(zig_root / "zig"), "build", "-Demit-lib-vt=true", "-Demit-xcframework=false",
                    "-Dapp-runtime=none", "-Doptimize=" + recipe["optimize"], "-Dsimd=false",
                    "-Dvt-features=" + recipe["vt_features"], "-Dtarget=" + target["zig"],
                    "-j2", "--prefix", str(prefix)]
                run(command, "build-" + arch, timeout=600, cwd=source)
                shutil.copyfile(prefix / "lib/libghostty-vt.a", library)
                result["library_reused"] = False
            # Record the physical library immediately so later failures retain a
            # reusable hash-bound slice instead of discarding a completed build.
            result["library"] = {"path": str(library.relative_to(output)), "sha256": sha(library)}
            result["inventory"] = inspect_darwin_archive(library, arch)
            symbols = run(["xcrun", "nm", "-gUj", str(library)], "defined-symbols-" + arch)
            defined = sorted({line.strip().removeprefix("_") for line in symbols.splitlines()
                              if line.strip() and not line.rstrip().endswith(":")})
            if not REQUIRED_SYMBOLS <= set(defined):
                raise Rejected("required C/extension exports are absent: " + arch)
            result["symbol_delta_from_linux"] = {"added": sorted(set(defined) - set(linux_symbols)),
                                                  "removed": sorted(set(linux_symbols) - set(defined))}
            result["undefined_symbols_log"] = "undefined-symbols-" + arch + ".log"
            run(["xcrun", "nm", "-u", str(library)], "undefined-symbols-" + arch)
            objects = []
            common = [clang, "-std=c11", "-Werror", "-DGHOSTTY_STATIC", "-arch", arch,
                "-isysroot", str(sdk), "-mmacosx-version-min=" + recipe["minimum_macos"],
                "-I", str(bundle / "include"), "-I", str(BASE)]
            for name in ("probe", "resource", "collector", "integrated", "metadata"):
                obj = directory / (name + ".o")
                run([*common, "-c", str(BASE / (name + ".c")), "-o", str(obj)], name + "-c-" + arch)
                result.setdefault("c_object_inventories", {})[obj.name] = inspect_macho(
                    read_bounded(obj, 16 * 1024 * 1024), arch)
                objects.append(obj)
            result["objects_sha256"] = {p.name: sha(p) for p in objects}
            c_probe = directory / "c-abi"
            run([*common, "-Wall", "-Wextra", str(HERE / "darwin-abi.c"), str(library), "-o", str(c_probe)], "c-abi-link-" + arch)
            binary = directory / "metadata-fixture"
            command = ["rustc", "--edition=2024", str(BASE / "metadata.rs"), "--target", target["rust"], "-C", "linker=" + clang]
            for item in [*objects, library]:
                command += ["-C", "link-arg=" + str(item)]
            command += ["-C", "link-arg=-isysroot", "-C", "link-arg=" + str(sdk),
                        "-C", "link-arg=-mmacosx-version-min=" + recipe["minimum_macos"], "-o", str(binary)]
            run(command, "rust-c-abi-link-" + arch)
            result["binaries_sha256"] = {p.name: sha(p) for p in (c_probe, binary)}
            result["state"] = "compiled_linked_inspected"
            if target["execute"]:
                for label, executable, expected in (("c-abi", c_probe, "darwin_abi_success=true"),
                                                    ("metadata", binary, "metadata_success=true")):
                    stdout = run([sys.executable, str(BASE / "run.py"), "--execute-fixture", str(executable)], label + "-native-" + arch, timeout=10)
                    if expected not in stdout:
                        raise Rejected("native fixture did not complete: " + label)
                    result[label + "_stdout"] = stdout[:16384]
                result["execution"] = "passed_on_arm64_runner"
            result["state"] = "complete"
        receipt["bundle_bytes"] = sum(p.stat().st_size for p in bundle.rglob("*") if p.is_file())
        if receipt["bundle_bytes"] > MAX_BUNDLE_BYTES:
            raise Rejected("retained artifact bundle exceeds 256 MiB")
        if any(sha(REPO / name) != expected for name, expected in frozen.items()):
            raise Rejected("source inputs changed during artifact production")
        receipt["state"] = "complete"
        receipt["scope"] = "two thin macOS artifacts with declared minOS/C ABI links; arm64 execution only; independent Darwin inventory review still required"
    except Exception as error:
        receipt.update(state="failed", error=f"{type(error).__name__}: {error}")
    finally:
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"state": receipt["state"], "receipt": str(output / "receipt.json")}))
    return 0 if receipt["state"] == "complete" else 1


if __name__ == "__main__":
    sys.exit(main())
