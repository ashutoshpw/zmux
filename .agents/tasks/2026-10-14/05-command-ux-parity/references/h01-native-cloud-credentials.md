# H01 native credential ownership and cancellation (2026-10-15)

The existing native client was actor-isolated but reentrant across HTTP awaits.
`complete` saved its token before awaiting account validation, `signOut` erased
after awaiting revocation, and every HTTP401 erased the current Keychain item,
including unauthenticated exchanges. A cancelled login could persist a token;
an older sign-out or unauthorized response could erase a later login. Each native
window also constructed a separate client for the same Keychain origin.

Cloud settings now shares one configured endpoint/client actor across its native
windows. This is the production credential owner; independently constructed client
instances are not claimed to coordinate mutations. Each operation captures an
intent generation and exact credential. A login validates its candidate account
using that credential before its single synchronous Keychain commit. Cancellation
and generation are checked after HTTP suspensions and immediately before save.
Account identity and device reads retain one scope, preventing mixed accounts.

Sign-out invalidates older operations and clears its local credential before
awaiting revocation of the captured token. A later login cannot be erased by that
older response. HTTP401 clears only the current generation's exact committed
credential; unauthenticated exchanges and superseded requests cannot clear it.
Supersession uses cancellation rather than publishing an obsolete sign-in failure.
The settings UI ignores cancellation without marking a newer account unavailable.

The dedicated native fixture compiles the actual CloudClient, uses a real bounded
loopback HTTP server and actual Keychain calls, and exercises cancelled account
validation, superseded token exchange, stale401/revocation, unauthenticated401,
current401 invalidation, and local sign-out before a delayed remote response.
Fresh client readers verify persistence after each settled mutation. The server
keeps synthetic credentials only in memory and never logs headers or bodies.

Its origin is a freshly bound127.0.0.1 port. A separate absence preflight refuses
any preexisting Keychain item before ownership. Normal/error cleanup is verified,
and the external runner repeats exact-origin cleanup after a crash or timeout.
Fixture Keychain probes use a noninteractive Local Authentication context.
Neither the configured cloud endpoint nor any existing user credential is used.
There is no injected URLSession, credential-store substitute or production test
hook. Before Rust compilation, the early native workflow compiles the same
public fixture against pre-fix client `c25c4c631bb4917ee496f9caed39b42e843bb78d`,
then the current owner. Baseline acceptance requires the specific cancelled-login
credential-replacement assertion, exit1 and verified external cleanup; compile,
setup, timeout or unrelated failures do not count as the red. The current run
must pass every case and external cleanup. Probe output and process lifetime use
the existing bounded process owner. The optional `--client-source` tool argument
supports a single explicitly selected source for isolated diagnostic use.

Portable evidence: fixture Python syntax, shell syntax, scoped whitespace, and
actual loopback HTTP control/response roundtrips pass. These are not native client
or Keychain acceptance. Strict Swift compilation, the pre-fix red and repaired
native fixture execution are required hosted gates; their execution is recorded
below. Cancellation fences local state;
it does not claim rollback of a remotely minted token or completed revocation.
Public sign-in, deployment and full H01/cloud parity remain separate gates.

Hosted native acceptance: run 38818645717 at
`352ba3bf46065dd67d3269b1761644d3dc174a71` executed this exact fixture on macOS.
The pre-fix client SHA256 `bc91b45d9fdcd27c5cfbdf0fda1e575ed74808a649acf5050afd24d1a4cd93d2`
failed with the required cancelled-sign-in credential assertion; the runner
verified external Keychain cleanup. The repaired client SHA256
`f8c24cc98383d5cea3ff84bc1806c491c9a9cb699178292aaca617ad63473937`
passed every listed case and both in-process and external cleanup. Both native
Swift slices, the external Save panel driver, Rust/C ABI and the universal native
build also passed. The later packaged app smoke failed at the existing compound
sidebar keyboard/AX/persistence assertion, before browser keyboard smoke. This
establishes focused credential ownership/cancellation acceptance, not a complete
packaged app pass or public cloud authentication/deployment acceptance.
