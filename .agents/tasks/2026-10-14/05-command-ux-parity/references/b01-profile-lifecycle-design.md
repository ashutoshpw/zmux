# B01 clear/delete lifecycle design (read-only investigation)

No destructive command or release of constructed-store ownership is implemented.
The acquisition slice still requires actual macOS validation. This design proposes
a replacement for restart-only ownership for a sole local owner without treating wrapper
closure/deinit as physical WebKit retirement evidence.

## Public evidence and its limit

Apple's public identifier-removal API documents: release WKWebView instances using
the store before calling, then its completion reports removal or an error.
https://developer.apple.com/documentation/webkit/wkwebsitedatastore/remove(foridentifier:completionhandler:)

The website-data clearing API completes after removing specified record types but
has no error result or physical store-retirement promise. Listing identifiers is
an inventory of on-disk stores, not proof that no native owner remains.
https://developer.apple.com/documentation/webkit/wkwebsitedatastore/removedata(oftypes:modifiedsince:completionhandler:)
https://developer.apple.com/documentation/webkit/wkwebsitedatastore/fetchalldatastoreidentifiers(_:)

Read-only upstream reference, not imported implementation: WebKit commit
30281d59519dd0df7cf7b0326d31b726ddb04289 returns busy for active pages or a retained
network process, otherwise removes the identifier's network session and disk data
before completing. This is not installed-OS proof; busy/success native fixtures
must establish supported behavior. The identifier callback is the namespace-removal
receipt; V3's retained cookie shows that it alone does not establish empty website
data. Store-specific clearing has a separate completion, and native data observations
remain required. Weak object disappearance, stopLoading, canceled navigation,
empty host dictionaries, cache eviction and UUID listing cannot replace it.
https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/UIProcess/WebsiteData/Cocoa/WebsiteDataStoreCocoa.mm#L326

## Why one lifetime lock cannot safely be upgraded

Dropping this process's shared lock to try an exclusive lock exposes a gap while
framework references may still exist. Another destructive process could observe
exclusive ownership during that gap. Lock conversion must not be assumed atomic
on supported operating systems. Keep the existing process-lifetime behavior until
an admission gate is implemented everywhere.

Use two stable per-UUID files in the same validated OS Library lease root:
construction admission and physical lifetime. Never unlink either. Session export
must reserve both generated filename namespaces before the gate is introduced.
The service retains anchored root/file identities and uses the existing nofollow,
private-owner, zero-byte, single-link and before/after identity checks.

All constructors, including a new WKWebView that reuses a cached store, must join
the gate protocol. A shared construction permit precedes lifetime ownership and
current-catalog identity validation. Hold it through the actual MainActor native
construction and correlated receipt. Unknown construction acknowledgment retains
permit and lifetime capacity. A canceled, never-constructed attempt may release
its exact permits only after original-owner status resolves. Once construction
is known, the admission permit may release while lifetime ownership remains.

## Sole-writer sequence and restoration

Every lock attempt is nonblocking on a bounded worker. The only acquisition order
is admission then lifetime; never wait on admission while holding a new lifetime
request. No UI/core executor waits. One epoch/operation token identifies the whole
sequence, including native callback and catalog commit.

1. Reserve local destructive intent and bounded callback/receipt capacity. All
   create, reopen, restore, popup and automation-originated construction paths for
   that UUID consult the same owner. Do not close the user's live tabs implicitly.
2. Deny mounted or hidden profile pages, pending constructors and active/retiring
   download or conversion owners before mutation. A stopped browser wrapper is
   insufficient: actual WKWebViews and physical download/conversion jobs count.
3. Acquire admission exclusive. Contention returns busy without dropping lifetime
   ownership. Revalidate current-catalog identity and native busy state under it.
4. Evict only the idle local store cache, release application-owned WKWebViews,
   and observe actual WKWebView weak registrations plus existing physical job
   receipts. Retain the exact inactive store for store-specific data clearing.
   Keep the shared lifetime lease while this prerequisite is pending.
