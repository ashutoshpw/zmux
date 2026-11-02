# Claude adapter compatibility

**Updated:** 2026-11-02

## Policy

The local Claude adapter accepts Claude Code patch releases `2.1.301` through
`2.1.316`, inclusive. It rejects other major/minor series and future patches
above `2.1.316` until that range is reviewed. Each executable must also expose
a repeatable `--plugin-dir` option in its `--help` output; the adapter injects
both its lifecycle plugin and the existing session notice plugin with that
option.

The version probe requires one `--version` line in the `x.y.z (Claude Code)`
form. Both probes use the existing bounded child owner and its deadline. Claude
Code 2.1.316 truncates `--help` output when stdout is a pipe before the
`--plugin-dir` declaration, so the help probe writes to a private temporary
file instead. The temporary directory is mode `0700`, the file is mode `0600`,
and `/bin/sh` applies a 64 KiB file-size limit to the child. The existing
process timeout still applies; oversized, non-UTF-8, unsuccessful, or
non-repeatable help is rejected without printing provider diagnostics.

The provider's hashed binary identity is recorded before probing. Metadata is
checked after `--version`, the full binary identity is checked after `--help`,
and metadata is checked again when installation starts. The automatic
launcher retains its launch-once notice-plugin fallback when preflight fails.
It does not inject provider subcommands or change Claude user configuration.

Installation records keep format 1 and persist the detected provider version.
The validator accepts existing `2.1.301` records, and reinstalling the same
`2.1.301` binary remains idempotent without creating a new generation.

## Test-authoring gate

1. **Observable contract:** the shipped CLI may use the session lifecycle
   plugin only for a compatible Claude executable, and its owned installation
   record must contain that executable's actual version.
2. **Credible regressions:** the exact `2.1.301` check rejected `2.1.316`; a
   missing repeatability declaration or executable replacement during help
   must not publish a plugin or launch lifecycle tracking.
3. **Coverage gap:** prior CLI coverage exercised `2.1.301` and an unsupported
   `99.0` fallback, but never proved `2.1.316`, help capability checks, or
   truthful version persistence. The main regression runs the production CLI
   against a synthetic executable and exercises the real automatic lifecycle
   launch and plugin publication boundary.
4. **Production seam:** none added. The synthetic provider is an executable
   fixture consumed by the shipped CLI; no test flag, exported predicate, or
   injected production wrapper was introduced.

## Evidence

- Before the fix, the `2.1.316` automatic-launch regression failed at the CLI
  boundary. The provider started with only the notice plugin and before the
  lifecycle plugin was published (`Claude started before automatic plugin
  publication`). This reproduced the former exact-version rejection.
- After the fix, the focused `provider_hooks` suite passed in an isolated
  source snapshot of `12c5146` with only the Claude-owned implementation,
  integration test, and fixture files overlaid: 11 passed, 1 ignored. The
  package format check passed in that snapshot.
- A built CLI from that same clean snapshot installed and reported the actual
  local Claude Code `2.1.316` executable in a temporary plugin directory, then
  uninstalled it successfully. No model/API turn was run and no provider
  configuration was changed.
- CLI-boundary negative cases reject `2.1.317`, a `2.1.301` help response
  without repeatability, and an executable atomically replaced during help.
  The existing `2.1.301` installation remains idempotent and its record keeps
  `provider_version: "2.1.301"`.
- The shared checkout's later full-target retry is blocked by unrelated dirty
  in-progress provider lifecycle changes in `crates/desktop-runtime` and
  `crates/remote-runtime` that currently do not compile. Those files were left
  untouched; the isolated snapshot check validates this Claude slice against
  the requested `12c5146` baseline.

## Sources and limits

- Claude Code hooks documentation: <https://code.claude.com/docs/en/hooks>
- Claude Code 2.1.316 changelog: <https://code.claude.com/docs/en/changelog#2-1-316>
- Installed local `2.1.316 --help` declares `--plugin-dir <path>` and marks it
  repeatable. No authenticated provider turn was used as evidence.
- The exact endpoints `2.1.301` and `2.1.316` are covered by fixtures, and the
  live local executable was preflighted at `2.1.316`. Intermediate patch
  releases are admitted only inside the closed range and when their own help
  advertises repeatability; each intermediate release was not run separately.
- This work does not add Codex/OpenCode support, remote provider support, UI
  changes, or a downloadable app build.
