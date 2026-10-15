# B01 explicit persistent browser profiles

## Frozen first-slice contract

Original MIT implementation. This slice adds `browser profiles add NAME`, `browser
profiles list`, and `browser open [TARGET_OPTIONS] [--profile NAME_OR_UUID] URL`.
No rename/delete/clear, browser import, sessionStorage/history persistence or default
persistent profile is claimed. Omitted profile retains per-host private storage.
Profile names are case-sensitive, trimmed nonempty UTF-8 strings, at most 128 bytes,
without controls; UUID-shaped names are reserved. Names are unique. Profile identities
are generated nonnil UUIDs, immutable across reopen/restore; names never become paths.
The catalog is append-only, at most 32 profiles and 16 KiB of JSON. Durable operations
retain at most 8 pending operations and 16 total retained receipts. CLI
admission is once-only and status polling obeys its original absolute deadline.
Receipts/status requests carry a runtime epoch so restarted counters cannot retarget
a pending command to another creation.

Rust runtime owns the installation-local catalog and resolves names/UUIDs. Catalog
startup/add operations use an instance of the existing bounded filesystem worker and private state
directory, current-user ownership and no-symlink policy. A sibling 0600 temporary,
file sync, atomic rename and directory sync precede publication. Catalog failure is
explicit; no unknown profile falls back to another store. Add returns only after the
durable catalog receipt succeeds; it establishes metadata, not native datastore proof.
Catalog creation is unavailable when durable runtime state is disabled. Catalog adds
serialize under a dedicated advisory lock and re-read current disk state before validation;
concurrent writers cannot lose profiles. Startup/catalog work never waits on the UI
thread or workspace executor. Disk failures preserve previous authoritative catalog.

Core model v4 adds optional `browser_profile_id` to Surface and `profile_id` to raw
CreateBrowser. Null means private. Targeted creation accepts an optional profile name
or UUID and resolves it at runtime before pure core admission. Central candidate
validation rejects persistent references absent from the loaded catalog, including
raw/native actions, reopen and restored topology. Navigation/moves preserve binding;
reopen gives a fresh surface/host identity while copying the profile UUID. Browser
storage binding cannot change on a mounted surface.

Session format v2 saves `profile_id` only for browser content; no cookie/storage data
or profile catalog is exported. An explicit v1 reader migrates absent profile fields
to private; unknown versions/fields and v1 documents containing v2 profile fields are
rejected. V2 must decode profile identity without dropping it. Existing URL query,
fragment and observed-title privacy restrictions remain. Startup waits for catalog
load before admitting restored topology; explicit restore rejects unknown profile
UUIDs atomically before terminal launches or model publication. Profile UUIDs belong
to the installation catalog, not replaceable session identity namespaces.

Native AppController owns a bounded UUID→WKWebsiteDataStore cache. BrowserView gets
its chosen store before WKWebView construction. Persistent uses the macOS14 public
`WKWebsiteDataStore(forIdentifier:)`; private gets a fresh `.nonPersistent()` per
host. Popup creation inherits its parent's exact profile UUID through the shared
creation action. Existing hosts are never silently reused with another binding.
Native catalog/store construction failure does not substitute default/private state.

## Verification ownership

Real filesystem-worker tests protect durable add/reload, bounds, invalid/corrupt/unsafe
files and concurrent writers; real runtime/CLI tests protect resolution, policy,
unknown-profile atomicity and receipt polling. Core tests protect reopen/move and
versioned session migration. Packaged WebKit tests use isolated generated profile
UUIDs: same-origin A hosts share cookies/localStorage; B/private hosts do not; close
last A then reopen preserves A while old refs fail. App restart uses a dedicated
fixture instance with the same tagged bundle and catalog; never quit or replace the
user's running application. Native relaunch persistence remains open until that
actual restart journey is executed. Local tests do not establish WebKit behavior.

## Current status

Schema/ownership contract recorded before implementation. Implementation now threads
profile identity through core model4, sessionv2, actual durable catalog worker, CLI
receipts and immutable WebKit datastore construction. Profile receipts are bounded
and runtime-epoch scoped; catalog workers drain during bounded desktop cleanup.
Read-only list reports the current runtime snapshot (refreshed on startup and local
successful add), not continuous synchronization with another process's catalog edits.

Focused native fixture authored: actual bundled CLI creates A/B profiles and browser
hosts, checks UUID-addressed persistent stores versus private, shares cookie/localStorage
only inside A, closes all A hosts then reopens with fresh refs, and uses an explicit
DOM target-blank fixture to check popup profile inheritance. This is not pointer input
parity. Fixture startup now passes a unique persistent directory through the production
NativeBridge/core API; standalone notification-permission smoke remains transient.
Native compilation, this new fixture and actual app-relaunch storage persistence remain
hosted gates. No B01 full profile-management or V01 closure claim is made. Apple official documentation confirms the UUID datastore
initializer and identifier are public from macOS14, matching the app deployment target:
https://developer.apple.com/documentation/webkit/wkwebsitedatastore/init(foridentifier:)

## Local verification receipt

Private `CARGO_TARGET_DIR=/tmp/zmux-b01-target` focused run passes 46 tests:
`cargo test -p zmux-desktop-runtime --test persistence --test session_persistence
-p zmux-workspace-core --test session_document -p zmux --test browser`
(the package selection also executes the desktop browser target). This includes actual
filesystem add/reload/lock/cross-writer preservation/32-profile bounds, corrupt/symlink
rejection, session migration, unknown-profile restore before imported launches, and
public CLI create/name+UUID/raw-ingress/reopen/restart/read-only/old-epoch rejection.
Strict Clippy passes for core/runtime/CLI with all targets. All existing tests in those
three packages passed before the final receipt-epoch/cleanup additions; the focused run
above covers the final owners. Full current workspace and hosted native checks remain
parent-owned delivery gates. Shared Cargo build outputs are not used as evidence.

Parent acceptance: isolated index snapshot passed `scripts/check` (207 Rust tests,
formatting, strict Clippy, notices and C ABI). Fresh independent review accepted the
catalog/migration/host/receipt boundaries and independently reran the real CLI profile
lifecycle test. Native compile, packaged isolation/reopen and app-relaunch remain gates.
