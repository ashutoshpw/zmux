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

## First Darwin file-owner run

Run `38775130649` compiled the strict Swift file probe but failed its initial
directory walk with ENOTDIR. The repair preserves lexical POSIX path components
through preparation/publication and uses realpath for fixture setup: Foundation
normalization may restore macOS /var or /tmp aliases. All no-follow directory walks,
inode checks, no-replacement publication and alias refusal remain. The probe runs
both default and /tmp temporary roots; native smoke records selected panel paths.
Parent source review accepts the scoped change; shell syntax/diff checks pass.
Hosted execution is still required to establish the actual path behavior and repair.

Run `38775942552` stopped at Swift6 compilation of the new alias-refusal fixture:
its MainActor closure crossed a nonisolated helper without Sendable. The helper
now explicitly takes a Sendable async closure; its URL/UUID/actor captures support
that contract. No assertions or production isolation were relaxed.

Run `38776244720` at `eaf8cda` passed strict Darwin file-owner probes and universal
app compilation, then timed out during packaged download interaction. The log
showed NSSavePanel still selecting `/Users/runner/Documents/fixture.bin` in the
same turn that the fixture assigned its temporary destination. The shared fixture
helper now waits for the actual panel URL to match before OK, retaining exact-byte
and CLI-history assertions. Timeout diagnostics skip a second app launch because
the original Launch Services app may remain alive. Independent source review,
Python syntax and scoped diff checks pass; AppKit execution remains pending.

Run `38778435343` at `c33ec40` also compiled, but the real save panel remained at
`/Users/runner/Documents/fixture.bin` after the bounded destination wait. It did
not establish native save acceptance. Its second diagnostic app also failed a
sidebar check without key focus; that is not the original download failure.

Apple's public [`NSSavePanel.ok(_:)` documentation](https://developer.apple.com/documentation/appkit/nssavepanel/ok(_:))
explicitly states: "In macOS 10.15 and later, you cannot call this method
programmatically to trigger the OK action." The fixture's direct `panel.ok(nil)`
was therefore not a supported acceptance driver; another property-delay retry
cannot establish that path. No download-owner defect was demonstrated.

The replacement candidate uses a separate compiled Accessibility driver against
the exact packaged application PID and executable. It requires existing
`AXIsProcessTrusted()` and `CGPreflightPostEventAccess()` authorization, without
prompting, modifying TCC, or posting global/frontmost-app keystrokes. It identifies
one real save sheet by its filename and Save control, sets its actual filename,
uses the standard Go to Folder shortcut targeted to the app PID, and enters the
fixture directory through the actual UI. The app independently requires the real
panel URL to match before a second driver phase performs the Save button's public
`AXPress` action. Saved-record cleanup, exact file bytes, CLI history and event
assertions remain unchanged. The app waits for its helper outside MainActor, with
explicit deadlines and owned-process cleanup. Driver UI waits block only the
separate helper process; its termination/reaping ultimately depends on the OS and
the outer hosted fixture deadline.

This candidate has not yet compiled or executed on macOS. A host without existing
UI automation authorization fails with an explicit native-acceptance gate; no
download success or full-parity claim follows. Exact-source native compilation,
permission evidence and real save acceptance remain required. Production
`BrowserDownloads` and filesystem guards are unchanged.

Run `38813108591` at `f62a10cff9c2d613b9ba0b3047036c4fa35010dd`
passed the packaged close/save/reopen journey, including restored split ratio.
The next Save panel driver compilation failed before the broad app smoke: this
SDK exports no `kAXSheetsAttribute`, and strict warnings reject three
`unsafeBitCast` calls where `unsafeDowncast` is appropriate. No Save panel UI
interaction or browser keyboard fixture ran in that attempt.

The driver now names the same `AXSheets` attribute directly and uses the suggested
downcasts after the existing AX type-ID checks. Its PID/permission checks, tree
bounds, UI actions, deadlines and assertions are unchanged. The early native
source gate also type-checks this driver for both Apple architectures before Rust
compilation. Python syntax and whitespace checks pass locally; strict Darwin
compilation and actual permission-checked Save panel acceptance remain pending.

## Required host capability and independent assertion progress

Run [38836209900](https://github.com/ashutoshpw/zmux/actions/runs/38836209900)
at `12d5c140aa4341fff4fc5345fd2b02aa2c22911e` compiled the driver and reached
the first actual save sheet. Its external process reported
`accessibility_trusted=false`; native save acceptance remained gated. No prompt,
permission change or destination callback substitution occurred. The passed
browser keyboard and workspace/surface ordering fixtures are separate executed
subsets. Browser PNG/native render, remote clipboard, local tmux and terminal
special-key checks were not reached in that run. The later automatic diagnostic
app's BUSY/timeout does not identify the original failure or prove original exit.

The continuation fixture moves the unchanged download/history assertions after
those independent checks. Their actual progress is atomically retained in bounded
`native-smoke.json.stages.json`, correlated by source SHA and app PID. Its scope
is assertion progress; it never establishes shared shutdown or process exit. An
error records the active stage and already completed stages, then still throws
the original error. Full smoke JSON remains unavailable unless every assertion,
including real native download acceptance, passes.

At the final download stage, a separately launched driver's `preflight` checks the
exact app PID/executable and existing AX/post-event capabilities without prompting
or inspecting/mutating a sheet. This precedes fixture browser creation, download
admission and sheet presentation. Its retained `native-download-preflight.log`
reports capability only. An untrusted host still fails the required download gate;
an authorized host must still execute the original select/accept UI phases and
all saved-byte/history/cancellation assertions.

The original browser HTTP origin is carried from the automation setup. Remote
clipboard cleanup restores its original workspace; local tmux restores its
original workspace/surface; special-key cleanup closes its temporary tab and
restores the previous selected surface. The final download fixture creates its
own browser without focus, with the same main-window owner and existing cleanup
barrier. No production download/resource behavior changed. Local whitespace and
source review are available; strict Swift compilation and actual native stage
receipts remain hosted gates because this Linux host has no Swift/AppKit SDK.
