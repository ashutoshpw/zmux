#!/usr/bin/env python3
"""Prove the native repaint regression against the prior zmux renderer only.

This deliberately mixed diagnostic build is never a product artifact. Restore
the committed renderer even on failure; the next workflow step rebuilds the app.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'apps/macos/Sources/TerminalView.swift'
BASE = '53a98b5:apps/macos/Sources/TerminalView.swift'

def main():
    saved = SOURCE.read_bytes()
    diagnostic = ROOT / 'dist/repaint-baseline'
    diagnostic.mkdir(parents=True, exist_ok=False)
    receipt = {'variant': 'prior_terminal_renderer_with_current_repaint_assertion',
               'renderer_git_object': BASE, 'product_acceptance': False, 'success': False}
    try:
        previous = subprocess.check_output(['git','show',BASE], cwd=ROOT, timeout=10)
        receipt['renderer_sha256'] = hashlib.sha256(previous).hexdigest()
        SOURCE.write_bytes(previous)
        subprocess.run(['scripts/build-macos-dev','--architecture','host'], cwd=ROOT, check=True, timeout=600)
        with (diagnostic/'check.log').open('wb') as log:
            run = subprocess.run(['scripts/check-native-sidebar','--skip-build','--architecture','host'],
                                 cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=180)
        error = (ROOT/'dist/native-sidebar.stderr.log').read_text()
        lifecycle = json.loads((ROOT/'dist/native-sidebar-lifecycle.json').read_text())
        if run.returncode == 0 or 'Metadata-only terminal output requested a pixel repaint' not in error:
            raise RuntimeError('Prior renderer did not fail for the intended repaint contract')
        if lifecycle.get('physical_cleanup_complete') is not True:
            raise RuntimeError('Regression app did not confirm physical cleanup')
        try:
            os.kill(lifecycle['app_pid'], 0)
        except ProcessLookupError:
            pass
        else:
            raise RuntimeError('Regression app PID remains present')
        receipt.update(success=True, intended_failure=True, native_lifecycle=lifecycle)
    finally:
        SOURCE.write_bytes(saved)
        for file in (ROOT/'dist').glob('native-sidebar*'):
            if file.is_file(): shutil.copy2(file, diagnostic/file.name)
        (diagnostic/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')

if __name__ == '__main__':
    main()
