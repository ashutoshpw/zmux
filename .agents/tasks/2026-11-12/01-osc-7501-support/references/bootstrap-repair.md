# Managed shell bootstrap argument bound

## Failure evidence

The first integrated `scripts/check` receipt is preserved at
`/tmp/zmux-osc7501-integrated-check.json` (121 passed, 3 managed SSH failures,
3 ignored), and its original log remains at
`/tmp/zmux-osc7501-integrated-check.log`. All three SSH fixtures failed before
the lifecycle hello because the expanded bootstrap did not fit the remote exec
argument.

Before the repair, `/tmp/zmux-osc7501-arg-probe.rs` measured
`remote_command()` at **146,116 bytes**. Passing it directly to `/bin/true` failed
with `Argument list too long` (`E2BIG`, OS error 7). The new
`managed_ssh_bootstrap_fits_a_single_exec_argument` regression reproduced that
failure before the transport change.

## Repair

The local Rust runtime now compresses the trusted bootstrap with the pure-Rust
`flate2` backend and encodes it with the existing `data-encoding` dependency.
Expanded input is capped at 256 KiB and the encoded payload at 48 KiB. The
compressed value is cached per bootstrap variant. A small `/bin/sh -c` wrapper
decodes it into a shell variable and invokes the shell builtin `eval`; the
expanded script never becomes an exec argument. A sentinel retains the exact
trailing newline bytes through POSIX command substitution.

The decoder tries GNU Base64 `-d` and BSD Base64 `-D`, plus `gzip -dc`. If
`base64` or `gzip` is absent or decoding fails, it prints a diagnostic and
executes the original shell arguments without Zmux integration. Decoding does
not require Python 3. The local PTY stdin, bootstrap positional parameters,
single PTY writer, lifecycle order and existing cleanup ownership remain intact.

After repair, the exact same argument probe measured **41,957 bytes** and direct
exec succeeded. The expanded remote bootstrap remains over 128 KiB; the
owner-level compression round-trip test verifies byte-for-byte recovery of that
actual bootstrap.

## Focused evidence

- `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-session-runtime --lib --test shell_integration --test program_status`:
  1 compression unit test, 13 shell integration tests and 5 Program Status
  tests passed; 2 shell integration tests remain ignored under their existing
  conditions.
- `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux --test ssh`:
  all 3 loopback managed SSH tests passed.
- License inventory includes `flate2 1.1.10`, `miniz_oxide`, `crc32fast`,
  `adler2`, and `simd-adler32`; `scripts/license-inventory --check` passed.
- The artifact lane's exact test
  `linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain`
  now reuses the shared explicit-close scenario after its Ghostty query/final
  drain case. Together they verify Ghostty's distinct DA reply and clearing of
  committed records after the worker aborts its staged engine before close.
  This feature-gated test remains pending the accepted linked macOS artifact;
  Linux cannot establish that linked-engine acceptance.
- Full `scripts/check` is intentionally held until the concurrent desktop
  provider-epoch source fix is stable. Parent acceptance must fingerprint all
  relevant Rust/native files, embedded shell/Python payloads, manifests and
  `Cargo.lock` immediately before and after that full run.
