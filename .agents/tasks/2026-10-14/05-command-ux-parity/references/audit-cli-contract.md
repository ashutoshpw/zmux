# CLI and shared control contract audit

Compared cmux `3ba653b9e5afb11d0bc578031142950d6136697e` with zmux
`69342a8`. Source observations, 2026-10-15. No reference implementation or help
prose was imported. References below are relative to the named repository.

## Finding

The current zmux CLI is a small, independently versioned local-control client.
It is not a compatible replacement for cmux's CLI. Printing more help would not
repair target resolution, options, output formats, lifecycle or remote behavior.
Even commands with matching names remain partial at the compatibility boundary.

| Contract | cmux source and behavior | zmux evidence and gap | Required acceptance |
| --- | --- | --- | --- |
| Global grammar | `CLI/cmux.swift:6329`: socket, JSON, ID presentation, window override, password; provider argument passthrough bypasses presentation parsing | `apps/cli/src/main.rs:150`: socket and bounded timeout only | Parse globals before/after verbs where supported; preserve provider `--` arguments; invalid or missing values fail before mutation |
| Help/version/offline | `CLI/cmux.swift:6445`, `CLI/CMUXCLI+TaskHelp.swift:84`: grouped task help, leaf help, docs/guide and version before socket connection | `apps/cli/src/main.rs:176`: top/leaf help and version exist; no topic groups, guide/skill or full leaf registry | App stopped: top, topic, leaf help and version return successfully; unknown topics/verbs fail; advertise only executable behavior, explicitly label planned coverage |
| Target identity | `CLI/cmux.swift:10963`, `:11001`, `:11138`, `:11314`, `:11381`: typed handles, UUIDs, indices, tab alias and ownership validation | `apps/cli/src/main.rs:546`: positive integer IDs; no stable UUID or typed-ref resolver | Same object resolves via supported forms; stale IDs, wrong object kind and cross-workspace/window targets reject without side effects |
| Caller defaults | `CLI/cmux.swift:8287` and per-command target resolution: caller workspace/surface defaults interact with explicit window/workspace | CLI parser `:195–419` requires positional IDs for many actions; only local socket env resolution `:556` | Spawned local/remote terminals receive zmux-owned caller context; explicit routing overrides defaults; commands cannot act on another window accidentally |
| Socket selection | `CLI/cmux.swift:6568`: explicit path/env path are pinned; implicit selection can discover live app instance; path open has launch behavior | `apps/cli/src/main.rs:556`: flag, `ZMUX_SOCKET`, platform default; absolute paths required | Explicit override never silently reroutes; default instance discovery, app-not-running, stale socket and tagged-instance errors are deterministic |
| Authentication and remote scope | `CLI/cmux.swift:2983`, `:6977`: explicit password, env, saved file/Keychain; capabilities filter for remote relays at `Sources/TerminalController+Capabilities.swift:7` | `crates/desktop-runtime/src/control.rs:355`: same-UID peer enforcement, private socket directory, request caps; no password mode or remote relay | Preserve same-user protection; add intentional auth modes and credential precedence without leaks; remote scope deny-by-default with object ownership and no local-command escape |
| Output and errors | `CLI/cmux.swift:6993`, `:10826`: refs default, selectable UUID/both, JSON preserves stable IDs for designated listings; errors diagnosed for version skew `:9863` | CLI `:131`: JSON-only local results; `ControlResponse` version/request/error envelope `control.rs:99`; no cmux schema | Stable automation schema, human output, documented exit codes and stderr; distinguish transport, timeout, parse, unsupported, permission and stale-target errors |
| Focus policy | `CLI/cmux.swift:6999`: selected commands route global window via focus; leaf-specific no-focus flags | CLI actions commonly focus as model defaults; no global window/no-focus contract | Table of focus effects for create/open/move/select/notify; UI and CLI share action effects; background operations cannot steal focus |
| Path/open startup | `CLI/cmux.swift:6556`, `:6644`, `:10239`: path detection and explicit-socket route; launch environment scrub `:10314` | Parser unknown-command branch `apps/cli/src/main.rs:431`; no direct path opening/launch | File/directory/URL, spaces, relative paths, nonexistent input, running/stopped app and explicit socket tested; never reinterpret arbitrary unknown verbs as paths |
| Wire/protocol discovery | cmux v1 command bridge plus v2 socket methods; `TerminalController+Capabilities.swift:29` | zmux `control.rs:25,44`: protocol v1, 16 KiB request/1 MiB response, shared Action envelope | Publish zmux's own negotiated capabilities; reject version skew, malformed/oversized requests; do not claim wire compatibility from equivalent UX |
| Registry and aliases | Main/early dispatcher, delegated parsers and task help are separate owners | `apps/cli/src/main.rs:176` hand-written parser/help | One original registry drives help, completion/docs, dispatch metadata and compatibility tests; preserve documented alias equivalence and external passthrough boundaries |
| Liveness/backpressure | cmux has transport/startup/command-specific deadlines, including hooks `CLI/cmux.swift:6903` | zmux bounded socket worker, snapshots and request deadlines `control.rs:25,355,539` | Concurrent requests with output flood, disconnected SSH and stalled consumers: bounded memory, responsive UI, explicit overload/timeout, no duplicate mutation on retry |

## Catalog reconciliation

[audit-source-catalog.json](audit-source-catalog.json) captures identifiers and source
locations from the main CLI switch, early dispatch conditions and advertised RPC
methods, including simulator and DEBUG additions. It is **lexical discovery**, not
a claim that every candidate is a public command or every RPC is available in every
build. DEBUG, feature flags and remote authorization change availability.

The source adds namespaces and compatibility paths absent from the supplied help,
including window/canvas/layout/workspace-group, project, mobile, VPN and billing.
It also contains process/PTY/hook helpers and legacy browser aliases. Domain reports
classify these; internal helper names must not be blindly added to user help.
External provider wrappers accept an external tool's grammar: pin the dependency
version and test argument forwarding rather than inventing a finite list of its verbs.
Dynamic indices, selectors, settings keys and numeric zoom values are domains, not
an infinite command count. Generic RPC requires the separate method catalog.

The original 428 help rows and 171 shortcut rows are historical discovery ledgers.
Current audit reconciliation adds a source-family owner for each; family review does
not mean each option combination, menu route or runtime failure has been executed.
Keep future behavior evidence on the individual row/contract when implemented.

## Delivery sequence

1. Define original typed identities, caller context, action/result/error and focus
   contracts in Rust. Preserve positive-integer migration compatibility deliberately.
2. Build the shared command registry, offline help and complete parser contracts.
   Generate compatibility cases from real command definitions, not help-only stubs.
3. Route implemented commands through shared actions; retain explicit unsupported
   errors for planned commands. Complete managed SSH and transfer as one journey.
4. Each family must pass normal, invalid-target, lifecycle, no-focus, disconnection
   and output-schema checks through every public entrypoint before parity is closed.

The audit adds no executable command. Full CLI parity remains open.
