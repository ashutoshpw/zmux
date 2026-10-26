# Cloud, platform, and advanced command parity audit

Source-only audit on 2026-10-15. Baselines: cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`; zmux
`69342a8f036037736c6eb9a500e479ac41e32d93` (`main`). This covers the cloud,
VM, account, VPN, mobile, Simulator/iOS, capture, diagnostics, resource,
privilege, distribution, localization, reliability, and tmux-compatibility
families assigned to P7/P8. It does not import or translate upstream code.

## Result

The linked JSON ledger records 23 feature families: 18 missing, 3 partial, and
2 source-verified subsets. No family is fully verified. The two source-verified
subsets are native PKCE/session handling and the owner-scoped device registry;
their focused control-plane tests are useful evidence, but no deployed sign-in,
packaged callback, production database, or remote transport was exercised here.

| Priority | Families | Meaning |
| --- | ---: | --- |
| P0 | 9 | Security, identity, authority, deployment, or release foundation needed before dependent behavior can be trusted. |
| P1 | 12 | Required product behavior with clear upstream owners and no current zmux implementation. |
| P2 | 2 | Compatibility breadth that remains required for final parity but follows the owning platform/runtime work. |

| Status | Families | Meaning |
| --- | ---: | --- |
| source-verified-subset | 2 | A narrow zmux behavior exists and has focused source tests at the pinned SHA; native/deployed evidence is still open. |
| partial | 3 | Some foundation exists, but its user journey or required invariants are incomplete. |
| missing | 18 | No zmux behavioral owner for the audited contract was found. |

The decisive boundary is that zmux currently has an identity/device control-plane
slice, not a cloud terminal product. Its own readiness response reports `vm`,
`relay`, and `email` as unavailable (`services/control-plane/src/app.ts:268`),
the native UI says remote transport is unavailable
(`apps/macos/Sources/CloudSettings.swift:234`), and the CLI dispatcher contains
only local topology, browser, attention, send/read, and prototype commands
(`apps/cli/src/main.rs:23`, `:148`). `doctor` is a local socket/protocol probe
(`apps/cli/src/main.rs:681`); it is not cmux diagnostic parity.

## Evidence boundary and classifications

This audit read source, source tests, documentation, and workflow definitions at
the pinned SHAs. It did not build either app, run a native artifact, authenticate
against a deployed service, provision a VM, connect a relay/VPN, mutate a
database, capture a window, invoke a sudo broker, run a Simulator, or perform a
remote/resource soak. A command name, help line, parser branch, mock response,
or explicit “unavailable” response is not implementation evidence.

- **source-verified-subset** means focused tests exercise a bounded behavior in
  current zmux source. It does not mean native, packaged, hosted, authenticated,
  provider, or production verification.
- **partial** means useful source exists, but the complete upstream contract and
  acceptance journey do not.
- **missing** means no behavioral owner was found in zmux. A differently named
  local primitive is not counted when it lacks the same effects and invariants.
- **fully verified** would require the acceptance layers in `verification.md`:
  contract coverage, shared-owner behavior, public CLI integration, packaged
  native behavior, real remote/cloud dependencies, and measured stability where
  applicable. No row reaches that state.

`cloud` aliases the entire cmux `vm` dispatcher (`CLI/cmux.swift:7161`,
`:21223`). It must therefore track every nested leaf, alias, option, default,
target rule, effect, and error below; it cannot be signed off as a single command.

## Cloud authority, identity, organizations, and billing

| Family | Priority | Zmux result | Source-backed comparison | Required closure |
| --- | --- | --- | --- | --- |
| Deployment configuration and owned origin | P0 | Partial | `config/deployment.json:1` owns `https://zmux.w3dev.app`; `CloudEndpoint` rejects non-HTTPS non-local origins (`apps/macos/Sources/CloudClient.swift:26`). The cloud workflow only packages an image/receipt (`.github/workflows/cloud-package.yml:13`), while `docs/cloud-deployment.md:1` defers host, provider, and database and requires Actions-only deployment. | Validate generated native/web/release inputs, deploy only through Actions to the owned origin, rehearse protected migrations, and inspect the actual destination. Missing services must fail explicitly with no upstream fallback. |
| Native auth and session | P0 | Source-verified subset | Native code creates S256 PKCE, validates exact callback/state, rejects token handoff, bounds responses, uses an ephemeral session, and stores the token in Keychain (`apps/macos/Sources/CloudClient.swift:44`, `:71`, `:105`). The server consumes single-use codes transactionally and binds tokens to live Better Auth sessions (`services/control-plane/src/app.ts:116`, `:136`, `:194`). Focused tests cover state, verifier, replay, revocation, and malformed credentials (`services/control-plane/test/control-plane.test.ts:110`). | Packaged callback tests, concurrent/cancelled login, expiry/revocation, separate macOS/iOS clients and keychains, live Better Auth and PostgreSQL, and deployed origin checks. No raw token may appear in URL, log, or device handoff. |
| Organizations, CLI teams, MFA, mail, and admin invariants | P0 | Partial | `/v1/me` returns Better Auth organizations and tests membership output (`services/control-plane/src/app.ts:320`; test `:244`). Zmux has no CLI auth/team surface. cmux exposes team `list/use/create/members/invite/link/revoke-invite/invitations/accept/decline/remove` (`CLI/CMUXCLI+AuthTeam.swift:6`) plus login/logout/status (`CLI/cmux.swift:7056`) and separate settings account routing (`CLI/CMUXCLI+DocsSettings.swift:800`). PR #1 also requires current member checks, MFA, localized invite/recovery mail, last-admin protection, and transactional admission/deletion. | Implement one authority for native/web/CLI membership; prove removed members and revoked sessions lose access; cover invite lifecycle, role/capacity and last-admin races, MFA and localized mail, and bounded transaction pools on a disposable database. |
| Billing and entitlement enforcement | P0 | Missing | cmux exposes `billing checkout --plan` with `--no-open` (`CLI/cmux.swift:7033`); upgrade/portal behavior is owned by the web billing routes (`web/app/api/billing/checkout/route.ts:145`; `web/app/api/billing/portal/route.ts:60`), not additional top-level CLI verbs. Its VM create/resize paths enforce plan size and shared pool ceilings before the server repeats enforcement (`CLI/CMUXCLI+VMHelp.swift:219`). Zmux has no billing client, Stripe flow, plan model, or VM entitlement path. | Hosted checkout/portal/webhook journeys, durable entitlement reconciliation, idempotency, team ownership, plan/pool admission, pause/delete effects, and failure recovery. A UI plan label cannot authorize resources. |
| Device registry and presence | P0 | Source-verified subset | The control plane lists, registers, heartbeats, and revokes only the authenticated owner's devices (`services/control-plane/src/app.ts:333`, `:351`, `:369`, `:383`). Native UI can register/list/revoke devices (`apps/macos/Sources/CloudSettings.swift:120`, `:184`, `:200`), and tests cover cross-owner isolation and heartbeat. | Add bounded stale-record cleanup/retention, session/device revocation interaction, packaged background heartbeat behavior, multi-device live evidence, and the relay/session identity that makes a registered device operational. Registration alone is not presence or remote transport parity. |

