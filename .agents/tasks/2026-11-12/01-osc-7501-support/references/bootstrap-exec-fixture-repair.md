# Managed SSH bootstrap exec fixture repair

## Observed failure

At source `82df3353d2bea3a9b2f6919ba67b48b2df5fb05a`, Foundation macOS job
`131692796323` and universal macOS job `131693921355` failed in
`managed_ssh_bootstrap_fits_a_single_exec_argument` before reaching its explicit
64 KiB assertion. The captured Foundation log reports `ENOENT` from
`Command::new("/bin/true").arg(&command).status()`; the universal log reports
the same spawn failure. This evidence establishes the spawn error only, not a
more specific cause in the macOS filesystem. The remaining shell integration
tests passed (12 passed, 1 failed, 2 ignored), and Linux passed.

## Repair

The existing test now starts `/bin/sh -c ':'`, sets the shell's command name
to `zmux-bootstrap-argument`, and passes the full, unmodified
`shell_integration::remote_command(...)` as the next argument. With the POSIX
`sh -c` argument layout this value is `$1`; the fixed command string is only
`:`, so the remote command is never evaluated. The OS still receives the
complete remote command in the `exec` argument vector. The test retains both
the successful-exit assertion and the `command.len() <= 64 * 1024` transport
budget assertion. `/bin/sh` is already used by this test target's fixtures.

## Test-authoring gate

1. **Observable contract:** The real managed SSH bootstrap must fit as one
   argument to a kernel `exec` call and remain within the 64 KiB transport
   budget.
2. **Credible regression:** Growth or encoding changes can make the real
   command too large for a platform's argument limit, causing process creation
   to fail before an SSH session can use it.
3. **Coverage gap:** The existing SSH/session tests exercise managed shell
   behavior but do not independently prove that this exact complete command
   can cross the local kernel `exec` boundary as one argument. This test owns
   that contract; no duplicate test or synthetic replacement payload was added.
4. **Production seam:** None. The test calls the real `remote_command` producer
   and the operating system directly.

## Local validation

- `cargo test --locked -p zmux-session-runtime --test shell_integration managed_ssh_bootstrap_fits_a_single_exec_argument` — passed (1 passed).
- `cargo test --locked -p zmux-session-runtime --test shell_integration` — passed (13 passed, 0 failed, 2 ignored).
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --locked -p zmux-session-runtime --test shell_integration -- -D warnings` — passed.
- `git diff --check -- crates/session-runtime/tests/shell_integration.rs` — passed; the new note produced no whitespace diagnostics under `git diff --no-index --check /dev/null <note-path>`.

No production code, new suite, seam, timeout, skip, fallback, or negative
control was added. The negative control was unnecessary for this fixture repair:
the passing test invokes the kernel with the actual generated command, while
the unchanged length assertion independently enforces the transport budget.

## Parent acceptance and kernel control

Parent accepted the three-argument fixture-only addition: the complete generated
command is still one unmodified OS argument, and the shell evaluates only `:`.
A separate Linux probe with the same inert-shell argument layout and a
146,116-byte positional argument failed with E2BIG as expected, confirming that
the replacement retains the kernel argument-limit boundary. This probe is not
a new test or a claim of post-fix macOS execution.

All 229 retained-linkage source inputs were compared with the working tree;
only this test file changed, so engine library bytes need no rebuild. Refresh
the retained linkage receipt at the next source because the file is captured.
