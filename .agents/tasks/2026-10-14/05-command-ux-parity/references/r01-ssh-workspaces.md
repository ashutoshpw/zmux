# R01b SSH workspace integration

Date: 2026-10-15. Original MIT implementation. Reference behavior is documented in
[the SSH comparison](ssh-transfer-contract.md); no upstream code or tests are copied.

## Implemented subset

- `zmux ssh` creates a typed remote workspace through the authorized shared action executor.
- Wire configuration is validated before allocation. SSH runs as argv on a launch worker;
  remote commands request a real PTY. Connection failure never runs the command locally.
- Config aliases, ports, identities (including spaces), agent forwarding and bounded SSH
  options are retained on the exact surface. Relative CLI paths resolve against CLI cwd.
- Creation reports exact created workspace/pane/surface identities even with `--no-focus`.
- New tabs, splits and reopen retain the remote endpoint and clear the explicit startup
  command. Unknown remote cwd is cleared; it cannot become a local launch directory.
- Local control remains v2 with valid v1 server compatibility. Native ABI v1/model v3 stay.

## Verification

`scripts/check` passes: 73 Rust tests, fmt, Clippy with warnings denied, dependency notice
inventory and C ABI. Test ownership is the real CLI/socket/PTY/OpenSSH boundary, with a
small core invariant test for atomic validation and remote restoration.

The isolated loopback sshd journey generates private temporary keys and configuration,
authenticates with an identity path containing spaces, verifies PTY allocation and
input/output bytes, preserves no-focus selection, closes/reopens without startup replay,
and observes exit 255 after server loss without local fallback. Strengthening the real
journey to require a remote PTY observed exit 77 before adding `-tt`; the same test passes
with the fix. Linux CI installs openssh-server; macOS uses its system sshd.

Fresh read-only integration review returned `ship` with no material findings.
Exact-SHA hosted runs are pending. This does not establish
password authentication, host-key failure, jump-host or two-host/native transfer behavior.

## Remaining acceptance

Private authenticated connection sharing, bounded file/image upload, progress/cancel,
native drop/paste and origin-generation completion are next. Durable remote sessions,
reconnect, mosh, relay policy, remote cwd restore and full SSH grammar remain open.
R01 and V01 remain incomplete. No user-facing artifact is shared for this subset.