The retained PR #1 matrix remains authoritative for the missing pieces. In
particular, provider/database deferral does not erase MFA/mail, admin invariants,
protected PostgreSQL migrations, session-bound relay tickets, VM/relay/artifact
readiness, distinct iOS identity/callback registration, or owned distribution.

## Complete VM/cloud command surface

The following table expands the upstream umbrella into behavioral groups. Every
row is absent from the zmux CLI and remote runtime at the pinned SHA. cmux routes
socket VM methods through `TerminalController+Capabilities.swift:112` and
`Sources/Cloud/VMClientSocketCommands.swift:8`, where cloud-disabled builds fail
closed and the app delegates to `VMClient`. Control-plane routes and services live
under `web/app/api/vm` and `web/services/vms`; reproducing CLI syntax without an
independent zmux provider/relay owner would be a stub.

| Command leaves and aliases | Options/defaults and behavioral contract | Cmux owner | Zmux state |
| --- | --- | --- | --- |
| `vm|cloud guide`, `--skill`, `prompt`, `skill` | `prompt [--json]`; `--open <claude|codex|opencode|pi>` installs a cloud skill/prompt and may open a local agent terminal. | `CLI/cmux.swift:21223`; `CLI/CMUXCLI+VMHelp.swift:342` | Missing; no cloud skill or remote runtime. |
| `base open`, `base reset` | `--desktop|--base`, `--workspace`, `--window`, `--focus <bool>|--no-focus`, `--detach|-d`; reset accepts `--reason`. Open reuses Base; reset retains the old generation. | `CLI/CMUXCLI+VMHelp.swift:385` | Missing. |
| `new|create`, `ls|list`, `status`, `stats`, `resize`, `agent-updates`, `rename`, `pause`, `resume`, `restart`, `rm` | New: size, agent update channel, name, provider, image, workspace, network policy, focus and detach. Resize is grow-only with `--cpu 1…32`, `--memory 4…64 GiB`, `--disk 4…256 GiB` in 4-GiB steps and plan/pool checks. List/status/stats expose state/resources; lifecycle preserves or destroys durable state according to verb. Global `--json` applies where documented. | `CLI/cmux.swift:21240`; `CLI/CMUXCLI+VMHelp.swift:219`, `:352`; `CLI/CMUXCLI+VMAgentUpdates.swift:6`; `CLI/CMUXCLI+VMTui.swift:989` | Missing. Zmux readiness advertises `vm: unavailable`. |
| `tree`, `self` | `tree [machine|local] [--refresh] [--json]` returns addressable local/cloud workspaces, terminals, ports, desktops, and devices. `self <machine> [path] [--json]` reads owner/machine/peers/integrations through the session without spawning a shell. | `CLI/CMUXCLI+VMTui.swift:807`; `CLI/CMUXCLI+VMSelf.swift:9` | Missing. Local `status`/topology lacks provider/device/session addressing. |
| `workspace new/open/rename/close/rm` | New: `--name`, `--reuse`, `--no-open`, focus policy. Open: `--here`, `--tabs`, local `--workspace`, `--pane` plus direction, focus policy. `close` detaches live terminals; `rm` permanently kills them. | `CLI/CMUXCLI+VMTui.swift:898`, `:1065` | Missing. Zmux local workspaces do not address a remote daemon. |
| `terminal send|write/read|screen/wait/wait-exit/output/close/rename` | Send exact text plus named keys without attaching/focusing; read includes bounded screen/cursor/size in JSON; regex wait defaults 30 s; process wait distinguishes exit/signal/pending; output uses `--after` and `--max-bytes` offsets; close kills; rename updates all clients. | `CLI/CMUXCLI+VMTui.swift:927`, `:1236` | Missing. Local `send-text`/`read-screen` target a local pane and do not provide remote identity, retained output, exit, or focus contracts. |
| `tab rename` | `<machine> <tab-id> <name>` renames one daemon placement, distinct from terminal-wide rename. | `CLI/CMUXCLI+VMTui.swift:955`, `:1396` | Missing. |
| `layout export|get|show`, `layout apply|set` | Export workspace/default focus, optional `--raw`; apply file/stdin/saved layout with `--workspace` or `--name`, `--cwd`, `--open`, validation before mutation, and exit 2 for invalid documents. | `CLI/CMUXCLI+VMLayoutEnv.swift:22`, `:80` | Missing. Zmux close-history layout descriptors are local recovery metadata, not portable remote layout documents. |
| `env set|add`, `env ls|list`, `env rm|remove|unset`, `env path` | Set `KEY=VALUE`, `--from-file`, or stdin over the end-to-end machine link; list hides values unless `--show`; remove by name; persisted file is mode 0600 and inherited by snapshots/templates. | `CLI/CMUXCLI+VMLayoutEnv.swift:50`, `:298` | Missing; especially no secret delivery path independent of control-plane plaintext exec. |
| `network` show, `set`, `add-domain`, `remove-domain`, `add-range`, `remove-range`, `preset|presets` | Modes `full|allowlist|none`, `--dns`, full JSON policy, exact domains, CIDR with port/protocol/note, and preset add/remove. Mutations apply live and preserve cmux control hosts. | `CLI/CMUXCLI+VMNetwork.swift:6`, `:51` | Missing. |
| `domains list|ls`, `zones|custom`, `verify`, `publish`, `access`, `grant`, `ungrant`, `grants`, `rm|remove|delete` | Custom-domain verification; publication by VM/port with hostname, `personal|team|public`, team/org selection and explicit `--yes` for public; per-email grants with optional expiry. | `CLI/CMUXCLI+CloudDomains.swift:13`, `:35` | Missing. |
| `snapshot|checkpoint`, `snapshot ls|list`, `snapshot rm|delete`, `fork`, `restore`, `promote-template` | Named provider checkpoint; list/delete; fork with name/window/workspace/focus/detach; restore with provider/window/focus/detach; promote a template-oriented snapshot. | `CLI/CMUXCLI+VMSnapshots.swift:8`; `CLI/cmux.swift:7695` | Missing. |
| `exec` | `exec [--timeout 1…900] <machine> -- <argv...>` preserves argv, returns remote exit code, and emits structured stdout/stderr/exit code with JSON. | `CLI/CMUXCLI+VMTui.swift:976`; `CLI/cmux.swift:7965` | Missing; no remote execution owner, and local fallback would violate the contract. |
| `run`, `route` | Run routes to an idle/woken/new VM with `--sync`, `--pull`, `--machine`, `--new`, size and timeout; route explains choice using `--cwd`, `--new`, `--provision`, size, JSON without executing. | `CLI/CMUXCLI+VMTransfer.swift:995`, `:1677` | Missing. |
| `agent` | Required agent kind; machine/sync/cwd/name/no-open/focus/remote-workspace/wait/output/timeout/new/size/JSON; detached remote terminal identity and exit/output propagation. | `CLI/CMUXCLI+VMTransfer.swift:1701` | Missing. Local attention does not create or own remote agents. |
| `dev` | Machine and local directory with workspace/layout/command/port/remote path/sync/no-sync/no-open/focus/dry-run/JSON; composes route, transfer, layout, browser and open behavior. | `CLI/CMUXCLI+VMDev.swift:17` | Missing and dependent on several missing owners. |
| `push|upload`, `pull|download`, `wait` | Recursive SCP-style transfer, repeated excludes/default excludes, watch/interval; secret upload uses the encrypted machine link and mode 0600; pull remote path; wait timeout and optional wake. | `CLI/CMUXCLI+VMTransfer.swift:30`, `:62`, `:76` | Missing. Local `send-text` is not file transfer. |
| `shell|attach`, `tui`, `desktop|vnc`, `open|port`, `ssh`, `ssh-info` | Window/workspace/focus targeting; managed session/open reuse; private HTTPS port URL with `--print`; desktop browser split; SSH details and managed workspace. | `CLI/cmux.swift:21223`; `CLI/CMUXCLI+VMTui.swift:63`, `:864` | Missing. Registration UI explicitly reports no remote transport. |
| `ports`, `tools`, `handoff` | Inspect remote listeners/tools and print an attach handoff block; JSON where documented. | `CLI/CMUXCLI+VMHelp.swift:376`; `CLI/cmux.swift:8046` | Missing. |

