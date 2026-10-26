# R01a SSH connection descriptor foundation

Date: 2026-10-15. Parent baseline: `a1c621f`. This is the descriptor portion of R01a;
managed workspace creation, authentication sharing and transfer behavior are not implemented.

## Implemented boundary

New original MIT crate `zmux-remote-runtime` provides validated destinations/config aliases,
IPv4/IPv6, optional user/port/identity, ordered jump hosts, explicit host-key policy,
agent forwarding on/off, typed TCP forwarding and a bounded allowlist of extra OpenSSH
options. One endpoint renders route/auth/host-key flags for terminal and future file
operations. Process specifications use program plus argv; no local shell is introduced.

The default host-key policy preserves ssh_config. Explicit host-key overrides and -A/-a
are rendered only when selected. Conflicting agent policies, repeated options, malformed
or option-shaped hosts, control characters and oversized descriptors are rejected.
Remote command argv still has OpenSSH's remote-shell semantics; it is not a literal remote
execution protocol. No new public `ssh` command is advertised from this library alone.

## Evidence

The implementer ran eight public API tests, Clippy and rustdoc through a temporary manifest.
The parent inspected the complete source/test candidate and independently ran the eight
tests before integration, then `scripts/check` with the crate in the actual workspace:
65 Rust tests, formatting, Clippy with warnings denied, dependency notices and C ABI pass.
No additional external dependency is introduced by this crate (`thiserror` was existing).
OpenSSH `-G -F /dev/null` accepted representative port, multi-hop IPv6 jump, host-key and
agent arguments. This was configuration parsing only: no connection/auth/upload occurred.

## Required next integration and remaining gates

- Add a validated wire configuration, CLI parser and remote workspace/surface creation.
- Bind connection/transfer identity to the original surface generation; retain remote
  descriptors in close/reopen/persistence, with no local-shell fallback.
- Implement interactive authentication plus private shared connection ownership; file
  operations must use the authenticated route even with ssh_config changes or disconnect.
- Add bounded file/image preparation/upload, progress/cancel, native drop/clipboard/paste
  and correctly quoted remote-path insertion to the originating live terminal.
- Resolve raw ProxyCommand/arbitrary -o/custom known-host/Unix forwarding/IPv6-zone and
  remote-command PTY behavior against full frozen contracts; current subset is explicit.
- Real two-host auth/host-key/proxy/Unicode/disconnect/cancel/close/switch journeys,
  Mac native tests and full V01 signoff remain open. This does not close any remote family.

## Review

Implementer `/root/ssh_descriptor`, requested `gpt-5.6-sol / high`, completed its bounded
three-file task. Realized model/effort unobservable. Fresh reviewer `/root/ssh_descriptor_review`, requested `gpt-5.6-sol / high`: **ship**,
no material findings. Realized model/effort unobservable. Reviewer independently passed
eight tests and OpenSSH configuration parsing including an option-shaped identity path.

Integration must adapt SSH transport flags if using scp/sftp (notably ssh -p versus
scp/sftp -P). The library documentation states this requirement; preferably transfer
through the owned authenticated SSH connection. Command payload content is capped at
64 KiB; OpenSSH may add at most 127 joining spaces for 128 argv entries.
