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