Required VM acceptance is an authenticated deployed journey, not a parser test:
provider creation/list/status/resize/lifecycle, plan and ownership failures,
session-bound expiring relay tickets, remote execution identity on a second host,
reconnect and daemon restart, bounded output/backpressure, transfer integrity and
cancellation, network/domain isolation, snapshot/fork/restore, and explicit proof
that failed remote requests never execute locally. Image, relay, and artifact
readiness must remain disabled until independently verified owned artifacts exist.

## VPN, mobile, Simulator, and iOS

| Family | Cmux contract | Zmux result | Required acceptance |
| --- | --- | --- | --- |
| System VPN | `vpn up|on`, `down|off`, `status`, `revoke`; uses `vm.tunnel_*`, separates the user-space terminal tunnel from the system route, requires the signed Network Extension and user approval, and has no sudo fallback (`CLI/CMUXCLI+VPN.swift:9`; `CLI/CMUXCLI+VPNAppManaged.swift:19`). | Missing: no Network Extension target, entitlement, tunnel API, UI approval flow, or CLI namespace. | Signed packaged app/extension, install/approval/denial, reconnect, revoke, sleep/wake, route and DNS policy, other-app reachability, and proof terminals do not depend on the system tunnel. |
| Mobile compatibility settings | `mobile set-font <points> [--surface] [--workspace]`; `compatible-tags list/set/add/remove/clear` (`CLI/cmux.swift:8136`). | Missing. | Shared settings ownership, target validation, persistence/sync, malformed/range cases, and paired mobile observation. |
| Mobile/iOS client | cmux has iOS packages and distinct mobile identity; PR #1 requires separate macOS/iOS bundles, keychains, exact channel callbacks, and concurrent/cancelled login isolation. | Missing: no iOS app target, mobile identity, callback registration, remote terminal client, release lane, or mobile reliability evidence. | Physical/simulator install, auth, device enrollment, background/foreground reconnect, terminal input/output/resize, revoke, upgrade compatibility, and distinct signed identities. |
| `simulator` namespace | `type`, `tap`, `gesture`, `multitouch`, `swipe`, `button`, `rotate`, `ca`, `memory-warning`, `event-log`, `tools`, `camera`, `permissions`, `ui`, `accessibility|ax`, `foreground`, `targets`, `attach`, `send`, `highlight`, `release`; commands target `--surface` and wait for correlated worker/inspector results (`CLI/CMUXCLI+Simulator.swift:32`). | Missing. | Real Simulator worker and Web Inspector sessions; correlation/cancel/timeout, malformed events, permission reset/grant/revoke, accessibility bounds, target churn, and cleanup. |
| `ios` namespace | Accepts every Simulator leaf and adds `list [--workspace]`, `context [--udid]`, `select <UDID>`, and `screenshot [--out]` for one or `--all` up to eight targets (`CLI/CMUXCLI+Simulator.swift:74`). | Missing. | Exact device/pane binding, stale UDID/target handling, multi-device screenshot naming and byte checks, selection ownership, and packaged macOS-to-Simulator evidence. |

