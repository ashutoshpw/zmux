# SSH workspaces

With the app running, open a remote terminal workspace:

```sh
zmux ssh user@server --name Backend --identity ~/.ssh/id_ed25519
```

Arguments can include `--port`, `--ssh-config PATH`, explicit agent forwarding (`-A`
or `-a`), repeated `--jump HOST` and allowlisted `--ssh-option KEY=VALUE` values.
Relative identity/config paths are resolved against the CLI's working directory.
`--ssh-config none` suppresses normal config loading. OpenSSH owns config aliases,
host-key verification and interactive authentication, on the terminal's PTY worker.

`--command TEXT` starts an explicit remote command. Alternatively place remote command
arguments after `--`; OpenSSH combines them using the remote account's shell semantics.
The managed terminal requests a remote PTY for both shell and command launches.
For interactive shell launches, zmux adds a temporary session-only Claude attention
plugin on the remote host. Bash, zsh, POSIX sh and fish have startup adapters; user
startup files and Claude settings are retained. Turn completion and provider
permission notices travel through that exact SSH PTY into its local inbox, without
forwarding the local control socket or requiring cloud services. Explicit remote
commands are not wrapped. Set `ZMUX_CLAUDE_INTEGRATION_DISABLED=1` in the remote
shell to bypass Claude wrapping; `ZMUX_SHELL_INTEGRATION_DISABLED=1` in the app's
launch environment disables integration for new local and SSH shells. A supported
Claude version with repeatable `--plugin-dir` is required. A private session FIFO
relays fixed attention events to the original terminal, including from nested tmux
sessions. One blocked relay process lives with each integrated shell; no tmux
configuration is changed. An inherited wrapper bypasses expired hook directories.
On legacy Bash3/4 owners, the signal-reset stage uses `/usr/bin/perl` and immediately
execs the interactive shell; it adds no resident interpreter. If that tool is
missing, zmux starts the ordinary shell and reports that integration is unavailable.
User Perl startup/module preferences are preserved for the launched shell.
Configured OpenSSH `RemoteCommand` launches are preserved and bypass automatic
integration. A bounded `ssh -G` probe runs on the launch worker before ordinary
shell launches; OpenSSH may evaluate configured `Match exec` during this probe.
Notification banners
also require enabling zmux's macOS system-notification permission.
The launcher passes the SSH program and connection options directly as argv.
OpenSSH still owns configured ProxyCommand, Match exec and LocalCommand behavior.
This launch requires full local-control access and remains denied to remote relay origins.

`--no-focus` (or `--focus false`) preserves the existing workspace selection. The JSON
result includes `created_workspaces` with exact workspace/pane/surface IDs and UUIDs,
even when focus remains elsewhere. Launch and authentication are asynchronous; query
the returned surface with `read-screen --surface UUID` to inspect output/exit status.
A successful create response acknowledges workspace creation, not authentication.

New tabs/splits from a remote terminal retain the endpoint. Reopening a closed remote
terminal retains that endpoint and clears its explicit startup command; saved terminal
input is not replayed. It opens a fresh connection and follows current ssh_config/login
defaults. Remote cwd restoration is not implemented; fresh remote terminals show unknown
cwd until observed. Remote paths are never used as local launch directories.

After an SSH process exits or its launch fails, the native app's reconnect icon on that
same tab starts a fresh login using the retained endpoint. It does not resume the prior
shell or replay the explicit startup command or terminal input. This is a manual recovery
control; automatic or durable reconnect and CLI reconnect are unavailable.

File drops and Finder file paste use the same shared operation as:

```sh
zmux paste-files --surface SURFACE_UUID -- ./image.png ./notes.txt
zmux list-uploads
zmux cancel-upload UPLOAD_ID
```

For a local terminal these insert quoted local paths. For a managed SSH terminal,
regular files upload through that terminal's private authenticated connection. Each
terminal owns its connection; tabs do not share a terminal master with one another.
File operations never authenticate again or fall back to another route. A closed or
unavailable connection fails the upload. Successful paths are quoted and inserted into
the originating surface/session, even after focus changes; no command is submitted.
Closing the origin or cancelling suppresses insertion. Acknowledged completed files remain reported after a later failure/cancellation.
`remote_result_uncertain` identifies a started operation whose result could not be
confirmed, including connection loss before receipt; verify remote temporary storage
in that case. Cancellation suppresses insertion and does not delete acknowledged files.

The native header shows pending byte progress and an icon to cancel the latest active
upload. CLI results distinguish bytes handed to SSH, remote completion, and insertion
outcome. Remote completion requires an exact byte-count acknowledgement. Sources are
opened on a worker without following symlinks; FIFOs/directories are rejected. Bounds:
16 files, 64 MiB per file, 256 MiB total, four active jobs, eight recent records, and a
two-minute operation deadline checked between I/O calls. A filesystem call stalled in
the kernel cannot be forcibly interrupted; it stays off the UI/core executor and shutdown
reports workers that miss its deadline. Remote uploads require a POSIX shell with mktemp,
cat and wc.

Image files work as files. Raw clipboard images, directory transfers, custom upload
rules, durable remote sessions, automatic reconnect, mosh and remote CLI relays remain
unavailable. Additional SSH options use a bounded allowlist; full option compatibility
remains open. No cloud deployment is required for this SSH path.

Persistent messages can be scheduled on an exact remote tmux pane through a managed
SSH surface. They are stored and dispatched by the remote per-user service rather than
by a local timer. See [Persistent scheduled messages](scheduling.md) for setup,
capacity, receipts and service lifecycle.

The isolated loopback test covers actual key authentication, input/output, remote PTY,
no-focus creation, fresh reopen without command replay, authenticated upload after the
fixture's authorized key is removed, exact binary bytes and Unicode/quote/shell-shaped
filenames. It checks insertion after switching to a local terminal, stalled cancellation,
origin closure, late cancellation after a remote acknowledgement, failed-acknowledgement
cleanup and server-loss failure without local fallback. Temporary fixture keys
and configuration are removed. Two-host, password/jump-host, physical drop/paste, raw
clipboard-image and packaged native acceptance remain separate gates.
