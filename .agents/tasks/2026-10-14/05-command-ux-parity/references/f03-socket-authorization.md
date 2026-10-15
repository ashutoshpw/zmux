# F03a local socket authorization

Date: 2026-10-15. Parent baseline: `c572484`. This bounded foundation does not
close F03, managed preferences, remote authorization or full artifact acceptance.

## Implemented

- Same-user peer enforcement and private socket paths remain mandatory in all modes.
- Native startup configuration supports full/read-only/disabled socket access and
  optional required passwords; malformed modes and missing required credentials fail.
- CLI credential precedence: leading flag, environment, private saved file. Nofollow,
  regular-file/owner/permissions/size/UTF-8 checks occur on the opened descriptor.
- Local wire protocol is v2. The new server accepts legacy v1 requests and replies in
  v1 under the same authorization; new CLI/old-app skew gives explicit upgrade guidance
  without mutation retries. C ABI v1 and model v3 remain unchanged.
  Opaque credentials redact Debug; bounded fixed-size comparison uses `subtle` 2.6.1.
- Generation admissions are checked at the workspace executor, holding policy access
  during the mutation. Live policy replacement revokes queued stale requests. A completed
  mutation can still return success; a timeout is never converted into definite denial.
- Read-only classification covers legacy, generic-action and targeted wire forms.
  Remote origins deny by default. This does not expose a remote socket tunnel.
- Capabilities reports effective authentication/access; child PTYs remove inherited
  socket passwords. ZMUX_SOCKET_PATH takes precedence over legacy ZMUX_SOCKET.

See [local control](../../../../../docs/local-control.md) for actual configuration.

## Verification

`scripts/check` passes: 57 Rust tests, formatting, Clippy with warnings denied,
dependency inventory and C ABI harness. Public CLI integration uses a real Desktop and
Unix socket for credentials/precedence, access changes, denied mutation/no state change,
private-file/symlink rejection and capabilities. The executor test injects an admission
revoked before execution and verifies unchanged workspace revision/count. Remote-origin
rejection is verified at its policy owner, not claimed as a deployed relay journey.

The PTY password inheritance regression was observed red without scrubbing (the synthetic
credential appeared in the child environment), then green with scrubbing restored.
`subtle` BSD-3-Clause notice is retained; inventory has 62 packages/43 notice texts and
three previously documented non-macOS missing-text follow-ups. `git diff --check` flags
only a verbatim trailing space in that upstream license text, not source formatting.

The first independent review found macOS accepted-stream nonblocking inheritance, an
application-environment override of password scrubbing, and an undeclared schema upgrade.
Repairs explicitly set blocking/timeouts on accepted streams, scrub after overrides, and
version the wire contract. Nonblocking delayed-input and environment-override regressions
were observed red before repair and green afterward. Legacy v1 mutation/password-denial
and CLI protocol-skew error behavior are checked at their actual public boundaries.

Hosted macOS run `38705037622` at `d1393cb` supplied the framing failure. F01a run
`38706071891` at `c572484` passed native compile/packaged smoke, but does not include
these F03 changes or establish final artifact acceptance.

## Remaining gates

- Native per-mode credential/access/reload journeys and native settings/Keychain integration.
- Forced managed preferences, cross-native-entrypoint enforcement and resource cancellation.
- Object-scoped relay authorization and remote command/content-effect analysis in R02.
- Per-mode source configuration/reload UI and deployed/native policy lifecycle acceptance.
- Full command and product parity; no new user-facing artifact is eligible.

## Review

Initial `/root/control_authorization_review`: **fix-first**, three issues repaired above.
Requested `gpt-5.6-sol / high`; realized model/effort unobservable. Fresh `/root/control_authorization_repair_review`: **ship**, no material findings.
Requested `gpt-5.6-sol / high`; realized model/effort unobservable.
The integrated workspace including separate R01 descriptor work also passes (65 tests).

Hosted follow-up: macOS Actions run `38707343689` at `a1c621f` passed Rust/C ABI
checks (including the framing regression), universal native compilation and packaged
default-mode smoke. This does not verify native password-mode configuration journeys,
Intel runtime, physical input, deployed services or whole parity. No archive is shared
as the requested artifact.

## Client response deadline repair

One monotonic deadline now covers Unix socket connect, partial request writes, partial
response reads, and response acceptance after bounded JSON decoding/validation. The
existing pinned socket2 dependency supplies bounded connect; no timeout worker or command
replay is introduced. CPU decoding cannot be preempted, but an expired decoded response
cannot be accepted as success. Output rendering follows response acceptance.

Real Unix-wire slow header/body/staged cases failed before repair and pass after. A
940 KB response exposed decode finishing after expiry; manual wire evidence and a
negative control of the genuine production acceptance owner verify the fix. CI uses a
deterministic expired Instant rather than assuming a particular CPU parsing speed.

Parent isolated full verification and fresh review precede the repair commit. Hosted
Darwin verification remains required. Browser receipt fixtures retain an exact runtime
error check, with a 4-second positive budget and unchanged 2-second stalled-receipt
negative control; both old-reserve red/current-reserve green runs were reproduced.
