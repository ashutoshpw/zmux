# B01 browser downloads: first-slice contract

Status: original implementation and portable checks complete; native compilation and
packaged acceptance pending, 2026-10-15. Original MIT
implementation only. The pinned sources below were inspected to characterize the
public interface and observable behavior; no implementation is imported or translated.

## Observed compatibility contract

Pinned cmux revision: `3ba653b9e5afb11d0bc578031142950d6136697e`.

- `CLI/CMUXCLI+BrowserDownload.swift:8-193`: the handler receives a resolved browser
  surface; this does not require an explicit selector from every CLI caller.
  `download list|ls` accepts one `--limit N` or `--limit=N`, integer 1 through 25.
  Unexpected arguments and repeated limits fail. `download wait` and bare `download`
  share the wait grammar: one positional path or `--path PATH|--path=PATH`, optionally
  one `--timeout-ms INTEGER` or `--timeout SECONDS`, including equals forms. Timeout
  forms conflict; seconds must be finite and convert to milliseconds with a floor
  of one. `--` terminates option parsing. Conflicting/repeated paths and unknown
  options fail. A path is an observation target, **not a save destination**.
- `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Policy/BrowserDownloadWaitTimeout.swift:30-73`:
  default wait is 10,000 ms; the handler clamps to 1 through 120,000 ms. Client
  response timeout adds five seconds. List uses a 20-second response timeout.
- `Sources/TerminalController.swift:10589-10666,10688-10731,10735-10895`: resolve
  workspace/browser once before waiting, without an explicit focus action. With a
  nonempty trimmed path, wait observes that path becoming existent with reported
  size greater than zero; it does not verify a matching download or completion,
  choose a destination, write/copy a file, or consume any download event. An already
  nonempty path returns immediately. Missing parent/watcher setup fails explicitly;
  a file that stays absent/empty times out. Success includes `path` and
  `downloaded: true`. No-path wait returns the oldest queued eligible event or waits
  for one from the resolved surface; `started` is excluded. Saved/cancelled/failed
  events consume that download ID and suppress its later duplicate events. Timeout
  observes only; it does not cancel a download. Event success includes `download`.
- `Sources/TerminalController+BrowserDownloadHistory.swift:1-155`: list is a newest
  first, non-consuming history snapshot, independent of the wait event queue.
  Default/max limit is 25. Entries expose `download_id`, `filename`, `path`,
  `path_exists`, `status`, and `bytes`, with nulls for unavailable metadata; payload
  includes count/limit and resolved workspace/surface identifiers. Upstream list
  statuses are downloading/saved/failed. Repeated list calls do not consume events.
- `Sources/TerminalController.swift:6957-7009`: missing workspace/browser and
  non-browser targets fail rather than choosing an unrelated browser. Neither wait
  branch re-resolves a new focused surface midway. Path wait is a filesystem
  observation convenience; its success is not evidence of our ledger's saved state.

These facts refine BC-08 in `audit-browser-content.json`. They do not establish an
upstream guarantee about concurrent waiters, regular-file/symlink acceptance, durable
history, or event delivery after a lost socket response. Zmux will state its own
bounded ownership guarantees rather than infer them from implementation accidents.

## Shared owner and native lifetime

Add a dedicated Rust download owner, separate from document-scoped browser action
receipts. One preselected correlation UUID, admitted authoritatively by Rust, identifies a
download and its immutable initiating runtime
epoch, identity namespace, surface UUID, host UUID, profile UUID/private binding and
captured document generation. Admission validates the exact native browser/profile;
callbacks never consult current focus. Later navigation does not retarget or cancel
that admitted resource. Surface close, host replacement and desktop shutdown cancel
the exact physical download and pending destination sheet. Late callbacks cannot
revive a terminal record or affect a reopened surface with another host identity.

States: awaiting_destination, downloading, finalizing, saved, cancelling, cancelled,
failed. A saved path is published only after successful file finalization. Native
WebKit owns network I/O; Rust owns admission, lifecycle/status, history, cancellation
requests and event consumption. Pages receive no native bridge or filesystem ability.

Implemented first-slice bounds: four physical downloads globally/two per surface, one
visible destination sheet, 25 history records per surface/64 globally, 512 KiB total
retained metadata and 16 wait tickets. Active handles retain capacity until native
terminal acknowledgement and owned-file cleanup complete. Progress coalesces to at
most four updates per second per job; bodies never enter Rust/JSON. Limits reject
explicitly; history eviction never discards an active handle. Metadata/error/path
bounds and terminal-event retention are checked together before admission. A byte
threshold driven by WebKit progress is cancellation policy, not a hard disk quota.

