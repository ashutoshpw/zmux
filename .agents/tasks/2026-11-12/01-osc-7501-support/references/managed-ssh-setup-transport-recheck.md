# Managed SSH setup transport recheck

**Date:** 2026-11-13 UTC
**Baseline:** `2efb76d9bcbb9e0a00015602b8bcaf81c00920de`
**Scope:** Read-only review of the `apps/cli/tests/ssh.rs` working-tree diff and
`managed-ssh-setup-transport-repair.md`. Only this recheck note was created.

## Verdict

The private-file transport fixes the observed multiline PTY corruption and
preserves the intended readiness and provider assertions. One P2 test-harness
failure path remains: both setup-file source commands use `;`, so the provider
can launch even when sourcing its required environment setup fails. Gate the
provider command on successful sourcing before treating this transport repair
as fully closed.

## Finding

**P2 — Provider launch is not conditional on setup-file success** —
`apps/cli/tests/ssh.rs:517-522,729-735`.

The Codex command is `. '<setup-path>'; command codex fixture-daemon-start &&
codex`; OpenCode similarly uses `. '<setup-path>'; opencode ...`. These lines
are sent to the interactive remote shell. If the dot command fails, `;` still
advances to the provider launch. In that case the intended fixture `PATH` and
environment are absent, so the shell may resolve a real user-installed
`codex` or `opencode`. The later synthetic-provider markers should fail the
test, but only after this unintended process has already started. The setup
files are privately created and preflighted, so this is a failure-path
hardening gap, not evidence that the normal path currently fails. Short-circuit
the launch with `&&` after sourcing so a source error cannot invoke either
provider.

## Accepted transport and preserved coverage

- `write_private_fixture_file` uses `create_new`, mode `0600`, and checks the
  regular-file type, root ownership match, single link, exact mode, and staged
  bytes. `Fixture::drop` stops sshd before removing the unique mode-`0700`
  fixture root; the new files share that root and lifetime.
- Both tests run the prepared Python readiness probe after PTY `running` and
  before provider launch. It reads the live nonce and matching lifecycle root,
  waits up to eight seconds for the actual generation file, and checks owner,
  modes, no-follow open, regular-file type, link count, size, ASCII UUID, and
  nonzero generation. The existing `screen()` ten-second deadline remains.
  The generic success/failure markers are assembled from fragments and matched
  as exact terminal lines; neither short quoted PTY command contains the READY
  marker, so command echo cannot satisfy readiness.
- The setup exports and OpenCode session argument use single-quoted values;
  the quoting helper escapes apostrophes and retains literal newlines inside
  shell quotes. Prepared scripts receive Python AST or `/bin/sh -n` checks
  before use. The real PTY receives only short path-based commands.
- The diff retains Codex launch count/arguments, config-hook preservation,
  provider identity, heartbeat/work/completion, Stop-hook, and post-exit
  passthrough assertions. OpenCode retains exact launch arguments, config and
  plugin preservation, root identity, foreign-event rejection, long-turn
  heartbeat, waiting/resume/completion/interrupt behavior, shutdown, and
  fallback launch-count/arguments. No production API or runtime file changed.
- Rechecking the fixture-root mode for each staged file is redundant but
  nonblocking (P3 at most); it adds no meaningful test or runtime risk. The
  exact-byte, owner/mode, and parser preflights directly cover the staged-file
  transport and are not called blockers.

## Validation and limits

- Parent-reported SSH integration coverage (three cases), two dependent
  provider cases, formatting, and strict Clippy passed. This review did not run
  tests, builds, or workflows, as requested.
- `git diff --check 2efb76d -- apps/cli/tests/ssh.rs` passed. The reviewed diff
  is limited to the requested test file; other dirty and untracked workspace
  paths were left untouched.
- This is a source review of test-fixture behavior. It does not independently
  establish hosted CI, native app, or production acceptance.
