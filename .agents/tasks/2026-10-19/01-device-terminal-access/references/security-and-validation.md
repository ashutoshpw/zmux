# Acceptance boundaries

## Authorization and lifecycle
- Registration proves stable installation identity, not a device name or hardware fingerprint.
- Historical rows cannot be safely merged from display names. Normal explicit revocation is
  retained; no production row cleanup is performed as part of source implementation.
- Sharing is opt-in and authorizes other registered devices on the same account to read and
  type into existing terminals (including execution through terminal input). The UI must say so.
- The account service is the trusted key directory. End-to-end native encryption hides payloads
  from database relay storage; OAuth/proof headers still require TLS to the trusted service.
- Original sender OAuth session, device revocation, target sharing epoch and expiry must pass
  immediately before execution authorization. An already-authorized operation may finish after
  later revocation; no queued old grant may enter the Rust owner after local sharing disable.
- Model namespace plus exact surface/session pair protects against restore/restart ID reuse.
- Closing a viewer detaches only. Host terminal processes remain owned by Rust and the running
  host app. Closing Cloud settings does not revoke app-wide sharing; signout/quit does.
- No process creation, generic socket route, file/browser surface, arbitrary workspace mutation
  or automatic retry of accepted/uncertain terminal input is part of this protocol.

## Evidence required
- PostgreSQL integration proves same-installation uniqueness under concurrency; unchanged
  key/proof; distinct installations sharing a name; legacy preservation; revoked denial.
- HTTP/DB owner tests prove cross-account/proof denial, bounded single claim, ID idempotency,
  expiry, sharing epoch and original sender-token revocation before begin.
- Real Rust PTYs prove list/frame on background tabs without focus mutation, ordered input,
  resize, stale scope rejection, grant revocation and unchanged general remote socket denial.
- C caller proves actual exported sharing/operation wire contract and bounds.
- Final hosted macOS workflow `39238746402` and Foundation run `39238746676` succeeded on source
  SHA `633347ff33dce806ad99dc3e8662e0be306060c2`. The corrected packaged lifecycle report passed
  the two-origin CloudClient/CryptoKit, CloudTerminalHost, NativeBridge and Rust PTY path,
  including registration identity, terminal I/O, resize, sharing denial, cleanup races and
  account-switch recovery. The source-pinned universal artifact `12181162700` was independently
  verified with SHA-256
  `2bf2fe16965952bebe32106bee1b444e9e4b355b816cc132fd640e8ce444ce4e`. Strict codesign passed;
  signatures are ad-hoc and notarization is incomplete.
- Hosted runner execution establishes packaged-path coverage, not physical two-Mac acceptance.
  Two physical Macs and an authenticated public-route journey remain separate unverified gates;
  Linux/runtime/HTTP evidence alone is not a MacBook dogfood receipt.
- Production migration/deployment is a separate final gate after concrete validated artifacts;
  no production DB, serving service, tunnel, or user's running cmux is changed during development.