`WKDownload` conversion covers both action-requested downloads and response MIME
conversion. The coordinator captures origin at the policy/conversion boundary and
adopts each object once. HTTP(S) redirects remain supported in the same WebKit profile,
with a finite redirect budget and scheme/credential validation at every hop. Unsupported
schemes, redirect overflow and unsupported authentication fail visibly; no URLSession
re-fetch, cookie substitution, certificate bypass or automatic retry is allowed.

## Destination and cancellation policy

Default policy asks asynchronously with the real NSSavePanel; no blocking runModal.
Suggested names are untrusted: bounded safe basename, no separators/control characters
or dot/dotdot, with a safe fallback. The explicit selected URL is validated separately.
The first slice refuses an existing final destination, including symlinks; overwrite
is unavailable rather than implemented by deletion. Filesystem work stays on a bounded
background owner, never the main actor or workspace executor.

Create an exclusively owned private sibling staging directory and pass WebKit a
nonexistent payload URL inside it. On finish, validate the owned regular file and
publish atomically without replacing an existing final path. Preserve WebKit file
attributes/quarantine. Failure/cancel removes only owned staging objects and reports
cleanup failure; it never deletes a completed/user file. Finish/cancel/publication
races resolve once at the shared owner. A late cancellation cannot remove a saved file.
Resume data is not persisted or advertised; no restart/resume claim is made.

Apple public API provenance, live checked 2026-10-15: WKDownload and WKDownloadDelegate
are available from macOS 11.3 and use MainActor APIs. WKDownload conforms to
ProgressReporting. The destination must be a file that does not exist in an existing
writable directory; returning nil cancels. Redirects follow by default unless the
delegate supplies a policy, so the slice must implement its explicit bounded policy.
Sources: https://developer.apple.com/documentation/webkit/wkdownload and
https://developer.apple.com/documentation/webkit/wkdownloaddelegate/download(_:decidedestinationusing:suggestedfilename:completionhandler:).

## CLI/read policy and acceptance

The first coherent implementation includes actual native downloads plus Rust list/wait
contracts. List and wait observation are permitted by read-only control policy; Cancel
is a mutation. List and the native Downloads view use the same history. No-path wait
atomically assigns one eligible terminal event to one bounded epoch/surface ticket;
ticket status polling is repeatable and cannot consume another event after a lost
reply. Started/progress updates never satisfy it. History remains after consumption.
Path wait uses a separate bounded background filesystem observer, never the native
save coordinator, and never consumes events. Neither timeout cancels physical work.
CLI submits once and polls its exact ticket under the same absolute handler deadline
and reply slack; no socket or executor thread blocks for the full wait window.

Rust owner tests protect origin/profile fencing, navigation independence, close/late
callbacks, cancel/finish races, admission/history/event bounds, single consumption,
repeatable status, read-only policy and wait timeout. Real filesystem tests protect
no-replace, symlink/collision refusal and owned cleanup. CLI tests exercise aliases,
flag conflicts/limits, timeout conversion and actual public ticket polling; path wait
must leave queued completion events untouched and must not claim ledger completion.

Packaged WebKit acceptance uses real attachment/MIME responses, profile-cookie-gated
downloads, exact saved bytes, slow cancellation/failure, save-sheet cancellation,
concurrent tabs/profiles and navigation/close during work. Actual native controls and
bundled CLI list/wait observe the same owner; fixture writes/DOM eval do not claim
pointer parity. macOS compilation/native execution remain separate hosted gates.
PDF preview, blob/data schemes, resume, persistent history, download settings and
remaining B01 workflows stay open until independently implemented and verified.


## Implemented owner and current evidence (2026-10-15)

Rust owns exact admission tokens, immutable origin, lifecycle, cancellation requests,
newest-first history, once-only terminal events and repeatable wait tickets. List and
cancel scope by stable surface UUID, so numeric IDs reused during a restore cannot
inherit earlier history or events. Native lifetime intentionally survives document
navigation, with exact host detach, namespace/profile loss and resource age cancellation.
Finalizing is Rust's publication reservation: a cancellation accepted first prevents
reservation; a reserved publication rejects later cancellation. A terminal receipt
never revives on duplicate or forged callbacks. Cleanup failure retains physical credit.
History has a 320 KiB serialized allowance, with the remainder of the 512 KiB budget
reserved for at most sixteen bounded independent ticket results/paths and queue metadata.
Fields are bounded and every metadata-bearing record insertion/update checks its size.
Ticket results survive unrelated history eviction. Restart history/resume remain absent.

