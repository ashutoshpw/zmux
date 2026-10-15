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

This subset does not yet implement connection sharing, durable remote sessions,
automatic reconnect, mosh, remote CLI relays or file/image upload/drop/paste. Additional
SSH options use a bounded allowlist; full option compatibility remains open. Ordinary
text input works over the SSH PTY, while file/image transfer parity requires the next
R01 work. No cloud deployment is required for this SSH path.

The loopback integration test covers actual key authentication, input/output, remote
PTY allocation, no-focus creation, fresh reopen without command replay, and server-loss
exit without local command fallback. It uses temporary host/client keys, private known
hosts and a custom fixture shell; it does not establish two-host or packaged Mac parity.
