# CLI, native UI, and delivery plan

## Existing owners and help behavior

- `apps/cli/src/main.rs` owns argument dispatch and the `Parsed`/`Offline` routes.
  `apps/cli/src/schedule.rs` owns the schedule grammar and dispatch request.
  Parsing and offline help finish before any app socket, SSH operation, or stdin
  read.
- `apps/cli/src/registry.rs` is the public command catalog and help source.
  `top_help()` prints each command's usage but not its description; `help()` adds
  descriptions and groups. One `schedule` entry in the `remote` group must carry
  concise multiline usage for all verbs so `zmux --help` discovers the feature.
  `zmux help schedule` and `zmux help remote` carry details.
- Route `zmux schedule --help`, `zmux schedule create --help`, and other nested
  help forms to offline output before app/SSH access. Add deliberate help paths
  for `zmux help schedule edit` and other supported subcommands; unknown nested
  help should fail as help without contacting a remote host.
- `registry::catalog()` is built from the same `COMMANDS` registry and
  `apps/cli/src/registry/contracts.rs` verifies live parser leaves/routes.
  Include each implemented verb and its route, arguments, bounds, and states in
  those contracts. Keep historical `apps/cli/contracts/baseline` inventories
  frozen. Reviewed counterpart mappings belong in live contracts or
  frozen-coverage metadata; never rewrite the historical baseline for this feature.
- The CLI already uses `--pane` for a local Zmux pane. Schedule commands use
  `--surface UUID` for the managed SSH connection and `--remote-pane TARGET_TOKEN`
  for a token returned by `schedule targets`; raw tmux pane IDs are rejected.
  Require `--surface` on every schedule verb.

## Public command shape

```text
zmux schedule setup --surface UUID [--check]
zmux schedule status --surface UUID [--request-id UUID]
zmux schedule targets --surface UUID
zmux schedule create --surface UUID --remote-pane TARGET_TOKEN (--at RFC3339 | --in DURATION) (--message TEXT | --stdin) [--submit] [--request-id UUID]
zmux schedule list --surface UUID
zmux schedule show TASK_UUID --surface UUID
zmux schedule edit TASK_UUID --surface UUID --expect-version N (--at RFC3339 | --in DURATION)? (--message TEXT | --stdin)? [--request-id UUID]
zmux schedule cancel TASK_UUID --surface UUID --expect-version N [--request-id UUID]
zmux schedule history --surface UUID [--limit N]
```

The implemented names follow these behaviors:

- `setup --check` is a read-only compatibility preflight. `setup` without it is
  the explicit helper install/enable action. Start, stop, restart, upgrade, and
  removal are documented systemd/user-admin operations, not implicit job verbs.
- Create requires exactly one time form and one message source. `--submit` is
  create-only and defaults off. The server returns a stable task UUID and version.
- Dispatch has a five-second grace after its deadline. More than five seconds
  overdue before claim, a task becomes `missed` without a claim; if a claimed
  task crosses the grace during pre-paste work, mark it `missed` without input.
  If text was accepted but optional Return becomes stale, record partial
  `outcome_unknown`, send no Return, and never replay either phase.
- Edit requires a nonempty patch containing message, time, or both. Message-only
  preserves the deadline; time-only preserves the exact message; both are one
  atomic versioned update. `--in` is measured from remote edit acceptance time.
- List/show expose the current task version and pinned target before editing.
  Help examples show fetching the current version, then message-only, time-only,
  and combined edits. Cancel uses the same expected-version contract.
- Every create/edit/cancel wire mutation carries a unique client request UUID.
  CLI generates one by default, retains its ID and a payload hash privately
  before sending, and prints the ID before remote I/O. `--request-id` lets
  scripts provide a stable value. The remote ledger returns the original receipt
  for an exact duplicate and rejects a conflicting payload. `schedule status
  --request-id UUID` fetches a task-mutation receipt, including the task UUID for
  a create whose acknowledgement was lost. Status without a request ID can list
  locally retained unresolved task mutations. The remote service keeps up to
  1024 mutation receipts for 30 days and caps private state at 16 MiB. It allows
  up to 100 pending tasks and reserves one receipt slot per pending task for a
  valid version-matched cancellation; a create needs two free receipt slots and
  an edit needs one unreserved slot. It reports `request_expired` when expiry is
  provable, otherwise `not_found_or_expired` with `outcome_unknown`. Never
  automatically retry an ambiguous request or create a replacement ID.
- Setup install carries a request UUID but has no durable mutation receipt. If
  its outcome is uncertain, run `schedule setup --surface UUID --check`; do not
  query `schedule status --request-id` or retry installation blindly.
- Do not consume `--stdin` for help, malformed arguments, unsupported time forms,
  or a missing target option. Bound it before starting SSH. Do not print message
  bodies in command diagnostics.

Help examples should show the current version before editing, and then all three
patch forms:

```text
zmux schedule show TASK --surface SURFACE
zmux schedule edit TASK --surface SURFACE --expect-version 3 --message "Build reminder"
zmux schedule edit TASK --surface SURFACE --expect-version 3 --at 2026-10-23T09:00:00-07:00
zmux schedule edit TASK --surface SURFACE --expect-version 3 --at 2026-10-23T09:00:00-07:00 --message "New reminder"
```

These are independent alternatives, each starting from the version 3 returned
by `show`; they are not a sequence. After one edit succeeds, fetch the new version
before another edit. The examples omit `--request-id` to show the generated
default. Automation can provide a UUID and reuse it only with the exact original
request body.

## CLI and native ownership

All queue mutations and dispatch state belong to the Rust remote helper/store.
The CLI is a typed client of that protocol. The AppKit schedule window sends the
versioned JSON protocol through the managed-SSH bridge; Swift owns transient
presentation and drafts, while the helper owns durable jobs and delivery state.
Swift must not create a second timer, copy jobs into a competing store, or display
a remote mutation as accepted before the helper acknowledges it. The Workspace
menu entry is enabled for a selected managed SSH terminal. A native smoke fixture
covers the menu, editor, and lost-reply receipt flow, but its packaged run remains
pending.

Show each task's remote account, exact pane ID, current window/session display
metadata, resolved deadline and local time zone, version, and status. Keep message
editing and scheduled-time editing in one versioned form. If SSH is disconnected,
preserve an unsaved local draft and show it as a draft; do not claim the remote
job changed or was paused. When a create/edit/cancel acknowledgement is uncertain,
query the receipt with the original request UUID before submitting anything again.

Native disk/network work stays off AppKit's main thread and the shared UI action
executor. Reuse the Rust/session ownership boundaries and the existing app bridge
patterns rather than moving remote storage or scheduling into view code.

## Implementation and verification state

1. **Contract and compatibility:** protocol/schema, host/account and tmux identity
   checks, helper metadata, explicit setup/status/service lifecycle, limits,
   retention, and failure strings are represented in the scheduler contract and
   source. Packaged helper availability and hosted artifacts are separate gates.
2. **Remote helper/store:** the Rust scheduler owns private bounded storage,
   stable task/version and request IDs, time parsing, service transitions, restart
   recovery, and exact-target dispatch. Record the focused tests and isolated
   Linux/tmux fixture evidence in the task status.
3. **SSH and CLI:** the CLI parser, all verbs, registry help, nested offline help,
   catalog leaves, opaque target-token validation, and task-mutation journal are
   implemented. Verify invalid grammar before stdin/socket access and preserve the
   frozen historical catalog baseline.
4. **AppKit:** the schedule list/editor/history source uses the remote helper
   protocol through the managed-SSH bridge. Record native draft and uncertain-
   request behavior separately from CLI evidence.
5. **Acceptance:** run race/restart/target/time/payload checks, then a packaged
   Mac smoke for queue display, edits, and receipt reconciliation. The CLI and
   macOS user docs now cover entry points, service lifecycle, limits, and receipts.
   Report portable, isolated SSH/tmux, and packaged-native evidence separately.

## Behavioral validation

Exercise real owners and isolated fixtures; do not rely on a mock claiming tmux
accepted text or a source-grep inventory that duplicates the command contract.

- Use an owned remote SSH fixture and real tmux server, never the user's server.
  Cover supported/unsupported host, explicit setup, service status/stop/restart,
  account authorization, helper identity, private storage, queue/history bounds,
  host linger, and no local fallback.
- Race edit and cancel against timer claim. Cover earlier/later time edits,
  message-only/time-only/combined edits, stale versions, duplicate request IDs,
  conflicting request IDs, and old timer invalidation.
- Check the five-second dispatch-grace boundary, a task becoming late before
  paste (missed with no input), and optional Return becoming stale after accepted
  text (partial unknown, no Return, no automatic replay).
- Disconnect the app/SSH client after remote acknowledgement; restart the helper;
  prove future jobs rearm, overdue work becomes missed, uncertain dispatch is not
  replayed, and offline drafts/unknown outcomes are not shown as success.
- Move the same pane between windows and change its name/index while it remains in
  the bound session; prove dispatch follows its pane ID. Remove it from the session,
  replace the session/server/socket, or reuse names/indexes; prove fail-closed with
  no input to another pane.
- Validate RFC3339 offsets/Z, `--in` based on remote acceptance for create/edit,
  remote past-time rejection, DST gaps/overlaps, 4096-byte UTF-8 limits, multiline
  and bracketed paste behavior, unsupported control bytes, and optional Return.
  Include crashes between claim, text, optional Return, and result persistence.
- Verify `zmux --help`, `help schedule`, `help remote`, nested `--help`, and the
  machine catalog show every implemented verb and safe edit examples without
  contacting an app or SSH server.