Simulator option subcontracts also remain open: type and raw send accept positional,
stdin, or file input; gestures carry bounded ordered touch events; camera owns
configure/switch/mirror/status; permissions owns list/grant/revoke/reset; UI owns
status/get/set; the agent bridge has its own dispatch (`CLI/CMUXCLI+SimulatorAgent.swift:13`)
and settings validation (`CLI/CMUXCLI+SimulatorSettings.swift:13`).

## Diagnostics, inspection, capture, privilege, and compatibility

| Family | Cmux leaves/options and effect | Zmux result | Required closure |
| --- | --- | --- | --- |
| Diagnostics and raw RPC | `ping`, `iroh-diag`, `version`, `capabilities`, `rpc <method> [json]`, `socket-status`, `debug-terminals`, `trigger-flash`, `set-app-focus`, `simulate-app-active`, `simulate-sidebar-drag` (`CLI/cmux.swift:21080`, `:21446`, `:22664`, `:23311`). | Missing except narrow local `doctor`. | Typed capabilities, bounded diagnostics, honest implementation-specific transport labels, exact exit/output schemas, and authenticated test-only gating for simulation/debug methods. Raw RPC must not bypass method authorization. |
| Resource inspection | `top` targets all/window/workspace, process trees, CPU/memory/process sorting, flat/tree/TSV/JSON, and WebKit PID attribution; `memory` separates app physical footprint from recursively grouped terminal child RSS (`CLI/cmux.swift:22414`, `:22445`). | Missing. Local `status` exposes counts/state, not process attribution or budgets. | Named-machine measurements, PID de-duplication/ownership, exited/racing processes, WebKit lifecycle, output schema, and p95 input/main-loop, CPU, RSS, cleanup and soak budgets. |
| Window capture and recording | `shot|screenshot` supports png/jpg, scale, width, quality, region, output, label, caption and window. `record start/stop/status/note/list` supports mp4/gif, fps, bounded duration, scale/width/region/output/label/captions/window and single-recording ownership (`CLI/CMUXCLI+Screenshot.swift:4`; `CLI/CMUXCLI+Record.swift:4`). App handlers capture only owned windows without Screen Recording permission (`Sources/TerminalController+WindowScreenshotMethod.swift:7`; `Sources/TerminalController+WindowRecording.swift:7`). | Missing. Existing workflow screenshots are test harness artifacts, not public capture commands. | Packaged capture of normal/region/retina states, byte/format/dimension checks, output collision/errors, duration auto-stop, annotation timing, cancellation/close, and permission behavior. |
| Privileged execution | `sudo run [-r reason] [-t timeout] (-c command|script|-)`, `pending`, `setup-touch-id` (`CLI/CMUXCLI+TaskHelp.swift:424`). cmux uses a signed helper/broker and enforces requester identity (`CLI/CMUXCLI+Sudo.swift:6`; `Packages/macOS/CmuxSudoBroker/Sources/CmuxSudoBroker/SudoCLICommand.swift:90`). | Missing. | Signed helper installation/update/removal, code-sign identity and caller checks, explicit reason/timeout, stdin/script bounds, cancellation, Touch ID setup/denial, audit/redaction, and no shell interpolation. |
| Tmux compatibility | Implemented families: `capture-pane`, `resize-pane`, `pipe-pane`, `wait-for`, `swap-pane`, `break-pane`, `join-pane`, `next-window`, `previous-window`, `last-window`, `last-pane`, `find-window`, `clear-history`, `set-hook`, `set-buffer`, `list-buffers`, `paste-buffer`, `respawn-pane`, `display-message` with documented target/focus/timeout/buffer options (`CLI/cmux.swift:22764`; dispatch around `:30637`). `popup`, `bind-key`, `unbind-key`, and `copy-mode` deliberately fail as unsupported placeholders (`CLI/cmux.swift:22905`). | Missing. Zmux local actions do not expose this contract. | Share topology/text/focus owners with native/CLI actions; exact bytes and bracketed paste; buffer bounds; hook lifecycle; timeout/exit semantics; command execution policy. Preserve honest unsupported failures for the four placeholders until real behavior exists. |

