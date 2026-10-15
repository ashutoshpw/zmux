# Feasibility provenance

All C/Python drivers, the private byte broker and workflow are original Zmux code
under the repository MIT license. No cmux implementation, tests, assets or binaries
were imported or translated. No production code calls these fixture functions.

The runner downloads the official Zellij 0.43.0 **no-web** release for its host into
a private temporary directory. `pins.json` records exact archive and extracted binary
SHA-256 hashes. Archive hashes come from GitHub's release asset digests; executable
hashes come from that release's published `.sha256sum` files. Its annotated tag resolves
to commit `23bb542548d517f7f20a7fdd14a943f1a610df37`.

Zellij's root [license](https://github.com/zellij-org/zellij/blob/23bb542548d517f7f20a7fdd14a943f1a610df37/LICENSE.md)
is MIT, copyright 2020 Zellij contributors. A verbatim copy is retained in
[licenses/Zellij-LICENSE](licenses/Zellij-LICENSE). The downloaded executable can
contain separately licensed components; this root-license record is not a complete
binary redistribution notice audit. The workflow does not redistribute the archive,
executable, compiled fixture library or other upstream assets.

Darwin SDK headers/system libraries are provided by the host's Xcode installation.
The fixture calls public declarations and defines no private kernel structure or
copied Darwin implementation. No package/system install or HOME change is performed.

The separate experimental `produce-patched.py` verifies that same source commit
from the archive hash in `patched-producer-lock.json` and applies the original
Zmux stdin-interruption patch in `patches/`. Its public-client regression example
is original MIT fixture code. The producer retains the upstream MIT license and
records source, lock, patch, fixture, toolchain, flags and executable hashes.
It inventories upstream prebuilt WASM assets by hash; those assets are retained
from the verified archive and are not rebuilt by this producer. Build processes
use an environment allowlist excluding inherited compiler/linker/Rust/Cargo
overrides, and effective Cargo configuration files are recorded by hash.
It does not replace the official-binary runner or product acquisition pins and
is not an app redistribution pipeline; full linked-component distribution
notices and actual Linux/Darwin owner acceptance remain separate gates.
