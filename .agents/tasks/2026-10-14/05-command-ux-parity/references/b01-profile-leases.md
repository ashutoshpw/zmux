# B01 profile lifetime lease primitive

This original MIT prerequisite does not expose clear/delete or change native
WebKit construction. Catalog rename remains independent of store lifetime.

A dedicated bounded worker owns stable per-profile UUID lock files under the
validated private state directory. Shared leases coexist; an exclusive request
fails busy without waiting while another process retains its shared lease.
Acquisition checks current catalog identity before opening and again under the
lease, validates nofollow/private/current-user/zero-byte/single-link files, and
compares anchored descriptor and directory-entry identity before and after the
nonblocking OS lock. Lock files are never unlinked or atomically replaced.

Lease service epoch, monotonic token ID, owner epoch/store UUID, profile UUID and
mode fence release. Lost acknowledgments retain physical ownership and capacity;
status uses the original owner, never a replayed acquisition. Requests/retained
leases are bounded at 8/64 and waits occur only off UI/core executors. A native
integration must retain its service for process lifetime, beyond Desktop shutdown.
This primitive alone does not establish actual WebKit lifetime ownership.

Session export rejects the UUID lease filename namespace everywhere, including
ASCII case aliases. Restricting this rule to the exporter's own state directory
failed a real second-worker regression: a desktop using another state directory
could atomically replace the first desktop's held lease pathname. The same focused
case now requires ReservedStatePath, unchanged inode and continued contention.

Portable validation uses an isolated source archive of committed 3fc2364 plus only
this slice under target/agent-validation/profile-leases-isolated-3fc2364. A real
independent child process shares a lock, blocks exclusive ownership after the
parent releases its lease and after the catalog worker joins, then is killed to
verify actual OS crash release. The test also protects distinct-profile isolation,
rename/export inode retention, recovered status and stale-token successor safety.
A separate file-safety case refuses symlink/hardlink/public/nonempty locks and
unknown profile acquisition without replacing files. Existing persistence and
session-persistence cases must continue passing.

Native integration remains open. Browser wrapper closure or deinit does not prove
that framework-retained WKWebView/website-store/download/conversion references are
physically gone. A conservative process-lifetime shared owner is a safe next
prerequisite; restart-only release is not final clear/delete-after-closing parity.
Native store retirement requires documented/public lifetime evidence or an actual
retirement receipt before any shared lease release. No destructive admission is
part of this primitive.

The lease namespace must also match the actual WebKit storage namespace before
native admission. The current native constructor receives only the profile UUID,
not Desktop's configurable Rust state directory. Two Desktop catalogs at different
paths containing the same UUID therefore cannot safely choose independent lock
roots. Integration must use a validated stable root common to every native owner
of that WebKit UUID (or a conservative per-user superset) and validate catalog
identity separately. Binding acquisition only to the caller's state path remains
insufficient. No arbitrary state-directory lock root is admitted for native use.