5. Under retained admission exclusive, transition this process's exact known
   lifetime owners to one lifetime-exclusive reservation. Existing foreign shared
   owners cause busy. Restore all required local shared ownership before releasing
   admission; never expose an unlocked gap. Other processes' cached stores remain
   valid busy owners; do not steal their tokens or infer retirement from age.
6. With writer authority retained and WKWebViews released, invoke the exact inactive
   store's all-data clear once and retain its distinct completion receipt. Release
   store refs, then call public identifier removal once and retain its distinct
   namespace-removal receipt. Residual framework/network ownership may still return
   busy. No background retry loop or synthetic completion.
7. On known busy/error, restore local lifetime shared authority while admission is
   still exclusive, then unlock admission and report the exact failure. If shared
   restoration fails, retain writer authority and capacity; reopening stays denied.
   A namespace-removal error after data-clear completion is a partial mutation,
   never unchanged/busy rollback. Retain and report both phase outcomes.
8. Timeout/cancel after admission or callback dispatch is unknown, never success.
   Retain writer, lifetime-exclusive authority and physical callback capacity until
   that original callback/status resolves. No replay and no same-UUID reopen.
9. Known data-clear completion plus namespace-removal success permit the correlated
   durable metadata sequence below. Only its
   exact known completion releases writer authority. Late callbacks cannot affect a
   successor; no successor is admitted while the original outcome remains unknown.

Clear can use this data-clear/identifier-removal owner while retaining the immutable catalog
UUID, so the next permitted open creates empty storage. Delete removes catalog
membership only after public removal succeeds. Never substitute private/default
storage, delete unrelated history or silently rewrite old session/closed bindings.

Retain distinct correlated durable phases for data-clear dispatch/completion,
namespace-removal dispatch/completion/error and selected-catalog commit. Completion
of an earlier phase does not imply later phases ran. Unknown callbacks retain
authority and callback capacity; restart reconciliation and receipt retention must
be chosen before production admission. A known partial failure must disclose the
already-cleared data and must never replay clearing while retrying catalog work.
Generated cleanup UUID callbacks in the diagnostic prove only their reported
namespace outcomes; they do not independently prove cookie clearing, as V3 showed.

## Delete scope and stale copies

Delete applies to the selected catalog and the actual datastore removal receipt.
The frozen B01 contract requires old closed/session references in that installation
to fail when its catalog no longer contains the UUID. It does not promise global
revocation across separately copied catalogs. No global retired-UUID state or
cross-catalog resurrection-prevention mechanism is part of this slice.

A copied catalog still containing that UUID can later recreate an empty WebKit
store after this known successful operation releases the gate. This is explicit
stale-copy behavior, not cross-process exclusion failure. During a pending/unknown
removal, the common writer gate must still prohibit every catalog from reopening
that UUID. Concurrent constructors are serialized even when catalogs differ.

If current-catalog commit fails after native success, retain the original operation's
known removal receipt and report the partial outcome; do not claim rollback restored
website data. Recovery retries that correlated catalog step without replaying removal.
The exact receipt retention/restart policy must be chosen before production admission.
No new global identity registry is required by this contract.

## Required discriminators before implementation admission

- Actual independent processes using copied catalogs and the common root prove
  constructor/writer serialization, fixed lock order, busy restoration and crash
  release. Pause between dropping own shared and acquiring exclusive: neither a
  second writer nor a new constructor may cross the retained admission gate.
- Mounted/hidden pages, pending construction, active downloads and conversions
  reject before WebKit removal, unchanged catalog/data/host identity.
- Actual macOS public removal returns busy with a retained WKWebView, then succeeds
  after real native owner release under writer authority. No assertions based only
  on a wrapper or weak-store count.
- Callback suppression/late completion/cancellation retain authority and capacity;
  old epoch/token cannot unlock or mutate a successor. Native and CLI status query
  the original operation. Busy restoration is externally verified before reopen.
- Clear retains UUID/name and reopens empty cookies/localStorage while a separate
  profile remains intact. Delete makes references absent from the selected catalog explicitly unavailable;
  a separate stale catalog may later create empty storage. Catalog failure after removal reports
  partial success without data-loss concealment.