Remote socket parity has an additional security gate. cmux's
`RemoteRelayCommandPolicy` denies by default, scopes selectors to the session's
owned objects, and rejects command-bearing parameters
(`Packages/macOS/CmuxRemoteWorkspace/Sources/CmuxRemoteWorkspace/Relay/RemoteRelayCommandPolicy.swift:9`,
`:20`, `:25`, `:268`). A zmux relay must define owned-object mappings and policy
tests before exposing any generic method. Terminal spawn/respawn cannot be
allowlisted until a live two-host test proves execution occurs remotely.

## Distribution, update, installation, localization, and reliability

| Family | Zmux result | Gap and acceptance gate |
| --- | --- | --- |
| Distribution/update/CLI installation | Partial | `scripts/build-macos-dev:38`, `:79`, `:91` builds a universal ad hoc-signed development app; `.github/workflows/macos-development.yml:27` publishes a development artifact. There is no notarized production identity, channel manifest, updater, rollback, or iOS distribution. Native CLI installation only creates a symlink and refuses overwrite (`apps/macos/Sources/CloudSettings.swift:258`). Require signed/notarized channel artifacts, update provenance/signature and rollback, correct bundle/callback/channel metadata, Intel/Apple Silicon runtime evidence, safe CLI upgrade/uninstall and moved-app behavior. |
| Feedback, welcome, onboarding, and update UX | Missing | `cmux feedback` opens the shared modal or submits `--email`, `--body`, and repeatable `--image` through the app pipeline, preserving caller-workspace activation behavior and requiring review before agent-submitted external content (`CLI/cmux.swift:10410`, `:21548`). `cmux welcome` is a first-launch, one-shot default-shortcut surface (`CLI/cmux.swift:6499`, `:21463`; `Sources/TabManager.swift:1467`). Update checks are owned by `CmuxUpdater` and shared menu/sidebar actions (`Sources/ContentView.swift:26`, `:7958`, `:15979`); permission onboarding uses one coordinator (`Sources/AppDelegate+ComputerUseOnboarding.swift:18`). No equivalent zmux feedback submission, first-launch/onboarding owner, updater, or release-note/check flow was found. Require draft/open versus external-submit separation, explicit user approval for sending, attachment validation/redaction, retry/idempotency, one-shot welcome persistence, permission denial/re-entry, shared update action, signed metadata, and release-note/version transitions. |
| Localization | Missing | Zmux user-facing Swift/web/CLI text is English literals and no string catalogs or localization workflow were found. cmux uses catalog-backed strings and CI catalog/default/structure/lint checks (`Resources/Localizable.xcstrings`; `.github/workflows/localization-catalog.yml:1`; `.github/workflows/ci-guards.yml:302`). Require catalogs for native, CLI, service mail/pages and extension metadata; plural/placeholder validation; locale fallback; layout/pseudolocale review; localized MFA/invite/recovery mail. |
| Reliability and resource gates | Missing | Zmux has focused local model/control-plane tests, including output-flood cancellation/leader reaping and saturated-input backpressure without blocked cancellation (`crates/session-runtime/tests/lifecycle.rs:60`, `:114`), plus a development artifact receipt. It still lacks approved numeric Mac budgets, measured multi-session output fairness, a WebKit process budget, long remote sessions, provider lifecycle, VPN sleep/wake, mobile reconnect, and deployed control-plane soak. Require the P0 measurement contract: named hardware/build/workload, idle/1/8-session CPU and RSS, p95 keystroke/main-loop delay, worker cleanup, browser process lifecycle, remote backpressure/reconnect/daemon restart, and multi-hour authenticated soak. |

