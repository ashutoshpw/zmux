"""Original MIT foreground fixture; own and reap the interactive shell directly."""
import json
import os
from pathlib import Path
import subprocess
import sys


root = Path(sys.argv[1])
with (root / "startup-count").open("ab") as count:
    count.write(b"x")
child = subprocess.Popen(["/bin/sh", "-i"])
try:
    report = {"wrapper_pid": os.getpid(), "server_pid": os.getppid(), "shell_pid": child.pid}
    pending = root / "owner.pending"
    pending.write_text(json.dumps(report))
    pending.replace(root / "owner.json")
    print("\nZMUX_ZELLIJ_FEASIBILITY_READY\n", flush=True)
    try:
        child.wait(timeout=75)
    except subprocess.TimeoutExpired:
        # The unreaped Popen child remains ours; never signal an adopted global PID.
        child.kill()
        child.wait(timeout=5)
finally:
    if child.poll() is None:
        child.kill()
        child.wait(timeout=5)