List does no filesystem I/O: `path_exists` is null and `path_exists_source` explicitly
says `not_checked`. Path observation deliberately retains the observed compatibility
predicate: ordinary following metadata and size greater than zero, including symlinks
and non-regular entries. This convenience does not establish safe destination selection,
a matching ledger record or completed bytes. Destination publication separately refuses
symlink components/preexisting leaves, uses owned private sibling staging and no-replace
Darwin publication. macOS `/var` and `/tmp` aliases are refused during strict destination
walk; native fixtures use resolved `/private/...` paths. The validation reopen can follow
intermediate aliases, but must match the retained parent descriptor's inode/device;
actual publication/cleanup remain relative to the owned descriptors.

The single path worker owns physical observation credit until completion is drained.
Elapsed deadlines alone never release that credit. It checks deadlines before each
metadata operation; the runtime checks a fresh clock before applying a completion.
A disconnected completion channel proves the physical worker ended, clears its credits
and surfaces a worker fault to pending receipts instead of silently stranding them.
CLI resolves scoped/caller/current targets once at admission, also accepts legacy
`browser SURFACE download ...`, and supports list `--json` (JSON is already default).
List gets a default twenty-second client budget; an explicit global deadline wins.
Wait's absolute client deadline is checked both before and after the polling sleep.

Native sources convert both action-requested and attachment/MIME response downloads,
retain exact policy objects during conversion, capture origin before asynchronous
admission, fence late conversion/close/process callbacks and preserve the original
WKWebView/datastore. One asynchronous save sheet is bounded by time; destination work
runs on a separate actor. Native acknowledgement checks the returned Begin token,
uncertain outcomes retain their token/credits, and bounded asynchronous shutdown waits
for physical cancellation plus cleanup before closing the bridge. The Downloads menu
opens actual shared history and Cancel uses the common Rust action route.

Portable validation passed:
- Eleven production download-owner tests: navigation/close/late callbacks, terminal
  reservation and forged tokens, once-only events/repeatable tickets/list, cancellation
  credits, history eviction, real path observations and deadline/worker/restore fences.
- Nine public browser CLI tests including two new download cases: real trusted native
  events feed list/wait, path waits leave events untouched, implicit/legacy selectors,
  list `--json`, read-only denial and conflicting/invalid grammar.
- Twelve existing browser-owner regression tests and one actual native ABI download
  test: malformed/oversized/null storage, response capacity before admission, exact
  returned token and stale/forged owner rejection.
- Full `CARGO_TARGET_DIR=/tmp/zmux-b01-downloads-target scripts/check` passes
  (workspace format/tests/clippy, license inventory and foreign C ABI checks). Three negative controls remove one guard
  each and fail for the intended observation: disconnected worker leaves pending,
  stale-clock completion succeeds after deadline, and old-namespace ticket consumes
  a new-namespace event. Restored guards pass the same focused owner command.

Native gates are implemented in source but **not executed here**:
`scripts/check-browser-download-files` compiles the real Darwin owner against actual
filesystem collision/symlink/quarantine/cleanup cases. `BrowserDownloadSmoke` and the
packaged runner serve attachment/MIME/action, same-cookie redirect and slow-body
fixtures; use real NSSavePanel OK/Cancel; compare exact saved bytes and shared CLI
history/event/path results; verify navigation-independent cancellation and retained
focus; and invoke the actual Downloads menu. The packaged flag is
`browser_downloads_native`. These are future execution gates, not passed receipts.
Broader cross-profile concurrent downloads, network truncation, close during active
body, chooser timeout, authentication/redirect overflow and physical disk failure
journeys still require independent native coverage. No full B01 parity claim is made.

Approved conversions retain their exact navigation policy object and originating
BrowserView delegate even after surface close; close/expiry requests cancellation
without treating an absent `didBecome` callback as proof that no physical download
exists. Only a rejection before returning `.download` can abandon that conversion.
The arriving physical object is adopted and cancelled with acknowledgement, or the
bounded shutdown reports unconfirmed ownership. A save-sheet network failure cancels
and drains its suspended destination decision before terminal acknowledgement;
a duplicate conversion never releases the existing physical job.

Parent independent full `scripts/check` passed in
`/tmp/zmux-downloads-parent-verify-z_a145zh` (log
`/tmp/zmux-downloads-parent-check.log`). The final list-default timeout correction
was then verified with the two public download CLI tests. Independent source review
and eleven owner, one ABI and two CLI cases passed; native gates remain pending.
