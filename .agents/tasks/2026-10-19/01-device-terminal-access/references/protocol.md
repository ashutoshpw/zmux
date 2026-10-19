# Device terminal protocol v1

## Identity
Before registration, require /v1/config capabilities.stable_device_identity=true;
otherwise an older server ignores the identity fields and creates another duplicate.
Terminal transport additionally requires capabilities.terminal_relay=true. The service
advertises these only after its additive identity/relay migration is present.
Registration `POST /v1/devices`: name, platform, installation_id (UUID), device_secret
(32 random bytes, base64url without padding), public_key (X25519, 32 bytes base64url).
Same active owner/installation/proof/key updates one row; substitution or revoked rows
are denied. Registration requires modern identity fields. Never merge names or silently
rewrite historical registrations. Keep the installation secret/key in ThisDeviceOnly Keychain.

Every relay/presence call has OAuth Authorization plus X-Zmux-Device-Id and
X-Zmux-Device-Secret. Server validates owner, active device and constant-time secret hash.
Device listing includes installation_id/public_key (optional for historical rows), online
(heartbeat within 15 seconds), sharing (online and enabled), and sharing_epoch.

## Presence and encrypted relay
- POST /v1/devices/{id}/presence: {sharing: boolean, epoch: UUID}; id must be self.
  Epoch is a fresh host-sharing grant, stable while enabled, changed after disable/restart.
- POST /v1/terminal-relay/requests: {id: UUID, target_device_id: UUID, payload: base64url}.
  Target same owner, online and sharing, not self. Response {id,state,expires_at}.
- GET /v1/terminal-relay/inbox: {requests:[{id,sender_device_id,sender_public_key,
  payload,expires_at,target_epoch}]}; atomic single claim, at most one. Never redeliver.
- POST /v1/terminal-relay/requests/{id}/begin: revalidate sender's original OAuth token,
  device, target owner/sharing/current epoch before moving claimed -> executing.
  Response {id,state:"executing"}. Accepted execution may complete after later revocation.
- POST /v1/terminal-relay/requests/{id}/response: {payload}; target only, executing,
  unexpired/current epoch and valid target proof; no replacement response. Returns 204.
  Later sender revocation does not undo execution already authorized by begin.
- GET /v1/terminal-relay/requests/{id}: sender only; {id,state,expires_at,payload?}.
  Requires the original sender device and original still-valid OAuth token/session.
  States pending/claimed/executing/completed/expired; expiry is terminal/unknown once claimed.

No automatic resubmission of mutations. A repeated id is idempotent only for identical
sender/target/epoch/payload; a conflicting envelope is rejected. Default TTL 20 seconds;
requests bind target epoch. Presence every 5 seconds. Poll idle inbox at 500ms and responses
at 100ms initially, back off errors. Separate bounded authenticated relay rate limit.
At most 16 active requests per device (sender or target), 256 KiB decrypted payload,
350000 base64url characters per envelope and 384 KiB relay JSON request body. Atomic
account-scoped capacity admission, expiry pruning and bounded retained rows required.

## Native encryption
Curve25519 shared secret, HKDF-SHA256 and ChaChaPoly. Bind protocol version, request UUID,
sender and target device IDs plus direction (request/response) into key derivation/AAD.
Validate exact 32-byte peer key, envelope lengths, expiry and context before execution.
Service is the trusted account/device key directory; TLS protects proof/OAuth transport.
No plaintext terminal content, private keys, proofs or tokens in logs/durable task notes.

## Terminal owner and native interfaces
Allowlist list/frame/input/key/paste/resize/scroll for EXISTING sessions only; no general
socket forwarding, browser/file APIs or process creation. Rust owns sharing grant, exact
model namespace + surface/session binding and checks grant again at executor admission.
Disable invalidates queued work. Reads use control_frames, including background tabs.
Native host loops belong to ApplicationCoordinator; window close does not disable them.
Signout, local revoke and app shutdown stop sharing. User explicitly enables sharing.
Remote TerminalView attaches to one exact tuple; close detaches without killing host PTY.
One ordered bounded client operation queue, resize coalescing, clear unknown-outcome errors,
read-only reconnect. Listing/rendering never selects or focuses a host tab.

Exact Swift/Rust function names are coordinated by workers in their interface reports.
