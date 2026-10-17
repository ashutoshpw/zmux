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

## Opening files and web pages

`zmux open [TARGET_OPTIONS] [--focus true|false | --no-focus] [--] PATH_OR_URL...` and
`zmux markdown open [TARGET_OPTIONS] [--focus true|false | --no-focus] [--] PATH...` send one
`targeted` request whose operation is `open_items`:

```json
{"operation": "open_items", "focus": true, "force_mode": null,
 "items": [{"kind": "path", "value": "/abs/README.md"},
           {"kind": "url", "value": "https://example.org/docs"}]}
```

`force_mode` is `null` or `"markdown"` (always `"markdown"` for `markdown open`). The CLI
classifies each operand lexically and makes relative paths absolute against its working
directory (it does not canonicalise and does not expand `~`). One to 16 operands are accepted
and the encoded request must stay within the 16 KiB request bound. A lexical error (empty
operand, control character, path or address over 4096 bytes, a scheme other than `http://` or
`https://`, a URL given to `markdown open`, an unknown option, too many operands or an oversize
request) fails the whole invocation before any connection, so a partly valid command has no side
effects. Selector options (`--window`, `--workspace`, `--pane`, `--surface`/`--tab`,
`--expect-revision`) precede the operands and follow the usual target rules, including
`ZMUX_WORKSPACE_ID` and `ZMUX_SURFACE_ID` defaults; `--` makes later operands literal.

`open_items` is a mutation: read-only and password-read-only sockets refuse it, and remote relay
origins are denied, before any file is examined. The app probes the files off its workspace
executor within a one second budget, then applies the items strictly in order. The default
client timeout for these two commands is 5 seconds; a global `--timeout` overrides it.

The result is the usual mutation receipt (`created_surfaces`, `focused_*`, `model_revision`,
`revision`) plus:

```json
{"ok": false, "items": [
  {"index": 0, "kind": "file", "ok": true, "path": "/abs/README.md", "mode": "markdown",
   "deduplicated": false, "surface": {"surface_id": 9, "surface_uuid": "..."}},
  {"index": 1, "kind": "url", "ok": true, "url": "https://example.org/docs", "surface": {}},
  {"index": 2, "kind": "file", "ok": false, "path": "/abs/src",
   "error": {"code": "is_directory", "message": "..."}}]}
```

`ok` is true only when every item succeeded; opening an already open file is a success with
`deduplicated: true`; no second tab is created and the existing one is selected when focus applies. Per-item error
codes are `invalid_operand`, `unsupported_scheme`, `invalid_url`, `not_found`,
`permission_denied`, `is_directory`, `not_regular_file`, `too_large`, `binary_file`,
`invalid_utf8`, `content_mismatch`, `path_not_utf8`, `io_error`, `probe_timeout`,
`file_worker_unavailable`, `busy`, `file_capacity`, `browser_capacity` and `tab_capacity`.
Messages never contain file content. A stale `--expect-revision`, an unknown target, a policy
refusal or a transport failure is an ordinary control error with no `items`.

The CLI prints the full result on stdout. Exit status 0 means every item succeeded. If any item
failed, stdout still carries the full per-item JSON, stderr says `zmux: N of M items failed` and
the status is 1. A whole-request failure prints `zmux: code: message` and exits 1. Nothing is
retried: after a timeout, inspect `list-pane-surfaces` before opening again.

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