The current packaging and cloud workflows must not be combined into a release
claim: one packages an undeployed control-plane image/receipt, and the other
publishes an ad hoc development app. Neither proves `zmux.w3dev.app` is serving
the pinned control plane or that the downloaded app completed live sign-in.

## Dependencies and closure order

1. Freeze the registry rows and target/output/error contracts represented in the
   linked JSON; add drift checks for CLI help, dispatch, socket methods, and UI.
2. Finish owned deployment configuration, protected PostgreSQL migration rehearsal,
   native identity, membership/MFA/mail/admin invariants, billing entitlements,
   and production signing/channel identity.
3. Select and implement an independent provider, image pipeline, relay, ticket
   authority, and artifact-readiness model. Keep unavailable capabilities disabled.
4. Implement VM lifecycle and session ownership before nested workspace/terminal,
   execution, transfer, layout/env, network/domain, and convenience composition.
5. Add VPN, mobile, Simulator/iOS, capture, sudo, diagnostics, resource inspection,
   and tmux compatibility through shared owners, with the security gates above.
6. Run packaged native, disposable-database, deployed-service, provider, two-host,
   permission, localization, update, and measured reliability acceptance. Rows
   remain open until their dependency-specific evidence is attached to the exact SHA.

Provider/database selection is an explicit external dependency for deployed VM
journeys, not a reason to mark the commands implemented. Actions-only deployment
to `https://zmux.w3dev.app` remains required. Ordinary local commands and the
existing identity/device subset can continue independently, but cannot be used as
evidence for VM, relay, VPN, mobile, Simulator, capture, sudo, billing, or release
behavior.
