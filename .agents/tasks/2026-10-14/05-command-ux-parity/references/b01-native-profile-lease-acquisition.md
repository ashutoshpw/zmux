# B01 native shared-lease acquisition design

This slice keeps clear/delete unavailable. It conservatively retains shared leases
until process exit; browser closure, Desktop shutdown and cancellation cannot
release them. Restart-only release is not final clear/delete-after-closing parity.

## Namespace invariant

Apple documents that init(forIdentifier:) returns the persistent store identified
by UUID, creating it if absent. Native construction currently passes UUID only;
Desktop's configurable catalog path is not a WebKit namespace input.
https://developer.apple.com/documentation/webkit/wkwebsitedatastore/init(foridentifier:)

Read-only upstream WebKit reference evidence (no implementation copied) resolves
the default UUID store below user-domain NSLibraryDirectory/WebKit, adding bundle
identity outside a container, followed by WebsiteDataStore/UUID. This motivates a
conservative per-Library lock scope that also covers distinct bundle identifiers.
Reference commit: https://github.com/WebKit/WebKit/blob/30281d59519dd0df7cf7b0326d31b726ddb04289/Source/WebKit/UIProcess/WebsiteData/Cocoa/WebsiteDataStoreCocoa.mm#L275
The UUID configuration joins that root with its identifier. Upstream source is
reference evidence, not a promise about installed OS internals; no WebKit source
is copied or translated into this slice.

Native code resolves user-domain Library through the same public Foundation
FileManager directory API, then appends a fixed app-owned Application Support
lease directory. The root never comes from Desktop state_directory, a profile
name, caller environment override or a browser command. Native contexts in the
same Library namespace use this one root. Catalog identity is checked separately.

The lease worker starts with this fixed nofollow/private root and retains its
anchored descriptor for service lifetime. Each acquisition supplies the actual
Desktop catalog_directory and revalidates the UUID before and under the shared
lease. Real independent-process tests must prove that distinct catalogs containing
the same UUID contend on this common lock root.

## Native acquisition and lifetime

A process-static Rust service survives every Desktop handle. Native Desktop
creation records its actual catalog path and the fixed OS-resolved lease root.
Acquisition is a narrow native-only bounded JSON operation, separate from generic
socket/browser commands. It runs off MainActor and the Rust core executor.
Owner epoch plus store UUID identifies exactly one attempted store acquisition.
The service returns its full epoch/token/owner/profile/mode receipt. On uncertain
acknowledgment, only status for the original owner may recover ownership; never
replay acquisition or replace the owner.

MainActor reconciliation starts one bounded acquisition per desired profile UUID
and constructs WKWebsiteDataStore only after the matching shared token is known.
Pending, busy, unknown, malformed or unavailable acquisition never substitutes a
private/default store. Existing stores reuse their already-held process ownership.
The native service retains tokens even if the originating task/coordinator/Desktop
shuts down or a framework callback outlives its wrapper. An exact acquired token may be released off UI only when cancellation is
confirmed before any WKWebsiteDataStore construction. Unknown acknowledgments
first reconcile the original owner; they never acquire another token. Once the
MainActor constructs a store, no production release path is exposed until
documented physical retirement or an actual retirement receipt supports it.
Cached stores and downloads therefore cannot outlive their lock.

## Verification boundaries

Portable process tests and real C callers cover common-root contention, distinct
catalogs with the same UUID, strict request decoding, bounded capacity before
acquisition, owner status and lease retention after logical Desktop shutdown.
Actual packaged WebKit acceptance must observe shared ownership before store use
and actual independent-process exclusive refusal while that store is mounted.
Native compilation and execution remain separate gates from portable checks.

## Local acquisition-slice receipt

The exact slice on committed b659b73 was isolated under
`target/agent-validation/profile-lease-acquisition-isolated-b659b73` and passed
`scripts/check`: 323 Rust tests, formatting, strict workspace Clippy, notice
inventory and the original plus new real foreign C ABI checks. The ignored Rust
child entrypoint is invoked by its primary real-process test, not skipped coverage.
That child uses a copied catalog at another state path and the same common root.
The foreign C host checks capacity before mutation, independent exclusive refusal,
original-owner status and retained physical lock after logical Desktop destruction;
its exact unconstructed token then releases without replacing the inode. The
standalone native probe was compiled with strict warnings and independently
returned busy for a held shared lease and failure for an unlocked inode.

Swift/WebKit compile and the authored actual mounted-store external contention
journey remain hosted gates. The new helper has a five-second child alarm and no
descendants, and runs off MainActor. Existing manager admission waits now include
the actual UUID store rather than treating topology publication as native readiness.
Clear/delete, retiring constructed-store release and full profile lifecycle parity
remain unavailable. The conservative lease survives logical Desktop shutdown.
