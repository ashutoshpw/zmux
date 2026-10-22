# CLI, native UI, and delivery plan

## Existing owners and help behavior

- `apps/cli/src/main.rs` owns argument dispatch and the `Parsed`/`Offline` routes.
  A future `schedule` parser module and dispatch path belong here; help parsing
  must finish before any app socket, SSH operation, or stdin read.
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
  `--surface UUID` for the managed SSH connection and `--remote-pane %ID` for the
  exact remote tmux pane. Require `--surface` on every schedule verb.

## Proposed public command shape

```text
zmux schedule setup --surface UUID [--check]
zmux schedule status --surface UUID [--request-id UUID]
zmux schedule targets --surface UUID
zmux schedule create --surface UUID --remote-pane %17 (--at RFC3339 | --in DURATION) (--message TEXT | --stdin) [--submit] [--request-id UUID]
zmux schedule list --surface UUID
zmux schedule show TASK_UUID --surface UUID
zmux schedule edit TASK_UUID --surface UUID --expect-version N (--at RFC3339 | --in DURATION)? (--message TEXT | --stdin)? [--request-id UUID]
zmux schedule cancel TASK_UUID --surface UUID --expect-version N [--request-id UUID]
zmux schedule history --surface UUID [--limit N]
```

Exact naming can follow established CLI conventions, but keep these behaviors:

- `setup --check` is a read-only compatibility preflight. `setup` without it is
  the explicit helper install/enable action. Start, stop, restart, upgrade, and
  removal are documented systemd/user-admin operations, not implicit job verbs.
- Create requires exactly one time form and one message source. `--submit` is
  create-only and defaults off. The server returns a stable task UUID and version.
- Edit requires a nonempty patch containing message, time, or both. Message-only
  preserves the deadline; time-only preserves the exact message; both are one
  atomic versioned update. `--in` is measured from remote edit acceptance time.
- List/show expose the current task version and pinned target before editing.
  Help examples show fetching the current version, then message-only, time-only,
  and combined edits. Cancel uses the same expected-version contract.
- Every wire mutation carries a unique client request UUID. CLI generates one by
  default, retains it locally before sending, and prints it in JSON receipts and
  errors. `--request-id` lets scripts provide a stable value. AppKit generates
  and retains one with its pending local draft. A retry reuses the same UUID and
  exact body. The remote ledger returns the original receipt for a duplicate and
  rejects a conflicting payload. `schedule status --request-id UUID` fetches the
  original receipt, including the task UUID for a create whose acknowledgement
  was lost. Status without a request ID can list locally retained unresolved
  requests. Document receipt retention; return `request_expired` when expiry is
  provable, otherwise `not_found_or_expired` with `outcome_unknown`. Never
  automatically retry an ambiguous request or create a replacement ID.
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
The CLI is a typed client of that protocol. The AppKit schedule list/editor uses
the same Rust request and response types; Swift must not create a second timer,
copy jobs into a competing store, or display a remote mutation as accepted before
the helper acknowledges it.

Show each task's remote account, exact pane ID, current window/session display
metadata, resolved deadline and local time zone, version, and status. Keep message
editing and scheduled-time editing in one versioned form. If SSH is disconnected,
preserve an unsaved local draft and show it as a draft; do not claim the remote
job changed or was paused. After a timeout, query/retry only with the original
request UUID and version. A service failure should include the next explicit
setup/status action.

Native disk/network work stays off AppKit's main thread and the shared UI action
executor. Reuse the Rust/session ownership boundaries and the existing app bridge
patterns rather than moving remote storage or scheduling into view code.

## Reviewable implementation phases

1. **Contract and compatibility:** finalize protocol/schema, host/account and
   tmux identity checks, supported OS/service matrix, helper packaging, explicit
   setup/status/service lifecycle, limits, retention, and failure strings.
2. **Remote helper/store:** add private bounded storage, stable task/version and
   request IDs, time parsing, remote timer, atomic state transitions, restart
   recovery, and exact-pane dispatch. Verify with isolated local fixtures.
3. **SSH and CLI:** call the helper only through a managed live master; add
   parser/dispatch and all verbs; update one registry entry, nested offline help,
   catalog output, and live contract leaves. Verify missing-master/helper failures
   before any mutation and preserve the frozen historical catalog baseline.
4. **AppKit:** add shared pending/history views and atomic message/time editor,
   including local draft and unknown-request reconciliation states.
5. **Acceptance and docs:** run race/restart/target/time/payload checks, then a
   packaged native smoke for queue display and edits. Document unsupported hosts,
   systemd linger, service lifecycle, limitations, and CLI examples from generated
   help. Report portable, isolated SSH/tmux, and packaged-native evidence separately.

## Future behavioral validation

Tests are not part of this planning-only change. When implementation is authorized,
exercise real owners and isolated fixtures; do not rely on a mock claiming tmux
accepted text or a source-grep inventory that duplicates the command contract.

- Use an owned remote SSH fixture and real tmux server, never the user's server.
  Cover supported/unsupported host, explicit setup, service status/stop/restart,
  account authorization, helper identity, private storage, queue/history bounds,
  host linger, and no local fallback.
- Race edit and cancel against timer claim. Cover earlier/later time edits,
  message-only/time-only/combined edits, stale versions, duplicate request IDs,
  conflicting request IDs, and old timer invalidation.
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