All of the above remains source/design evidence. No clear/delete parity or actual
OS retirement proof is established by this document.

## Finite public API feasibility probe

The original MIT standalone probe under `tools/native-profile-lifecycle/` uses a
unique generated bundle identifier and two generated UUIDs. It mounts A/B against
an owned loopback server and seeds distinct cookies/localStorage. Public removal
of mounted A must callback an error while data remains intact. After releasing
application A references and observing the actual WKWebView release prerequisite,
one removal call records its public completion/error. Weak-store state is only
diagnostic. Success then requires the same A UUID reopen empty and B stay intact,
followed by public cleanup of only these generated stores.

The diagnostic-only workflow targets `ci/browser-profile-lifecycle-probe`. The
runner bounds build/sign/app phases, retains exact source hashes and app archive,
records actual owned process PID and callbacks, and stops/reaps its own group.
The app has an external 45-second bound. It saves a pending marker before each
removal so a missing callback is unknown, never treated as a cleanup retry. Native
failure retains the exact error and generated namespace; it never deletes WebKit
filesystem paths. No user's bundle, stores or running app are touched.

Run [38837864579](https://github.com/ashutoshpw/zmux/actions/runs/38837864579)
executed exact commit `a32530fd36afafd21edfd99077331b502b7d99b4` on macOS 15.7.9
arm64. Build/sign passed. The owned native process exited 1 after 4.287 seconds,
without a watchdog timeout. Mounted A removal returned `WKWebSiteDataStore` code
1, `Data store is in use`, and A data remained intact. After the A view/store weak
references disappeared, the public removal callback returned no error. B remained
intact, but the reopened-A emptiness assertion failed. Both removal callbacks
arrived; this is a known reported-success callback followed by an unresolved data
observation, not a pending/unknown removal outcome.

The first report retained only a combined emptiness boolean. It cannot distinguish
localStorage retention, cookie retention or an observation-format mismatch. A
diagnostic-only follow-up retains bounded actual A/B observation strings plus A's
localStorage/cookie values, preserving the same assertions, fresh generated IDs
and one call per removal phase. No failed removal is retried. The native evidence
does not yet admit production clear/delete, and remains separate from the
production lease acquisition check.

Run [38838619443](https://github.com/ashutoshpw/zmux/actions/runs/38838619443)
at `4f6a6028c13389ac8ea0163dc40ced2b8064e94c` compiled and repeated the
mounted-busy/released-callback behavior. The same-process reopened A observation
was `{"empty":true,"cookie":false}`; actual localStorage was null but the cookie
was still `probe=A`. B retained its independent B data. The native process exited
1 in 3.494 seconds without timeout. Exact Git/source/archive/plist/executable and
owned PID/SHA/nonce/UUID correlation passed independent inspection. This is an
installed-OS cookie-retention blocker, not a formatting ambiguity or unknown
callback. Fresh-process durable behavior has not been tested.

## Store-specific public data-removal candidate

The next diagnostic releases A's WKWebView while retaining the exact inactive A
store, then calls
[`removeData(ofTypes:modifiedSince:completionHandler:)`](https://developer.apple.com/documentation/webkit/wkwebsitedatastore/removedata(oftypes:modifiedsince:completionhandler:))
once with `allWebsiteDataTypes()` and `.distantPast`. The installed set must include
`WKWebsiteDataTypeCookies`. A pending marker precedes dispatch; the callback has
no error parameter and establishes only completion, never physical retirement.
The probe then releases the store reference, calls UUID removal once, and retains
the strict same-UUID reopened-cookie/localStorage emptiness and B-preservation
assertions. Unknown callback outcomes still stop without replay or filesystem
cleanup. This isolated namespace does not prove the production writer protocol.

Apple also exposes store-specific
[`httpCookieStore`](https://developer.apple.com/documentation/webkit/wkwebsitedatastore/httpcookiestore),
[`getAllCookies(_:)`](https://developer.apple.com/documentation/webkit/wkhttpcookiestore/getallcookies(_:))
and [`delete(_:completionHandler:)`](https://developer.apple.com/documentation/webkit/wkhttpcookiestore/delete(_:completionhandler:))
on macOS 10.13+. They provide a public per-store cookie path, but this experiment
does not silently add a per-cookie fallback. It uses neither global cookie storage
nor WebKit filesystem paths/private APIs. Live official documentation responses
were retained under `/tmp/zmux-public-cookie-removal-docs-20261016` with hashes.

Before any production clear/delete, exclusive admission and lifetime authority
must span inactive-store data completion, store release, UUID callback and durable
catalog handling. Active pages, hidden views, pending constructors and retained
download/conversion owners still reject before mutation. Data-removal completion
does not replace those requirements or permit timeout-based authority release.

Run [38839388593](https://github.com/ashutoshpw/zmux/actions/runs/38839388593)
at `ade61dafe3d9aa6d868125f2fef4fd76d0f6c41e` passed the same-process
candidate on macOS: A's store-specific data completion arrived, UUID removal
returned no error, reopened localStorage was null and the cookie string was empty.
B retained its independent data. All four UUID callbacks arrived, including the
expected mounted-A busy error and generated-store cleanup. Build/sign/native
exited 0 without timeout. Exact source/archive/app/identity evidence was reviewed.
This accepts only the isolated same-process sequence, not production deletion.

## Fresh-process persistence gate

The success-only extension keeps the same signed archived app, bundle, UUIDs,
loopback origin and nonce across two separate native processes. The preparer runs
the same mounted-busy/data-clear/UUID-remove/same-process-empty assertions, releases
both views and preserves B, then writes `prepared_for_fresh_process` and exits.
Its receipt is preparation evidence. It does not delete B before the reader.

Only after the existing owned process runner returns and its retained child
handle reports reaped exit 0 may the reader launch. The runner validates the
preparation identity and hashes its immutable receipt; archive/executable hashes
must remain unchanged. The reader loads that exact preparation and records its
preparer PID, then opens A/B without seeding. It must observe empty A cookie and
localStorage and unchanged B in the new process before publicly removing only
those generated UUIDs. The reader must have a different actual PID and the same
SHA/bundle/nonce/UUIDs. Both processes have independent 45-second watchdogs and
separate reports/logs; a failed or unknown preparer never launches the reader.

The runner retains `same_process_acceptance` and `fresh_process_acceptance`
separately. Pending markers cover all-data completion and UUID callbacks. Unknown
outcomes never cause a retry, overlapping app or filesystem cleanup. Production
writer/lease coordination and destructive command exposure remain unavailable.

Run [38839907151](https://github.com/ashutoshpw/zmux/actions/runs/38839907151)
at `6e71df371c7fa9d4bcb911a67b2e44ca09353b6b` passed preparation and the
owned exit barrier (PID 8020, exit 0), then failed in reader PID 8045 without a
timeout. A was empty in the fresh process. B retained localStorage B but its
`probe=B` cookie check was false. The reader performed no removal/cleanup before
failing; no callback was pending. Exact retained sources/archive/plist/executable,
both identities, preparation hash and phase linkage were independently verified.
This blocks fresh-process isolation acceptance; it does not yet distinguish
cross-profile clearing from cookie persistence under the fixture's `exit(0)`.

The next fresh-ID control is explicitly `scenario=baseline`. Its preparer seeds
A/B, records actual JavaScript cookie/localStorage strings plus public per-store
`getAllCookies` results (bounded name/value/domain/path/session/expiry fields), then
uses the same view release and owned `exit(0)`/reopen sequence. It performs no
data clearing or UUID removal before the reader. The reader never seeds and must
preserve both A and B cookies/localStorage; cleanup only follows passing those
assertions. The clear scenario retains its original strict empty-A/intact-B
assertions. No delay, replay or per-cookie deletion is added. If the baseline also
loses cookies, normal documented app termination is a separate comparison to
investigate before attributing loss to A clearing or claiming durability.

Run [38840541909](https://github.com/ashutoshpw/zmux/actions/runs/38840541909)
at `c3b4a42a8631f86652e194158113b0ee09612f23` confirmed loss in that
no-removal baseline. Before exit, each public cookie store contained its correct
probe cookie with `sessionOnly=false` and future expiry. After reaped preparer
PID 3246 exited 0, reader PID 3267 retained localStorage A/B but both JavaScript
cookie strings and both public cookie-store arrays were empty. Both phases had no
UUID removal or all-data clear. No timeout or pending callback occurred. The
baseline does not establish cookie persistence under this fixture's direct exit,
so the V5 B loss cannot be attributed to A clearing from these runs.

The documented normal-termination comparison changes only successful phase
completion to
[`NSApplication.terminate(_:)`](https://developer.apple.com/documentation/appkit/nsapplication/terminate(_:)).
Apple documents its termination request/delegate/`willTerminateNotification`
sequence; it does not promise cookie disk persistence here. The probe retains an
actual `applicationWillTerminate` log receipt with exact phase/PID. The runner
requires that receipt plus the original owned exit/reap barrier before a reader
launch. Failure still exits 1, with no direct-exit fallback, delay or retry. All
baseline A/B retention assertions and store observations remain unchanged. The
comparison's outcome, not the API name, must establish its effect on persistence.

Run [38840974347](https://github.com/ashutoshpw/zmux/actions/runs/38840974347)
at `7447906867b30f28ed3ae5a067564b7e70f9da8e` observed the exact normal
`applicationWillTerminate` event for preparer PID 13599 and reaped exit 0. Fresh
reader PID 13622 still retained localStorage A/B but neither cookie, in both JavaScript
and public cookie-store observations. No removal or all-data clear ran. This does
not establish cookie durability even with normal termination. V5/V6/V7 require
reassessment, not a delay or another assumed flush path.

The shared owned process runner always signals its reserved process group before
reaping, even after the leader exits. Existing artifacts show an entirely zombie
group only when a signal returns EPERM; they contain no pre-signal process lineage.
They cannot establish whether live WebKit helpers were terminated during cleanup
or were unrelated launchd/XPC processes. The next diagnostic preserves the original
signaling/reaping policy and adds bounded public `ps` observations before native
spawn, immediately before owned cleanup signaling and after cleanup. Records contain
PID/PPID/PGID/state/comm only, without process arguments or environment. Owned-group
and observed descendant relationships are recorded; other WebKit candidates are
explicitly unproven. No observed PID becomes signal authority.

Snapshots have a two-second deadline, byte/row/count bounds and an independently
owned/reaped `ps` child; observation failure is retained as incomplete evidence.
Only the standalone probe wraps its existing process owner; shared cleanup code is
unchanged. Instrumentation itself changes elapsed time, so a different cookie result
alone will not establish cause. Actual lineage/group evidence must precede any
cleanup-policy change. Portable real-child observation/exit/reap checks pass; actual
Darwin relationships remain a hosted diagnostic gate.


## Single creation-path discriminator (source prepared, native pending)

The next diagnostic preserves V8b's no-removal baseline, normal AppKit termination,
fixed 45-second native phase limits, original JavaScript `probe=A/B` and localStorage
assertions, and unchanged owned-group cleanup and process observations. It adds
exactly one separately named `native_created=A/B` persistent cookie to each exact
identifier store through public `httpCookieStore.setCookie`, awaiting each callback
once. The new name cannot accidentally satisfy the existing `probe=A/B` substring
checks. No cookie backup, replay, added delay or production change is involved.

Creation first reads each original JavaScript cookie and requires the expected
loopback domain, root path, future expiry and non-session/non-secure/non-HttpOnly
attributes. The new cookie uses those same observed domain/path/expiry values.
Both JavaScript and public store observations are retained and checked before
termination and before any existing fresh-reader cleanup. The independent runner
requires both exact names, values, comparable persistence attributes, two acknowledged
preparer-only sets, no reader sets, and unchanged expiry across the process boundary.
An unknown native-set callback has its own receipt field, separate from an unknown
removal callback. All original callback/retirement semantics remain unchanged.

The public `WKHTTPCookieStore.h` says `setCookie` completion fires once the cookie
has been stored. The examined identifier-store and `isPersistent` documentation
and WebKit profile article describe persistent profile restoration, but do not
promise an fsync barrier for that callback. This experiment compares acknowledged
creation paths; it does not claim a flush mechanism. Both cookie paths surviving
would establish only this changed fixture result, not prove the cause of prior
loss. Native-only survival leaves the original JavaScript durability gate failing;
both paths disappearing points to a shared persistence issue. No outcome alone
accepts profile deletion or unlocks production lifetime ownership.

Portable Python syntax, validator positive and mutation controls, and diff whitespace
checks pass. Swift compilation and actual macOS/WebKit execution remain pending.

### V9 actual creation-path outcome

Diagnostic `137255d88b31cf095a65f9bc28c2afd4cb32e670`, run `38843434898`,
compiled with Swift 6 strict concurrency and warnings-as-errors (21.096 seconds).
Preparer PID 7941 exposed both original `probe` and `native_created` cookies in
both A/B public stores and JavaScript. Both native-set callbacks arrived; all four
cookies had expiry 1792123378, domain `127.0.0.1`, root path, and false
session-only/secure/HttpOnly flags. Both localStorage values were correct.
Normal AppKit termination was logged, and the owned preparer was reaped with
return code 0 before reader PID 7970 was launched from the same executable path.

The fresh reader retained localStorage A/B but its two public cookie arrays and
both JavaScript cookie strings were empty. Both native-cookie flags were false.
The unchanged original no-removal baseline guard failed before any removal or
cleanup; reader `nativeCookieSets` and `removals` were empty. Its owned child
returned 1 after 2.107 seconds. There were no timeouts, log truncation or unknown
callbacks. Acknowledged public-native creation was not a sufficient resolution
for cookie persistence in this fixture. This identifies neither a root cause nor
removal-specific cross-profile damage; no production lifetime decision follows.

Artifact `12022810426` ZIP SHA256 is
`f2c9592d130ff29bd9c27d7547eb67e7246344b88ee8ffa4da3f63951040d7be`.
Its archived executable and plist bind the exact source SHA/bundle ID; five retained
source inputs match published Git byte-for-byte and all six claimed source hashes
match Git. The retained workflow source copy is absent because `upload-artifact`
excludes hidden paths by default and this diagnostic workflow lacks
`include-hidden-files: true`; its hash and exact committed source remain available.
Independent phase/source/archive/matrix receipt is
`/tmp/zmux-b01-native-cookie-creation-result-review-20261016.json`, SHA256
`c4e176059d68feec87227b953ec3f4900321fc891274bb96fee451d400838bd3`.

No repeat is authorized. Next cookie work is read-only investigation of fixture
assumptions and public persistence scope: same bundle URL/ID and executable across
both phases, identifier namespace, loopback-origin policy, and guarantees actually
made by public APIs. Do not add delays, invent a flush contract, weaken cleanup,
or replay/backup cookies. Current main's actual native smoke takes priority.

### Read-only persistence-scope audit after V9

Both recorded phases execute the identical absolute bundle executable path; the
runner verifies unchanged executable/archive bytes before reopening. Their report
and archived plist bind the same bundle ID/source SHA. Both phases use the same
identifier UUID A/B, loopback URL (including its port) and nonce, and the reader
recovers both exact localStorage values. These observations support consistent
profile/origin access but do not independently prove the cookie backend identity.

The tested origin is first-party plain HTTP on `127.0.0.1`; this is not evidence
for HTTPS user-origin persistence. All four original/native cookies were observed
with host `127.0.0.1`, root path, a future expiry and non-session flags. The examined
identifier-store/profile/isPersistent docs specify persistence and restoration but
no loopback exception or callback-based fsync guarantee. The public cookie-store
header documents default policy Allow; the actual policy was not observed in each
phase. Do not infer an installed-OS bug or invent a policy fix from these results.
Further cookie experiments remain paused; native-smoke failure triage is active.

### Caller-store lifetime discriminator (source prepared, no native run)

The next bounded comparison changes only the preparer's direct caller lifetime:
release both views as before, but strongly retain both exact persistent UUID store
instances through normal `NSApplication` termination. The existing JS and native
cookie creation, origin/bundle/identifiers, reader no-reseed assertions, 45-second
phase limits, public cleanup after successful retention checks, and owned-process
signal/reap behavior remain unchanged. No extra delay, policy change, private API,
cookie replay or production mutation is added.

This is motivated by public WebKit source at
[`30281d59519dd0df7cf7b0326d31b726ddb04289`](https://github.com/WebKit/WebKit/commit/30281d59519dd0df7cf7b0326d31b726ddb04289)
(2026-10-16). `WebsiteDataStore::~WebsiteDataStore` removes its network session;
`NetworkProcessProxy::removeSession` sends `DestroySession`; and
`NetworkProcess::destroySession` removes the network and cookie storage sessions.
The separate `NetworkProcess::didClose` path iterates still-present network
sessions and asks `platformFlushCookies` to save their cookie stores. Native
`WebCookieManager::setCookie` acknowledges its set path separately from that save
path. This supports a concrete hypothesis that dropping the stores before parent
closure changes which sessions can participate in close-time persistence. It does
not prove that this happened on the runner or that retaining a caller reference
preserves a particular internal session.

Pinned source references:

- [Data-store destruction, lines195-220](https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/UIProcess/WebsiteData/WebsiteDataStore.cpp#L195).
- [Network session removal, lines1531-1547](https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/UIProcess/Network/NetworkProcessProxy.cpp#L1531).
- [Session destruction, lines772-792](https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/NetworkProcess/NetworkProcess.cpp#L772).
- [Parent connection close, lines265-279](https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/NetworkProcess/NetworkProcess.cpp#L265).
- [Cocoa cookie save path, lines256-280](https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/NetworkProcess/cocoa/NetworkProcessCocoa.mm#L256).
- [Native cookie set acknowledgment, lines129-137](https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/NetworkProcess/Cookies/WebCookieManager.cpp#L129).

The retained macOS15.7.9 logs do not identify the installed WebKit/CFNetwork build.
The examined source revision is not matched to that installed implementation, and
CFNetwork persistence remains opaque. The close-time source path is a testable
hypothesis, not an API contract or fsync guarantee. Existing process observations
also do not prove whether the unowned WebKit candidate belongs to these stores.

The preparer now checks both original store object identities, UUIDs and persistence
flags after view release. It records released views and retained exact caller
stores. The synchronous `applicationWillTerminate` event repeats those actual
reference checks, and the runner requires the exact phase/PID plus both true
retention flags and released views before launching the reader. Neither receipt
claims visibility into NetworkProcess state. Reader termination still requires
released views and no direct store references, and all strict cookie/localStorage
and public cleanup gates remain in place.

Python syntax and validator controls pass: positive preparer/reader receipts;
rejection of each missing retention/view-release fact, wrong phase/PID, duplicate
or missing termination receipt, and the original V9 cookie-loss reader. Historical
cookie attributes are checked at their retained observation time only. This is
portable validation, not new native evidence. Swift compilation and one frozen
native comparison await parent source review and separate authorization.

If both cookie paths survive, the result supports caller lifetime affecting this
fixture, but does not prove a specific internal save path or general durability.
If either original cookie is lost, the strict baseline still fails; caller
retention is not sufficient under the observed conditions. Neither outcome accepts
profile deletion or changes production lifetime ownership without design review.

Repository `CARGO_TARGET_DIR=target/agent-validation/profile-caller-retention/cargo
scripts/check` passed exit 0 before freezing this comparison: formatting, workspace
Rust tests, strict all-target Clippy, notice inventory, foreign C ABI and profile
lease checks. Log: `/tmp/zmux-b01-caller-retention-repository-check-20261016.log`.
No Swift compiler is available on this host; these checks do not compile or execute
the WebKit diagnostic. The previously dirty V9 outcome/scope notes are preserved
alongside this appendix. No push or diagnostic/native rerun has occurred.

### V10 actual retained-caller outcome: cookies survive, B cleanup blocked

Authorized single run
[38848628624](https://github.com/ashutoshpw/zmux/actions/runs/38848628624) at
`421b797f811adfb7f0ff56c9995caceefb405817` compiled with Swift 6 strict
concurrency/warnings-as-errors in 18.987 seconds. Preparer PID5284 recorded both
views released and both exact caller stores retained, including the synchronous
`applicationWillTerminate` event. It exited/reaped0 before reader PID5308.

The fresh reader observed all four original JS/native-created cookies: A/B public
stores each contained `probe` and `native_created`, with unchanged attributes and
expiry1792127084. Both exact localStorage values survived. Reader
`nativeCookieSets` was empty; no replay or reseeding occurred. These strict data
checks passed before cleanup. This contrasts with V9's loss and supports caller
lifetime affecting this fixture. It does not prove a particular internal
NetworkProcess session/save path or establish general durable-cookie acceptance.

The overall run failed after successful `cleanup_a` public callback, at the
unchanged B view-release guard: `Released B WKWebView still exists; cleanup
prerequisite is unproven`. B's five-second release deadline elapsed with
`bViewReleased=false`; no `cleanup_b` call followed. Reader returned1, without
normal AppKit termination. The original owned process-group cleanup/reap behavior
was preserved. No callback was unknown, native phase timed out or log truncated.
The retained process observations do not establish unowned WebKit helper state,
and B's public UUID-store cleanup remains incomplete.

Source inspection identifies a preexisting strong local in `runFreshProcess`:
`guard let view = bView` followed by `await load(view)`. That local can survive in
the async frame through later `dropB`. The observed live B view is actual evidence;
this exact retaining edge remains a source-backed explanation requiring a separate
reviewed repair. Earlier cookie failures never reached this cleanup guard. No
cleanup weakening or automatic rerun is authorized by this result.

Artifact12024347372 ZIP SHA256:
`46dd7fc8c04655ba42e56caebf4100db316e0385e5e6c11f5760c934fbdd595c`.
Five retained source files match exact Git and the reviewed freeze byte-for-byte;
all six claimed input hashes match. The unchanged workflow's hidden-source-copy
omission remains disclosed. Archived app/plist/executable identities and both
phases' namespace/origin linkage were verified. Independent interpretation receipt:
`/tmp/zmux-b01-caller-retention-result-review-20261016.json`, SHA256
`a6f28d3d4404bf5e0765a0de2dfd851f7eb26ec1f982c91929d9d2ccc18553e1`.
No production integration or further diagnostic run follows automatically.

### Fresh-reader B load scope repair

Parent reviewed the exact helper proposal SHA256
`635e539e30e3fa8ee43130e2811389a910cdd6d08d9d63355933d64c4b05c4e4`.
Fresh B creation/guard/load now lives in a returning `loadFreshB()` helper, matching
the existing A load pattern. The outer `runFreshProcess` activation no longer binds
a strong B view local across the later cleanup awaits. This changes only caller
reference scope; the strict five-second weak-view release guard remains unchanged.
Cookie creation/assertions, no-reseed reader, preparer store retention, public
cleanup order and owned process cleanup are unchanged.

The retained V10 run is the actual native red for this cleanup path. Portable
Python syntax and original validators accept V10's observed cookie/retention data
at its historical observation time and reject missing-retention/original-cookie-loss
controls. This does not accept V10's failed cleanup or replace a new native result.
Source diagnosis receipt:
`/tmp/zmux-b01-fresh-b-lifetime-diagnosis-20261016.json`, SHA256
`5afb3951e87bb4316f0efdc2b7cacad85e21d80e8b67cad5a5a918d201239503`.
Parent explicitly authorized this exact repair, portable/repository validation,
commit/push and one subsequent diagnostic. No production merge is authorized.

The repaired `CARGO_TARGET_DIR=target/agent-validation/profile-caller-retention/cargo
scripts/check` passed exit0: workspace tests, formatting, strict all-target Clippy,
notice inventory, foreign C ABI and profile lease checks. Log:
`/tmp/zmux-b01-fresh-b-lifetime-repository-check-20261016.log`. Portable validation:
`/tmp/zmux-b01-fresh-b-lifetime-portable-validation-20261016.json`. Swift/native
compilation and lifecycle outcome remain pending the one authorized diagnostic.
