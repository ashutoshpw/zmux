# Initial hosted acceptance failures

## Native Swift compilation

- Integration source `3b385182e43e66bde01100640b9c6cf213eac048`, macOS
  run `43299857734`, Swift job `131467799355`: ProgramSidebarViews statusStyle
  tuple switch lacked explicit return after a guard. Repaired in `284b00f`.
- Source `284b00fac59bb1fee1e478f500b1bd84ed5e2054`, run `43301472720`:
  strict warnings rejected implicit optional-to-Any coercion in the new Programs
  smoke receipt at App.swift:1253/1255. A narrow serialization cast repair is
  underway. No compiler flags or acceptance assertions are weakened.
- Logs: `/tmp/zmux-osc7501-swift-typecheck.log` and
  `/tmp/zmux-osc7501-swift-typecheck-round2.log`.

## Foundation

Run `43299857515` at integration source: Ubuntu passed, macOS job `131467797030`
failed the real local Codex Desktop route child. The shell reached readiness
and the synthetic daemon reported running; pending-route admission failed and
launch fell back without lifecycle tracking. The child then timed out its
expected tracking result. Thirteen later failures were shared-mutex poisoning.
The control server logged Broken pipe. Investigation distinguishes the 150 ms
existing lifecycle client budget from the new asynchronous worker checkpoint;
this mismatch is evidence to test, not a proven sole hosted cause.
Log: `/tmp/zmux-osc7501-foundation-macos-failure.log`.

## Fresh Ghostty producer

Run `43300002338`, source `3b385182e43e66bde01100640b9c6cf213eac048`,
failed artifact `13980787888`:

- API ZIP SHA-256 `e282d0a8b11a9292ad95dc73e369cc8c17206ce2016be4fd09534842826f264f`,
  4,870,104 bytes; download digest, size and bounded ZIP extraction verified.
- Owned diagnostic root:
  `/tmp/zmux-osc7501-ghostty-failed-43300002338/extracted/terminal-engine-artifacts`.
- Receipt state is failed. ARM library was freshly built, linked and inspected;
  its C ABI executable passed. Rust metadata fixture failed with exit101 at
  metadata-owner.rs:433, unwrapping an invalid metadata normalization result.
- New supplement emits source133/subcommand2 alongside the existing RIS effect.
  The immutable old fixture rejects unknown source133; its legacy expected event
  list also lacks that extra lifecycle record. Versioned harness adaptation is
  under read-only investigation. Historical fixture source and pins stay fixed.
- No Intel slice or successful ARM metadata execution exists in this artifact.
  It is an unaccepted diagnostic candidate, never feature delivery.
