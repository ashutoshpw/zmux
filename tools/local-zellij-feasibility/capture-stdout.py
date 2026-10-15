#!/usr/bin/env python3
"""Separate structured stdout inside the existing bounded parent's process group."""
import argparse
from pathlib import Path
import subprocess
import sys

MAX_STDOUT_BYTES = 8 * 1024 * 1024


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not args.output.is_absolute() or not command:
        parser.error("structured stdout requires an absolute fresh output file and command")
    # This helper must run under the existing retained ProcessGroup owner. Cargo
    # inherits that same group, stderr pipe, cwd and sanitized environment. It
    # never starts another session/group or owns an adopted/global process ID.
    # The parent keeps its deadline even if either pipe closes or a descendant
    # retains stdout. Its final group cleanup also covers command descendants.
    with args.output.open("xb") as output:
        child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE)
        try:
            retained = 0
            while chunk := child.stdout.read(65536):
                saved = chunk[:max(0, MAX_STDOUT_BYTES - retained)]
                output.write(saved)
                retained += len(saved)
                if len(saved) != len(chunk):
                    raise RuntimeError("structured stdout exceeded 8 MiB")
            code = child.wait()
        finally:
            child.stdout.close()
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=1)
    return code


if __name__ == "__main__":
    sys.exit(main())
