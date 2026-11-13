# Native selection metadata input repair

## Finding and repair

Universal workflow run `43371105829`, job `131707842944`, built the app, then
failed the terminal-selection `metadata-compile` phase at source
`2e2c9d3445574b51569a2428bf8962061332165d`. The retained compiler log reports
that `NotificationDelivery.swift` cannot find `ProgramAlertEventGate` and
`ProgramAlertSnapshotMode`.

Both types are defined by the production
[`ProgramAlertDelivery.swift`](../../../../../apps/macos/Sources/ProgramAlertDelivery.swift).
The helper already compiles `NotificationDelivery.swift` with its real source
dependencies, but omitted that owner from `METADATA_INPUTS`. Adding the owner
to that tuple makes the existing frozen-source inventory retain its hash and
causes the existing `swiftc` invocation to compile it in the same module. No
stubs or app behavior changes are involved.

## Reachability and sibling scan

`METADATA_INPUTS` is checked against the frozen source inventory, copied into
the retained `source/` directory, recorded in `metadata_probe_inputs`, and
expanded into the metadata `swiftc` arguments. The new entry therefore flows
through the existing capture, hash, and compile path.

The sibling `tools/native-completed-history/run.py` obtains decoder inputs from
`selection.METADATA_INPUTS` and replaces only the selection contract source with
its own history contract. It will inherit this production source automatically.
The bounded search found no second sibling helper with a separate explicit
`NotificationDelivery.swift` compile list.

## Evidence and limits

- Prior Foundation jobs passed: Ubuntu `763/0/7` and macOS `761/0/7`. The
  retained Ghostty pins passed their ARM test and compile lanes.
- The universal app build passed before this metadata compile failure.
- Diagnostic source: `/tmp/zmux-osc7501-final-2e2c9d3/universal-diagnostics-artifact/extracted/native-terminal-selection-universal/metadata-compile.log`;
  receipt: `/tmp/zmux-osc7501-final-2e2c9d3/universal-diagnostics-artifact/extracted/native-terminal-selection-universal/receipt.json`;
  full job: `/tmp/zmux-osc7501-final-2e2c9d3/universal-failed-job-131707842944-full.log`.
- Local `--check-inputs` and Python syntax validation cover fixture inventory
  and its bounded PTY protocol. They do not run `swiftc`; this Linux host cannot
  establish native macOS compilation. The parent owns the aggregate and fresh
  hosted delivery evidence for this repair.

## Parent acceptance

Parent accepted the single-entry input-list diff. All 229 source hashes in
the accepted `2e2c9d3` retained Ghostty linkage receipt still match the working
tree; this helper change is outside that compiled source closure. Retain that
accepted artifact and recheck its source closure at the new commit rather than
repeat engine/library production. Aggregate validation is running before commit.
