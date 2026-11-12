# Terminal program status with OSC 7501

This page describes Zmux's in-progress source integration for Program Status
Protocol revision 0.4. It is not a claim that a released or verified development
build includes the feature. The existing build receipt in
[development builds](development-builds.md) predates this work.

OSC 7501 lets a process report its own status through the terminal output stream.
Zmux keeps these records per terminal and shows them in a Programs group beside
provider Agents. The reports are self-declared presentation data. They do not
authenticate a process, identify a provider, or prove a provider turn. Provider
adapters remain the authority for provider lifecycle and identity.

## Emit reports

The wire form is `ESC ] 7501 ; key=value:key=value BEL` or the same sequence
terminated by `ESC \`. Fields are separated by colons. These POSIX shell
examples use `printf` to send BEL-terminated sequences through the current PTY:

```sh
printf '\033]7501;state=working:id=build:app=make:progress=25\007'
printf '\033]7501;state=blocked:id=deploy:kind=permission:app=release:progress=0:title=QXBwcm92YWwgcmVxdWlyZWQ=\007'
printf '\033]7501;state=done:id=build:app=make:title=QnVpbGQgZmluaXNoZWQ=\007'
printf '\033]7501;state=error:id=deploy:title=RGVwbG95bWVudCBmYWlsZWQ=\007'
printf '\033]7501;state=clear:id=build\007'
```

The title values decode from base64 as “Approval required”, “Build finished”,
and “Deployment failed”. A report replaces the whole record for its `id`, so
omitted fields are cleared. Repeating a key uses the last syntactically valid
pair before field validation. Unknown keys are ignored and malformed pairs are
skipped. A report with no recognized `state` is ignored.

| Field | Meaning and limits |
| --- | --- |
| `state` | Required: `idle`, `working`, `done`, `blocked`, or `error`. `clear` is a removal operation, not a retained state. |
| `id` | Optional hierarchical id. Omitted means the root record. Up to 128 bytes total, 32 bytes per segment, and 8 levels. Segments use ASCII letters, digits, `_`, `.`, `+`, or `-`, separated by `/`. |
| `kind` | `permission`, `question`, or `auth`; used only with `blocked`. |
| `progress` | Integer from 0 through 100; used only with `working` or `blocked`. Missing or invalid progress is indeterminate. |
| `app` | Self-declared app label using ASCII letters, digits, `_`, `.`, `+`, or `-`; up to 32 bytes. |
| `title` | Optional standard base64 text, padded or unpadded; up to 256 encoded and 192 decoded bytes. |
| `msg` | Optional standard base64 text, padded or unpadded; up to 2,732 encoded and 2,048 decoded bytes. |

The complete OSC sequence, including introducer and terminator, is limited to
4,096 bytes. Keys are limited to 16 bytes. `title` and `msg` must decode as
strict UTF-8 and contain no C0, DEL, or C1 control characters, which keeps them
single-line. Invalid text rejects the whole report. An oversized sequence,
key, id, app, or text field rejects it atomically without changing a record.
Invalid app syntax and progress outside 0 through 100 become absent. An id with
invalid path syntax is ignored without falling back to the root.

`state=clear` removes the addressed record and its descendants. For example,
clearing `build` also clears `build/test`; clearing the root removes all records.
Zmux retains at most 64 records per terminal and evicts the least recently
updated record when a new id arrives at capacity. A child inherits the nearest
currently reported ancestor `app` for display; the inherited value is resolved
from current records and is not copied into the child's stored report.

## Capability query

Producers may optionally send `OSC 7501;? ST`, where `ST` is BEL or `ESC \`.
Zmux replies with a constant list of supported states and blocked kinds. The
reply is ordered before a subsequent primary device-attributes (`CSI c`) reply.
This query confirms protocol support only. It never returns program records and
there is no record-query command.

## Lifetime and transport

Records do not expire by heartbeat or elapsed time. Zmux's integrated Bash,
Zsh, and fish shells emit OSC 133 `A` at prompt boundaries and preserve existing
prompt hooks. That marker retires `working` and `blocked` records from the
closing command. An observed attached-child exit also retires those states after
final output is drained. `idle`, `done`, and `error` remain while the terminal
stays open. RIS clears records; DECSTR and alternate-screen changes preserve
them. Closing the terminal removes them all. Without a delivered prompt marker,
Zmux cannot infer nested-command completion from output text, process scans, or
quiet time.

Reports use the existing terminal byte stream, so local PTYs, managed SSH, and
commands running in a container can carry them when every intermediary forwards
the OSC bytes. This does not establish behavior for any particular tmux, screen,
container, or unmanaged SSH configuration, nor adoption by installed agents. On
SSH loss, retained records can be shown as unavailable last-reported status;
they are not evidence that a program is still running.

## Sidebar and alerts

Programs are separate from provider Agents. Background terminals contribute one
summary record, prioritized `blocked`, `error`, `working`, `done`, then `idle`.
The focused terminal can show its full record list. Selecting a row revalidates
the exact existing workspace, pane, terminal, session, and status epochs before
focusing that terminal. The sidebar labels the source as self-reported OSC
7501; `app`, `title`, and `msg` are display hints only.

For each terminal command epoch, the first eligible external alert selects one
source. A pending or active provider adapter route can own alerts for that
epoch; when OSC 7501 owns the source, provider alerts for the same epoch are
suppressed. This is source arbitration, not cross-protocol event matching. No
message, title, or app-name matching is used. At an observed prompt boundary,
the next command can choose again.

When OSC 7501 owns alerts, entering `blocked` or `error` can raise needs-user
attention. A transition from `working` or `blocked` to `done` can raise a
completion alert. Progress-only updates, identical reports, removals, and
recovery baselines are silent. OSC-only external alerts have a two-second
per-terminal cooldown; it suppresses alerts without delaying record updates,
and suppressed events are counted. Existing provider alerts do not use this
cooldown. Zmux uses the existing local sound preferences and tags OSC-origin
audio separately so the system default sound does not also play. Muting sound
leaves program status and inbox entries available.

Existing OSC 9 and OSC 777 generic notifications keep their own path. They are
not automatically deduplicated against provider or OSC 7501 alerts.

## Build boundary

This integration is still in progress and has no fresh accepted app artifact or
CI receipt. Optional Ghostty 7501 support requires a new versioned source
candidate and fresh libraries accepted for that candidate. Historical Ghostty
roots and build receipts do not prove the 7501 Programs UI. See
[development build evidence](development-builds.md) for the current delivery
receipt and acceptance requirements.

The protocol is specified in the public [Program Status Protocol
revision 0.4](https://superlogical.com/rex/docs/build/program-status).
