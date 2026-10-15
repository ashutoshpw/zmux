# Local control and authentication

`zmux --help`, `zmux help navigate`, `zmux guide` (alias `--skill`),
`zmux commands` and `zmux version` work without a running app. The command catalog
describes the implemented subset and reports parity as incomplete. `zmux capabilities`
queries the running app's effective socket access, protocol and resource limits.

## Protocol compatibility

The bundled CLI uses local-control protocol v2. The new server also accepts legacy v1
requests and replies in v1, subject to the same access/password policy. A legacy client
cannot bypass a password requirement. New CLI versions require the updated running app;
a v1 response gives restart/update guidance and never causes a mutation retry. The native
C ABI remains v1 and the workspace model remains v3.

## Socket selection

The CLI uses `--socket PATH`, then `ZMUX_SOCKET_PATH`, then the legacy `ZMUX_SOCKET`
alias, then the platform default. Every selected path must be absolute. The native app
honors the same environment overrides at startup. Explicit paths never fall back to
another instance. The socket and containing directory remain private to the current
user; the listener checks the peer's operating-system UID before reading requests.

## Passwords and access modes

The native app reads `ZMUX_SOCKET_MODE` at startup:

| Mode | Authentication | Permitted operations |
| --- | --- | --- |
| `same_user` (default) | Same UID | Reads and mutations |
| `read_only` | Same UID | Inspection only |
| `password` | Same UID plus password | Reads and mutations |
| `password_read_only` | Same UID plus password | Inspection only |
| `disabled` | No requests admitted | None |

Unknown or malformed modes fail startup. Password modes require a configured credential;
they cannot silently fall back to same-user authentication. These are local socket
settings, not macOS managed preferences or cloud/relay permissions.

The CLI resolves its password from leading `--password TEXT`, then
`ZMUX_SOCKET_PASSWORD`, then `~/.config/zmux/socket-password`. An absolute
`ZMUX_SOCKET_PASSWORD_FILE` selects another saved file. The native server uses the
same environment/file precedence without the CLI flag. A saved file must be a regular
file owned by the current user with no group/other permission bits (`0600` or stricter).
Symlinks, oversized files and invalid UTF-8 are rejected. Passwords contain 1–1024 UTF-8
bytes without control characters; a single trailing newline in a file is accepted.

Offline commands do not read saved credentials or connect to a socket. Passwords are
redacted from debug output and authentication errors. Child PTYs remove
`ZMUX_SOCKET_PASSWORD` from their inherited environment; configure a private saved file
when CLI commands inside terminals need password-mode access. This credential store is
a private local file; Keychain integration and a native settings editor remain open.

## Policy changes and scope

The server's `set_policy` API changes access without replacing its socket. Admissions
carry a generation that the workspace executor checks before mutation. Replacement
revokes queued old admissions; a mutation already completed can still return success.
The API may wait briefly for an executing model mutation and must run off the UI thread.

Read-only access cannot create, close, focus, resize, send input, alter attention or invoke
generic actions. It can inspect screens and workspace/notification metadata. Remote
relay origins are denied; no generic forwarding of the local socket is supported.

This policy owner does not terminate resources already launched. Managed-device locks,
live resource revocation, native settings controls and object-scoped remote authorization
remain separate implementation gates. A CLI timeout has an unknown outcome and is never
retried automatically.
