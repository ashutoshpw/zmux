#!/usr/bin/env python3
"""Admit one exact independently verified optional native package; never download or execute it."""
import argparse
import json
from pathlib import Path
import sys
import tarfile

from package_admission import Rejected, accept, verify_payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify-payload", action="store_true")
    args = parser.parse_args()
    if args.verify_payload and args.output:
        parser.error("payload verification cannot extract")
    try:
        if args.verify_payload:
            result = verify_payload(args.root)
            if result["target"] != args.target:
                raise Rejected("packaged dependency target differs")
        else:
            result = accept(args.root, args.target, args.output)
        print(json.dumps(result))
    except (Rejected, OSError, ValueError, KeyError, TypeError, StopIteration, EOFError, tarfile.TarError) as error:
        print(f"optional Zellij package rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
