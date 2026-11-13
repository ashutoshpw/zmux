# Checkpoint fixture Unix socket path repair

Foundation `43358543419` at `7a3a0a0` passed Ubuntu (763/0/7 plus 148 DOM
cases) but macOS stopped at two Session checkpoint tests: bind of the
`child-ready.sock` Unix socket returned `path must be shorter than SUN_LEN`.
Universal macOS `43358543431` stopped at the same two tests before app build.
Its separate mutation-child EOF diagnostic belonged to the successful
process-death regression and was not another primary failure. Full logs are
retained in `/tmp/zmux-osc7501-final-7a3a0a0/` and
`/tmp/zmux-osc7501-7a3-development-full.log`.

The checkpoint fixture appended its long UUID directory/socket names to
`std::env::temp_dir()`. Darwin's per-user TMPDIR can exceed the socket limit
after those additions. Parent applied the existing repository convention of a
canonical short `/tmp` fixture root, preserving unpredictable UUID isolation
and using mode 0700 for the created directory. No production, deadline, input,
checkpoint assertion or synchronization behavior changed.

The existing two checkpoint tests were run with a deliberately 105-byte
TMPDIR. Before the fix both failed with the exact SUN_LEN bind error; after the
fix both passed under the same long-root condition. Logs:
`/tmp/zmux-osc7501-checkpoint-long-tmp-red.log` and
`/tmp/zmux-osc7501-checkpoint-long-tmp-green.log`. No new mirror test or production
seam was added. The full check is running before commit/push. Since the test
fixture resides in a captured Session source file, fresh retained-library
linkage will also rerun at the new source; engine library bytes are unchanged.
