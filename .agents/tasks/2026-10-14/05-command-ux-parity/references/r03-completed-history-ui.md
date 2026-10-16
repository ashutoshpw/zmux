# Completed terminal history presentation

The native view renders the bounded last visible workload viewport saved when
completion was authenticated. It is read-only selectable text with explicit exit
status and truncation information. This snapshot does not guarantee all final
output was drained before detach. Session ownership ends only after actual join.

The coordinator retains each view by namespace, surface and receipt digest, and
prunes it once from the complete canonical model. Individual window projections
must not prune views belonging to other windows. Moving a workspace rebinds its
focus callback and preserves the existing text selection and scroll position.

## Validation

The dedicated `completed-history` GitHub Actions suite starts the actual app from
an isolated generated v7 restore document. Its historical receipt is fixture input,
not proof of live supervisor authentication or the Rust no-launch contract.
An independent 64-line Unicode oracle checks exact UTF-8 copying, native Select All,
read-only typing and menu Paste, and retained selection/scroll after an actual
CLI workspace move and source-window closure. Success follows awaited shared
shutdown and verifies the owned launch PID.

Eight-file source freeze `9cb6ee60450e0daff49476852281ce993bf86b4a0d2ee0a1c920657e400ba015`
passed independent source review `cb77bad1128639654fd4c626d94a481056d6185ed6e0c5a2c3925c712fbc668a`.
Native run `38845311766` at `af8073108037bdc15bb73e14f0cdd788c85dc2d8`
passes ARM64 release compilation and the actual five-stage AppKit fixture. All
253 retained source files match their recorded hashes; all eight UI/runner/workflow
files match the main integration sources. Actual app, activation and owned-launch
PID are 7898. All four clipboard checks match exact UTF-8; UTF-16 selection
400/19 and scroll Y=233 survive both the move and source-window closure. The
owned process exits zero after awaited shared shutdown, without timeout or log
truncation. Archive SHA256:
`c01d3a6c6dc4320eebd648c8fa8e6958105127d9f56a92641b15a52cd319de8d`.

Earlier attempts caught Darwin device-ID typing, Swift clipboard type inference,
and missing startup dispatch for the fixture. Those were repaired without changing
the fixture's behavioral assertions. The script-only early C source check is not
part of the retained 253-source inventory or this runtime acceptance.

The restored receipt remains generated test input; live completion authentication
and no-launch restore have their separate Rust acceptance. V2 execution-admission
history decoding/presentation is a subsequent phase. No diagnostic archive is a
full-parity release.
This fixture does not establish full terminal selection or cmux parity.
