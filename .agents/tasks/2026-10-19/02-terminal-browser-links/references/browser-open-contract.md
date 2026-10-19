# Browser-open contract

This records the accepted contract for implementation across the Zmux core, shell/CLI, and
Swift/AppKit. It defines behavior and ownership; it is not a test result or a claim that the
implementation is complete.

## Scope and exclusions

- Keep the implementation original to Zmux. Do not copy or translate cmux code, prose, tests, or
  assets.
- Reuse the existing `zmux open` flow and deliberate terminal Command-clicks. Do not add a new
  CLI subcommand.
- Unsupported commands and integrations remain unsupported; do not claim general compatibility
  from fixture coverage.
- Use fixture launchers only. Do not perform a real GitHub login or mutate an account.
- Do not change the global OS browser default, user startup files, serving database/service, public
  route, tunnel, or the user's running app.

## Explicit OSC open requests

An automatic open request is authorized only by this explicit OSC event:

```text
ESC ] 777;zmux-open;v1;<32-lowercase-hex per-session nonce>;<base64url-without-padding URL> BEL
```

`ESC` and `BEL` are the OSC introducer and terminator. The nonce is the current fresh per-session
value, represented by exactly 32 lowercase hexadecimal characters. Create or refresh this
in-memory value as part of session/launch work. Do not make the UI or core action executor wait on
entropy, filesystem, or process work. The session terminal worker validates the nonce, strict
unpadded base64url encoding, UTF-8, and the decoded 8192-byte URL bound. The nonce filters ambient
terminal output; it is not a security boundary against code running as the same user.

The terminal worker does bounded OSC parsing on the session worker. It does not apply the desktop's
full URL policy: dependency layering does not allow the session runtime to call workspace URL
policy. The desktop runtime performs canonical HTTP(S) URL validation and rejects SSH-originated
loopback URLs unless relevant port forwarding is proven. No browser effect occurs before that
desktop validation. Printing or pasting a URL is not an open request.

Use a private, ephemeral, bounded in-memory queue that the browser owner drains; revalidate the
live owner when draining so closed, replaced, or stale origins are rejected. Do not make UI or core
action executors wait for filesystem or process work. Do not add a sequence field: PTY ordering is
preserved and requests are not retried. Queue entries, nonces, and opener-request state must never
be serialized into snapshots or cloud data, checkpointed, or logged.

The OSC event carries only the URL and nonce, not a pane ID. At desktop admission, the core maps
the live session to its current pane and binds the request to that pane; it revalidates the live
mapping before routing the browser action. This admission-time mapping does not prove which pane
owned the PTY when it emitted the OSC bytes. Keep configured opener overrides composable and
preserve the existing user-selected behavior.

Bounds are explicit: the open-request queue holds at most 8 requests, each with at most 8 KiB of
decoded URL data. The published viewport contains at most 512 link spans and 64 KiB of retained
link URL data. The default `vt100` backend retains at most 64 OSC 8 targets and 256 KiB of target
data in total, with an 8 KiB per-target bound.

## Terminal links and browser action

The core serializes visible terminal hyperlink spans with their content generation. Spans include
correct OSC 8 links and visible plain HTTP(S) links. URL text and hyperlink metadata may be part of
ordinary terminal content; snapshots must not contain opener intent or the OSC nonce.

The Swift/AppKit view opens a span only on Command-click. Its request requires
`identity_namespace`, `surface_id`, `pane_id`, `session_id`, `viewport_generation`, `row`,
`column`, and `expected_url` from the displayed owner and hit cell. The core is authoritative: it
revalidates the exact namespace/surface/session owner, verifies that the session still maps to the
required `pane_id`, then revalidates the cell, generation, and URL against current terminal
content before invoking the shared browser action. A missing pane ID or changed, stale, or
mismatched span is rejected.

Route OSC requests and accepted Command-clicks through the same browser action. Ordinary click,
drag, paste, selection, mouse tracking, and PTY byte ordering retain their current terminal
behavior. A cloud viewer with a nil open callback retains its current behavior; do not synthesize a
browser action or fallback for it.

The default development artifact uses the `vt100` backend and supports plain HTTP(S) spans, OSC 8,
and the explicit opener. Correct OSC 8 parsing with only the minimal needed patch to the
MIT-licensed `vt100` dependency. Retain the dependency's source and upstream license notice with
the patch; see [`vendor/vt100/UPSTREAM.md`](../../../../../vendor/vt100/UPSTREAM.md). Do not copy
cmux's terminal parsing or hyperlink implementation.

Nested tmux/screen automatic opener requests are unsupported because those layers may swallow OSC
on `/dev/tty`. The optional Ghostty backend supports plain HTTP(S) spans only; OSC 8 and automatic
opener requests are unsupported on that backend. Integration-disabled shells, explicit SSH
commands, and SSH `RemoteCommand` are also unsupported.

## Exclusions and verification boundary

Do not implement generic shell command execution, arbitrary URL schemes, an unbounded browser API,
or unsupported opener integrations under this contract. Reuse the existing `zmux open` flow;
unsupported commands and integrations remain excluded. The helper has no program-acknowledgment
wire: emitting the OSC request does not prove a browser effect. An end-to-end test must assert the
actual browser surface and URL in the intended pane. Fixture, hosted, and physical-device evidence
must be reported separately. No green test, review, or artifact status is asserted by this
contract.
