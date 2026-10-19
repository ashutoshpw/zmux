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


def exercise(root, source, binary, transport, sdk, baseline=False):
    sys.path.insert(0, str(root / "scripts"))
    from browser_process import run_browser

    subprocess.run([
        "xcrun", "swiftc", "-parse-as-library", "-swift-version", "6",
        "-strict-concurrency=complete", "-warnings-as-errors", "-O", "-g",
        "-target", platform.machine() + "-apple-macosx14.0", "-sdk", sdk,
        "-import-objc-header", str(root / "tools/native-fixtures/cloud-auth-callback.h"),
        str(transport), str(source), str(root / "apps/macos/Sources/CloudClient.swift"),
        str(root / "tools/native-fixtures/cloud-auth-callback.swift"),
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
            baseline = scratch / "CloudSettings-before.swift"
            baseline.write_bytes(subprocess.check_output([
                "git", "show", args.baseline_ref + ":apps/macos/Sources/CloudSettings.swift",
            ], cwd=root))
            exercise(root, baseline, scratch / "before", transport, sdk, baseline=True)
        exercise(root, root / "apps/macos/Sources/CloudSettings.swift", scratch / "current", transport, sdk)


if __name__ == "__main__":
    main()
