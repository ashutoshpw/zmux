"""Read host toolchain and the tested binary's concrete loader/ABI metadata."""
import json
from pathlib import Path
import platform
import re
import shutil


def collect(binary, sysroot, producer_receipt, output, environment, producer):
    phases = []

    def capture(command, label):
        path = output / (label + ".log")
        phases.append(producer.run(command, output, environment, path, 30))
        return path.read_text()

    host = re.search(r"^host: (.+)$", producer_receipt["rustc"], re.MULTILINE)
    if host is None:
        raise RuntimeError("producer did not record its Rust host target")
    target = host.group(1)
    if target not in ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin"):
        raise RuntimeError("package architecture/OS has not been reviewed or executed")
    rust = capture(["rustc", "+1.95.0", "--verbose", "--version"], "package-rustc")
    if rust != producer_receipt["rustc"]:
        raise RuntimeError("package Rust toolchain differs from producer")
    cc = shutil.which("cc", path=environment.get("PATH"))
    if cc is None:
        raise RuntimeError("native compiler unavailable for provenance")
    cc = Path(cc).resolve()
    compiler_version = capture([cc, "--version"], "native-compiler-version")
    description = capture(["file", "--brief", binary], "binary-format")
    result = {"target": target, "executed_host": platform.platform(), "binary_description": description.strip(),
              "compiler_path": str(cc), "compiler_sha256": producer.digest(cc), "compiler_version": compiler_version,
              "rustc_sha256": producer.digest(sysroot / "bin/rustc"), "rustc": rust,
              "reproducible_build_claim": False,
              "scope": "same-job executed host and recorded binary loader requirements; no universal or backward-OS claim",
              "phases": phases}
    if target == "x86_64-unknown-linux-gnu":
        if platform.system() != "Linux" or "x86-64" not in description:
            raise RuntimeError("ELF package does not match the executed host architecture")
        elf = capture(["readelf", "--file-header", "--program-headers", "--dynamic", "--version-info", binary], "elf-abi")
        versions = set(re.findall(r"GLIBC_(\d+(?:\.\d+)+)", elf))
        interpreter = re.search(r"Requesting program interpreter: ([^\]]+)", elf)
        if not versions or interpreter is None:
            raise RuntimeError("ELF loader/GLIBC requirements unavailable")
        result["abi"] = {"interpreter": interpreter.group(1),
                         "glibc_symbols": sorted(versions, key=lambda version: tuple(map(int, version.split('.')))),
                         "maximum_glibc_symbol_version": max(versions, key=lambda version: tuple(map(int, version.split('.')))),
                         "needed_system_libraries": re.findall(r"Shared library: \[([^\]]+)\]", elf)}
        result["system_linker_version"] = capture(["ld", "--version"], "system-linker-version")
        rust_lld = sysroot / "lib/rustlib" / target / "bin/rust-lld"
        result["rust_bundled_linker_version"] = capture([rust_lld, "-flavor", "gnu", "--version"], "rust-linker-version")
        result["host_libc_version"] = capture(["ldd", "--version"], "host-libc-version")
    else:
        if platform.system() != "Darwin" or "arm64" not in description:
            raise RuntimeError("Mach-O package does not match the executed host architecture")
        load = capture(["otool", "-hV", "-l", binary], "macho-load-commands")
        dylibs = capture(["otool", "-L", binary], "macho-dylibs")
        minimum = re.findall(r"\bminos ([\d.]+)", load)
        sdk = re.findall(r"\bsdk ([\d.]+)", load)
        if len(minimum) != 1 or len(sdk) != 1:
            raise RuntimeError("Mach-O deployment target/SDK load command unavailable")
        result["abi"] = {"minimum_macos_load_command": minimum[0], "sdk_load_command": sdk[0],
                         "system_dylibs": [line.strip() for line in dylibs.splitlines()[1:]]}
        result["sdk_version"] = capture(["xcrun", "--show-sdk-version"], "sdk-version").strip()
        result["sdk_build_version"] = capture(["xcrun", "--show-sdk-build-version"], "sdk-build-version").strip()
        result["xcode_version"] = capture(["xcodebuild", "-version"], "xcode-version")
        # Apple ld can print its version and then exit1 for no object inputs.
        # Keep that specific probe visible; all build/owner phases remain strict.
        path = output / "apple-linker-version.log"
        records = []
        try:
            producer.bounded_runner.run(["xcrun", "ld", "-v"], path.stem, output, records, timeout=30, env=environment)
        except RuntimeError:
            if not records or records[0]["timed_out"] or records[0]["exit_code"] != 1:
                raise
        record = records[0]
        text = path.read_text()
        if record["log_bytes_seen"] != record["log_bytes_retained"] or not re.search(r"PROJECT:ld|ld64-|ld-\d", text):
            raise RuntimeError("Apple linker version probe did not identify the linker")
        phases.append({**record, "sha256": producer.digest(path)})
        result["system_linker_version"] = text
        result["signature_display"] = capture(["codesign", "--display", "--verbose=4", binary], "signature-display")
        result["application_distribution_signing_established"] = False
    path = output / "platform.json"
    path.write_text(json.dumps(result, indent=2) + "\n")
    return result
