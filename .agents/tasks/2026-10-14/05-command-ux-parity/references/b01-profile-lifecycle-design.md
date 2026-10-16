# B01 clear/delete lifecycle design (read-only investigation)

No destructive command or release of constructed-store ownership is implemented.
The acquisition slice still requires actual macOS validation. This design removes
the restart-only limitation for a sole local owner without treating wrapper
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
must establish supported behavior. The public removal callback is the sole data
removal receipt. Weak object disappearance, stopLoading, canceled navigation,
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
4. Evict only the idle local store cache, release application-owned native refs,
   and observe actual WKWebView weak registrations plus existing physical job
   receipts. Keep the shared lifetime lease while this prerequisite is pending.
5. Under retained admission exclusive, transition this process's exact known
   lifetime owners to one lifetime-exclusive reservation. Existing foreign shared
   owners cause busy. Restore all required local shared ownership before releasing
   admission; never expose an unlocked gap. Other processes' cached stores remain
   valid busy owners; do not steal their tokens or infer retirement from age.
6. With writer authority retained, call public identifier removal once, after the
   documented WKWebView release prerequisite. Residual framework/network ownership
   may still return busy. No background retry loop or synthetic completion.
7. On known busy/error, restore local lifetime shared authority while admission is
   still exclusive, then unlock admission and report the exact failure. If shared
   restoration fails, retain writer authority and capacity; reopening stays denied.
8. Timeout/cancel after admission or callback dispatch is unknown, never success.
   Retain writer, lifetime-exclusive authority and physical callback capacity until
   that original callback/status resolves. No replay and no same-UUID reopen.
9. Public success permits the correlated durable metadata sequence below. Only its
   exact known completion releases writer authority. Late callbacks cannot affect a
   successor; no successor is admitted while the original outcome remains unknown.

Clear can use the identifier-removal owner while retaining the immutable catalog
UUID, so the next permitted open creates empty storage. Delete removes catalog
membership only after public removal succeeds. Never substitute private/default
storage, delete unrelated history or silently rewrite old session/closed bindings.

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

The probe remains unexecuted. Its actual mounted-busy/released-success result is
a design feasibility gate, separate from the production lease acquisition check.
