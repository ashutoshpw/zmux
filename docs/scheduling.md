# Persistent scheduled messages

Zmux can store literal text on a supported remote Linux account and send it to
one exact tmux pane at a future time. After the service accepts a task, its
schedule survives closing the Mac app and disconnecting SSH. The helper uses a
per-user systemd service on the remote host; there is no local timer fallback.

Scheduling requires a managed SSH surface in Zmux. A manually opened SSH shell
does not carry the connection identity needed to route a task. Open or select a
managed SSH surface, then check compatibility before installation:

```sh
zmux schedule setup --surface SURFACE_UUID --check
```

In the macOS app, select a managed SSH terminal and choose
**Workspace > Scheduled Messages…**, or right-click its workspace and choose
**Schedules**, to open a Schedules tab beside terminal/browser tabs. Reopening
selects the existing tab for that SSH source. The tab can be moved and closed
like other tabs; its SSH source stays fixed. Closing that source leaves the
Schedules tab offline. Session restore preserves source IDs; undo-close creates
new terminal IDs, so it never silently rebinds an old Schedules tab.

The native queue has a resizable right-side inspector. Use its toolbar button to
hide or show the inspector; the same draft is retained. **New message** shows the
inspector again. The status bar's information button opens setup details with
**Check Setup** and **Install / Enable**. Full status text is available there and
in the status bar tooltip; **Check Request Status** remains directly available
when an acknowledgement is unresolved.

Choose **In** for a delay such as `5m`, `1h30m` or `2d`, with 1m/5m/15m/1h
presets, or **At** for a local date/time. Delays allow descending integer d/h/m/s
units, from 1 second through 365 days, evaluated at remote acceptance.
Pending rows show live countdowns; **Absolute** shows resolved local times.
History always shows absolute timestamps. Countdown text uses your Mac's clock
and never establishes delivery or changes a remote job. The queue and inspector
resize inside the tab. On shorter windows the inspector form scrolls while its
save/cancel buttons stay available.

Select a pending message to edit its text, time or both, then save the versioned
change. A message-only edit preserves the exact remote deadline, including seconds.
Drafts remain transient in the tab and are not persisted in session documents. If the acknowledgement for a create, edit,
or cancel is unclear, reconnect to that managed SSH terminal and choose
**Check Request Status** to reconcile the original request before submitting
anything again.

The check reports host architecture, tmux availability, systemd-user support,
helper compatibility, service state and linger state. When it reports
`supported: true`, install and enable the service explicitly:

```sh
zmux schedule setup --surface SURFACE_UUID
```

Task creation never installs or starts the service. If installation times out
or its result is uncertain, check the current state instead of repeating it:

```sh
zmux schedule setup --surface SURFACE_UUID --check
```

## Create and manage tasks

Discover panes and use the returned opaque token. The token binds a task to a
specific remote account, tmux server incarnation, session and pane; a raw pane
ID such as `%17` is not accepted.

```sh
zmux schedule targets --surface SURFACE_UUID
zmux schedule create --surface SURFACE_UUID --remote-pane TARGET_TOKEN \
  --in 30m --message "Review the deployment" --submit
zmux schedule list --surface SURFACE_UUID
zmux schedule list --surface SURFACE_UUID --relative
zmux schedule show TASK_UUID --surface SURFACE_UUID
```

Default CLI list output remains JSON for automation. `list --relative` prints a
human-readable countdown snapshot based on this computer's clock. Run it again
to refresh; the remote job state remains authoritative.

`--at` accepts an RFC3339 timestamp with an explicit offset or `Z`. `--in` is
measured from the remote helper's acceptance time. Messages are literal UTF-8,
up to 4096 bytes; Zmux does not append a newline. `--submit` optionally sends
one Return after the text is accepted and is off by default. A `sent` result
means tmux accepted the input, not that the foreground application acted on it.
Dispatch has a five-second grace after the deadline. A task more than five
seconds overdue is marked `missed` before claim; if it passes the grace during
pre-paste work, it is marked `missed` without sending text. If text was already
accepted but the optional Return becomes stale, history records a partial
`outcome_unknown`: the Return is not sent, and the service does not replay the
text or Return.

Show a task before editing or cancelling, and use its current version:

```sh
zmux schedule edit TASK_UUID --surface SURFACE_UUID --expect-version 3 \
  --message "Review the release"
zmux schedule cancel TASK_UUID --surface SURFACE_UUID --expect-version 4
```

An edit changes the message, deadline or both in one versioned operation. A
message-only edit preserves the exact deadline; a time-only edit preserves the
message. Editing does not retarget a task. Fetch the latest version after every
successful edit.

## Capacity and receipts

The per-user service supports up to 100 pending tasks, keeps up to 1024 mutation
receipts for 30 days, and limits its private state to 16 MiB. It reserves one
receipt slot per pending task so a valid version-matched cancellation remains
recordable at capacity. A new create needs two free receipt slots: one for the
create receipt and one for the future cancellation. An edit needs one
unreserved receipt slot. Capacity errors leave the requested change unapplied.

Create, edit and cancel each have a request UUID. Zmux prints and records that
ID in a private local journal before remote I/O. If an acknowledgement is lost,
query its receipt with the same ID; do not retry the operation with a new ID:

```sh
zmux schedule status --surface SURFACE_UUID --request-id REQUEST_UUID
```

Setup installation also prints a request UUID but does not create a durable task
receipt. Use `setup --check` to recover its current state. The service keeps
task history separately from request receipts; see `schedule history` for
terminal delivery results.

## Service lifecycle

The remote unit is `zmux-scheduler.service`. Its state and lifecycle can be
managed through the remote user's systemd session:

```sh
systemctl --user status zmux-scheduler.service
systemctl --user stop zmux-scheduler.service
systemctl --user start zmux-scheduler.service
systemctl --user restart zmux-scheduler.service
```

If the service must run after the last SSH login closes, the host administrator
may need to enable user lingering:

```sh
loginctl enable-linger "$USER"
```

Zmux does not enable lingering or make privileged host changes. When the service
is stopped or the host is down, future tasks are checked when it starts again.
Tasks more than five seconds past their deadline become `missed` before claim;
the service does not catch up after that grace. It also checks the grace before
text reaches tmux, so a task that becomes too late during pre-paste work is
marked `missed` without input. If text was accepted but its optional Return is
stale, the partial result is `outcome_unknown`; no Return is sent and the
service does not replay either phase.
