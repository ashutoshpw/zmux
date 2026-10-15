# Provenance

All source in this directory is original MIT Zmux code. No cmux GPL/BUSL source,
asset, implementation or test was copied or translated. POSIX APIs and Python's
standard library provide the real kernel PTY peer/transport.

The fixture includes the accepted original owner sources unchanged and links the
same pinned MIT Ghostty `22fa134a07f5749edd9e92c6bc17d8aaf61843be` with unchanged,
separately hashed original resource/metadata patches. It reuses Zig 0.16 and the
same pinned dependencies. See [the retained notices](../terminal-feasibility/NOTICE.md)
and [Ghostty MIT license](../terminal-feasibility/licenses/Ghostty-LICENSE).

`base-lock.json` retains 28 accepted inputs at b86a700 unchanged and separately
records the original/revised subprocess runner, shared lifecycle helper and
regression-test hashes. The runner verifies every locked input before/after each run. This directory adds no
upstream patch or dependency; the runner-only repair changes process cleanup.
The runner uses actual native PTY APIs, not a copied reference transport or mock.

Evidence outputs are bounded JSON/logs. Compiled libraries/executables and downloaded
sources/toolchains are temporary and are not a distribution artifact. Shipping any
library still requires its full linked-component/compiler/Unicode/platform notice
audit. Linux portable evidence does not establish Darwin/native production behavior.
