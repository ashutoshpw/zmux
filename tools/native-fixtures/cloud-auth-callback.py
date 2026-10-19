#!/usr/bin/env python3
"""Exercise the real Cloud settings completion at its foreign-queue boundary."""
import argparse
from pathlib import Path
import platform
import resource
import signal
import subprocess
import sys
import tempfile


def source_at_revision(root, revision, relative_path, scratch):
    spec = f"{revision}:{relative_path.as_posix()}"
    exists = subprocess.run(["git", "cat-file", "-e", spec], cwd=root,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if exists.returncode != 0:
        return None
    source = scratch / f"baseline-{relative_path.name}"
    source.write_bytes(subprocess.check_output(["git", "show", spec], cwd=root))
    return source


def exercise(root, source, binary, transport, sdk, baseline=False,
             client_source=None, support_sources=()):
    sys.path.insert(0, str(root / "scripts"))
    from browser_process import run_browser

    swift_sources = [
        client_source or root / "apps/macos/Sources/CloudClient.swift",
        *support_sources,
        source,
        root / "tools/native-fixtures/cloud-auth-callback.swift",
    ]
    subprocess.run([
        "xcrun", "swiftc", "-parse-as-library", "-swift-version", "6",
        "-strict-concurrency=complete", "-warnings-as-errors", "-O", "-g",
        "-target", platform.machine() + "-apple-macosx14.0", "-sdk", sdk,
        "-import-objc-header", str(root / "tools/native-fixtures/cloud-auth-callback.h"),
        str(transport), *map(str, swift_sources),
        "-framework", "AppKit", "-framework", "AuthenticationServices",
        "-framework", "Security", "-framework", "CryptoKit", "-framework", "LocalAuthentication",
        "-o", str(binary),
    ], check=True, timeout=90)
    result = run_browser([str(binary)], timeout=15)
    print(result.stdout, end="", flush=True)
    if result.timed_out or result.output_truncated:
        raise RuntimeError("Cloud auth callback probe timed out or exceeded its output bound")
    if baseline:
        # macOS may send the dispatch assertion to CrashReporter instead of stderr.
        # The actual pre-fix closure has only the actor check before its inner Task:
        # require a trap inside that exact call, before the SDK stand-in can return.
        if (result.returncode not in (-signal.SIGTRAP, -signal.SIGILL)
                or "Invoking actual Cloud settings completion from a foreign queue" not in result.stdout
                or "Foreign authentication completion returned" in result.stdout):
            print(result.stderr, end="", file=sys.stderr)
            raise RuntimeError("Pre-fix Cloud settings did not reproduce the actor-executor trap")
        print("Baseline actual sign-in callback reproduced the foreign-queue actor trap.", flush=True)
    else:
        print(result.stderr, end="", file=sys.stderr)
        if result.returncode != 0:
            raise RuntimeError("Current Cloud settings callback failed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-ref")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        raise SystemExit("Native authentication callback acceptance requires macOS and its SDK")
    # The baseline intentionally traps. Do not leave a core dump on the fixture host.
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="zmux-cloud-auth-callback-") as directory:
        scratch = Path(directory)
        transport = scratch / "authentication-transport.o"
        sdk = subprocess.check_output(["xcrun", "--sdk", "macosx", "--show-sdk-path"], text=True).strip()
        subprocess.run([
            "xcrun", "clang", "-fobjc-arc", "-fblocks", "-Wall", "-Wextra", "-Werror",
            "-target", platform.machine() + "-apple-macosx14.0", "-isysroot", sdk,
            "-c", str(root / "tools/native-fixtures/cloud-auth-callback.m"),
            "-o", str(transport),
        ], check=True, timeout=30)
        if args.baseline_ref:
            baseline_settings = source_at_revision(
                root, args.baseline_ref, Path("apps/macos/Sources/CloudSettings.swift"), scratch)
            baseline_client = source_at_revision(
                root, args.baseline_ref, Path("apps/macos/Sources/CloudClient.swift"), scratch)
            if baseline_settings is None or baseline_client is None:
                raise RuntimeError("The callback baseline must contain both CloudSettings.swift and CloudClient.swift")
            baseline_support = [
                source for source in (
                    source_at_revision(root, args.baseline_ref,
                                       Path("apps/macos/Sources/CloudDeviceIdentity.swift"), scratch),
                    source_at_revision(root, args.baseline_ref,
                                       Path("apps/macos/Sources/CloudTerminalTransport.swift"), scratch),
                ) if source is not None
            ]
            exercise(root, baseline_settings, scratch / "before", transport, sdk, baseline=True,
                     client_source=baseline_client, support_sources=baseline_support)
        current_support = [root / "apps/macos/Sources/CloudDeviceIdentity.swift",
                           root / "apps/macos/Sources/CloudTerminalTransport.swift"]
        exercise(root, root / "apps/macos/Sources/CloudSettings.swift", scratch / "current", transport, sdk,
                 client_source=root / "apps/macos/Sources/CloudClient.swift",
                 support_sources=current_support)


if __name__ == "__main__":
    main()
