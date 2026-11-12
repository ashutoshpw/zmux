# Independent completed-slice review

**Date:** 2026-11-12

<t3-thread://v2/1288a998-af43-4dfa-8cb8-2e7ea7b4283e/thread:delegated-task:command%3Amcp%3A12f60eb2-a24b-413a-a7b3-3b2d107cda69%3Adelegate-task%3A415cc3a2-0f14-4244-a1d2-5a4fedb0fe91>

## Verdict

The shared protocol store and raw VT supplement pass source review. The native slice needs fixes before acceptance. I reviewed the completed files against [Protocol revision 0.4](https://superlogical.com/rex/docs/build/program-status); I inspected unfinished desktop code only to confirm the alert DTO’s epoch fields.

## Findings

1. **P2: A completion can be dropped after the next prompt.** [ProgramAlertDelivery.swift:97](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/ProgramAlertDelivery.swift:97) admits an event only when its command epoch matches the terminal’s current epoch. A `done` transition retains the epoch at which it occurred ([program-status lib.rs:483](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/src/lib.rs:483)); the following prompt advances the terminal epoch while leaving `done` intact ([program-status lib.rs:508](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/src/lib.rs:508)). The DTO carries the transition epoch on the event and the latest epoch on the terminal ([desktop projection:604](/data/PROJECTS/ashutoshpw/zmux/crates/desktop-runtime/src/program_status.rs:604), [event:743](/data/PROJECTS/ashutoshpw/zmux/crates/desktop-runtime/src/program_status.rs:743)). If `done` and OSC 133 A arrive before the native poll, the live completion is rejected and its selected sound is lost. Add a same-owner `done`-then-prompt fixture and preserve this live completion while still rejecting genuinely stale events.

2. **P2: The legacy notification smoke fails its own sound-routing check.** [AgentAlertSmoke.swift:201](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/AgentAlertSmoke.swift:201) removes `agent_alert_kind` from a payload that still contains `alert_sound_source: agent_lifecycle` and `alert_sound_kind: needs_user`. The subsequent assertions expect those fields to be absent and routing to be valid, so `validateWireCodec()` throws. `NotificationPermissionSmoke` calls this validation before the program alert smoke ([NotificationPermissionSmoke.swift:8](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/NotificationPermissionSmoke.swift:8)). Build the legacy fixture without all three sound tags.

3. **P3, coverage gap: ID duplicate and invalid-path ordering is not fully guarded by tests.** The parser checks ID limits before storing each pair ([program-status lib.rs:756](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/src/lib.rs:756)), but the tests do not cover an over-limit ID followed by a valid ID, or a syntactically valid pair with an invalid path after a valid ID. Those cases distinguish atomic hard-limit rejection and whole-report invalid-ID handling from malformed-pair skipping. The existing cases are at [protocol.rs:344](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/tests/protocol.rs:344) and [protocol.rs:521](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/tests/protocol.rs:521).

4. **P3, coverage gap: The native event-count bound has no smoke case.** [ProgramAlertDelivery.swift:53](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/ProgramAlertDelivery.swift:53) rejects oversized event arrays, while the smoke currently tests an oversized terminal-owner list at [ProgramAlertDelivery.swift:201](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/ProgramAlertDelivery.swift:201). Add an oversized event snapshot with a stable, matching owner so the test exercises the event-bound guard itself.

## Checks and limits

- `cargo test --locked -p zmux-program-status`: **10 passed**.
- VTE raw OSC tests, run from an unchanged temporary copy to avoid generating a vendored lockfile: **3 passed**.
- Scoped `git diff --check` and Python AST parsing of `scripts/check-native-sidebar`: **passed**.
- Swift/AppKit, `scripts/check-native-sidebar` packaged acceptance, screenshots and audio were not executed. This host is Linux and has no `swiftc`; they remain unverified gates.
- No edits were made. I did not review unfinished terminal/session or desktop integration as a completed slice.

## Reviewed scope and SHA-256

Hashes are for the files at final inspection. The reviewed scope covers workspace wiring, shared protocol source/tests, the vendored VTE package and VT100 hooks, the native Program UI/alert changes, and the sidebar acceptance script.

| File | SHA-256 |
|---|---|
| [Cargo.toml](/data/PROJECTS/ashutoshpw/zmux/Cargo.toml) | `ef72da575ade275fc3cadd4fc76fb49d25bda733107db78c27372af66b15e88a` |
| [Cargo.lock](/data/PROJECTS/ashutoshpw/zmux/Cargo.lock) | `c27b1c60299e4fd49fe45f2ff532141a48f953a89b87d7affb841b1824c2842a` |
| [program-status Cargo.toml](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/Cargo.toml) | `1b004056e30c8cd0bcbd8ae588a0aa14b6ed24990e14a93303f48679e25c0781` |
| [program-status lib.rs](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/src/lib.rs) | `8a55cb138854a0df612fc0a2917fd34196f48f4309c991216249cb4e00e53407` |
| [program-status protocol tests](/data/PROJECTS/ashutoshpw/zmux/crates/program-status/tests/protocol.rs) | `7e6f1a5f08d10f6cc970ef4d963534cf8fec7d44e5fa5761f6d2943cc9c6fce1` |
| [VTE CHANGELOG](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/CHANGELOG.md) | `f5d291cdb0b340c13aca2203c271ade81d45e0b6f2dcedbc442b2a4ee219ca60` |
| [VTE Cargo.toml](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/Cargo.toml) | `06bfdd57dec95f9172d84e867e9fcdd02c46eb5008b5d7e75848d76cbfaa96ef` |
| [VTE Cargo.toml.orig](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/Cargo.toml.orig) | `180c11737a8870eca54a25ee3c1ad3455a814ec35664bf09a68e1293fe2f8809` |
| [VTE Apache license](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/LICENSE-APACHE) | `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a` |
| [VTE MIT license](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/LICENSE-MIT) | `e4c9b06fa850cb9b540a5e400e9f6394cf15efcf4098144de477d1d3dae10150` |
| [VTE README](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/README.md) | `ff6200be737dfb4275b2cfbcd7742a7d7031f159908dd7799925733782341995` |
| [VTE upstream record](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/UPSTREAM.md) | `d78b87c457e70fce45bd77c067436d0fcaca0209f1a29f16c1d978cc87d5b697` |
| [VTE ansi.rs](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/src/ansi.rs) | `2ce81d9282575b0f79d7a127463c177d966d1da13eea32d5ecbc0dc35f392b13` |
| [VTE lib.rs](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/src/lib.rs) | `589958a35b0cd96288fd91bd81b343ebb7e2af44b7816512f2e1224463c25895` |
| [VTE params.rs](/data/PROJECTS/ashutoshpw/zmux/vendor/vte/src/params.rs) | `a305b112e6779d14537fd420aad45722b9cde756aba75153ac8560b7aa7d4c4a` |
| [VT100 upstream record](/data/PROJECTS/ashutoshpw/zmux/vendor/vt100/UPSTREAM.md) | `294f787933ca2c672ae980d68453e37760c1a34bfecc899a3004270afcfc225d` |
| [VT100 callbacks.rs](/data/PROJECTS/ashutoshpw/zmux/vendor/vt100/src/callbacks.rs) | `c1d8e9537f8a20e3c9cbab2b30e424fbfcdb3453345d32fb97f2c4c31751914b` |
| [VT100 lib.rs](/data/PROJECTS/ashutoshpw/zmux/vendor/vt100/src/lib.rs) | `834e38ba830d30bed4143c2207ff2ef06708eae0c2e284871e3403d005b6bc5c` |
| [VT100 perform.rs](/data/PROJECTS/ashutoshpw/zmux/vendor/vt100/src/perform.rs) | `8c0a3ab29245e4d259d03bb63bd0e82540184909b8bbaeb1179d29d1e362d9eb` |
| [ProgramAlertDelivery.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/ProgramAlertDelivery.swift) | `fc1236c06253c0da49624570e429313fbf6dbc8d70e576a87cfe70b9d3bde29d` |
| [ProgramSidebarViews.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/ProgramSidebarViews.swift) | `f8e898732d2ea65e560bb944cba1893680ac52194da838d9c7852a7a58cbf7ef` |
| [Models.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/Models.swift) | `b607322632ab7762c5f79545208739b2ce8e16af49bf6aa98e63064b36c5336c` |
| [NativeBridge.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/NativeBridge.swift) | `5f12cf825199103e37fcb45040a0fdcd047f3e8eea93c51aeaab9d5b429bfe86` |
| [NotificationDelivery.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/NotificationDelivery.swift) | `4ed147168013a811e4680e1fe2252af326013d612618ede621420314ae6178f9` |
| [App.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/App.swift) | `0ec5b5fcade51636133a3d094ebf44e99c45b97d96999832dc229a62eef54a98` |
| [ApplicationCoordinator.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/ApplicationCoordinator.swift) | `1e2026f477813493df5fc5240918ee137ce91e4fca406e54af7811f081d2b3fc` |
| [AgentAlertSmoke.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/AgentAlertSmoke.swift) | `76de45121063d7477c04ee6f2184ff3981d2eca1a016a0ebb51d0a1c1d745fb4` |
| [NotificationPermissionSmoke.swift](/data/PROJECTS/ashutoshpw/zmux/apps/macos/Sources/NotificationPermissionSmoke.swift) | `a972517b8304e0c5f7be94e7b299f7ff8630963a6c12fad62b7046b0c31115f9` |
| [check-native-sidebar](/data/PROJECTS/ashutoshpw/zmux/scripts/check-native-sidebar) | `cef7479b94e548ac7b5d1024183f2041afb5a7f1ddbc13de5c409410abceb9a5` |

**No PR; HEAD `0ac51927315e21830b6c31cb36ceb824730dc5c2` on `main` with a dirty working tree; checks: shared 10/10, VTE 3/3, scoped static checks pass; native/hosted checks unexecuted.**
