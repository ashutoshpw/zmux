# Zellij stdin interruption repair

2026-10-15. Isolated source producer validated on Linux; no product acquisition
pin, installed executable policy or app distribution change is established.

## Actual failure and boundary

Darwin owner run38805483589 at a87ae754445dcb7fcba2f47f8c4712fd440d41d7
passes bootstrap, original workload input, resize and actual foreground suspension.
Its next input probe after resume fails at local_zellij_owner.rs365. The bounded
terminal transcript reports Zellij0.43.0 stdin_handler panic at
zellij-client/src/os_input_output.rs186:47: stdin.fill_buf().unwrap receives
Os4/Interrupted. The foreground CLI reports child exit1, joined transport workers,
original owner preserved, no rejected/pending input and no requested cancellation.
This is an actual dependency stdin-read failure, not a metadata or broker denial.
Linux passes the same original owner journey.

Pinned MIT source23bb542548d517f7f20a7fdd14a943f1a610df37 confirms this exact
unwrap and its existing signal handling. Open PR searches for EINTR, Interrupted
and fill_buf found none. Upstream issue1429 remains open with related polling and
suspend failures; its old poll failure is distinct from this observed fill_buf
boundary. No fix to an unobserved poll path or upstream version change is included.

## Experimental producer

The independently authored patch retries only ErrorKind::Interrupted at the
existing actual stdin owner. A successful buffer is copied and consumed once;
other errors retain a descriptive fatal failure. No empty input, replay, replaced
signal contract or skipped suspend assertion is used as recovery.

produce-patched.py verifies the immutable source archive, upstream lock and exact
patch, uses explicit Rust1.95.0/no-default-features/vendored_curl build parameters,
and retains hashes/toolchain/host/flags plus upstream MIT license. Existing
pins.json and official downloaded-binary acquisition remain unchanged.

An original standalone example calls the dependency's existing public
get_cli_client_os_input().read_from_stdin; no new product export/helper or mocked
BufRead is introduced. An actual pipe and non-restarting SIGUSR1 handler exercise
kernel interruption. The unpatched owner must fail on actual Interrupted, the
patched owner must deliver literal subsequent bytes once, and a real WouldBlock/EAGAIN
control must still fail before and after the patch. The driver bounds readiness,
output, signal count and child lifetime. A retained process-group owner stops
owned descendants before final reaping, including readiness/capture setup failure.
Build logs are capped at 8 MiB per phase; fixture output at 64 KiB. A retained
subprocess imposes the download's total 120-second deadline independently of
individual reads. Build environment overrides are excluded and effective Cargo
configurations hashed. Upstream prebuilt WASM files are inventoried, not rebuilt.
The complete patched binary must then pass the unchanged actual foreground
suspend/input/detach/reattach owner journey on Linux and Darwin.

Source/caches live under target/agent-validation/zellij-patched-0.43.0 on /data;
receipts and logs survive build-cache cleanup. This producer has no app linkage,
automatic download, install or upstream publication side effect. Immutable binary
artifacts, complete distribution notices, native architecture execution and an
explicit acquisition decision remain separate gates before product selection.

The initial EBADF negative control was rejected during producer validation: Rust
standard stdin maps a closed descriptor to EOF, so it did not exercise an error.
The corrected control uses a real empty nonblocking pipe; no product behavior or
assertion is relaxed. The first real SIGUSR1/EINTR red already reached the exact
upstream fill_buf panic on Linux, independently of the hosted Darwin failure.

The bounded v3 rerun rejected its red prerequisite: Cargo reused the earlier
patched client artifact despite a fresh verified unpatched archive tree, whose
old archive mtimes predated the reusable cache. The producer now explicitly
cleans only its client/root source products in the nominated cache before the
baseline; dependency caches remain reusable. No passing artifact receipt was
issued for v3.

## Linux validation and hosted gate

The final bounded source producer passed at
`target/agent-validation/zellij-patched-0.43.0-v6/receipt.json`, SHA256
`6d3e51e3770a0c395a03c9c8460a65621cf3640a2e7deb1b0e118fa27c0ea1da`.
Its retained `artifacts/zellij` is version 0.43.0, SHA256
`b1c2c13d14e088d2c43601e078d8de5abb5e940a3e75aacb490b5aa472aaba59`.
Actual baseline EINTR exits 101 at the original unwrap; the repaired owner reads
exact bytes after eight real signals. Both actual EAGAIN controls remain fatal.
The upstream lock remains unchanged. Fifteen upstream WASM files, including eleven
current plugin-directory assets, are hashed and explicitly not rebuilt.

The bounded owner helper passed all three existing command groups (27 tests) in a
frozen c8bf10403d5b38f80ea34794843b5e2466aafef3 snapshot, with current producer/helper
tooling overlaid. Receipt:
`target/agent-validation/r03-patched-owner-helper-v7-20261015/receipt.json`, SHA256
`49addaf22eaea78992fa2aab85d40542f9bc4dc6d72bdf409f652cf3227dfc8e`.
The actual foreground journey confirms original identities/startup count one,
input, resize, suspension/resume, subsequent input, TERM/HUP detach, reattach and
termios restoration. This remains a Linux CLI owner result, not native semantic
attachment or Darwin acceptance.

Negative controls reject an oversized preexisting archive before extraction and
a changed producer receipt before Cargo execution. Their receipt is
`target/agent-validation/zellij-producer-negative-controls-20261015/receipt.json`.
Python syntax, manual two-host workflow YAML and repository notice checks pass.

The independent manual `local-zellij-patched-owner.yml` workflow builds source and
executes the unchanged owner gates on Linux and macos-15. It uploads immutable
source inputs/receipts/logs/notices, excluding executables, and uses its own
concurrency group so existing native runs are unaffected. Hosted source builds passed on Linux and Darwin; complete actual Darwin owner
acceptance remains pending as detailed below. Acquisition pins remain
unchanged; complete redistribution notices and native integration remain open.

### First hosted patched-source run

Run 38809114500 at 11671a48ddb484f8730ddf4df43e6a5394d7e886 completed the full
source producer on Linux X64 and macOS 15.7.9 ARM64. Both hosts establish actual
EINTR red/green, fatal non-EINTR controls and full patched executable build.
Uploaded archive, source-input, lock, phase-log, license and WASM hashes were
independently checked against the published commit. Local read-only verifier:
`target/agent-validation/r03-hosted-38809114500/verify-artifacts.py`; source-only
verification receipt `verified-producers-receipt.json` in that directory, SHA256
`3691c504ce7cf22d628fc71ad2a85eaa3581d040043166ec08a3bd58f4b7d9ff`.
No compiled executable was downloaded or uploaded.

Linux passed all 27 owner checks. Darwin passed the earlier failed resume/input
boundary, TERM detach and original owner identity checks, then failed the real
prefix-shadow fixture's bind at local_zellij_owner.rs:413. Its `-shadow` suffix
made the canonical `/private/tmp` socket path 104 bytes, beyond Darwin's Unix
socket capacity; the actual production backend path was 97 bytes. The fixture
now appends one byte to the same complete backend prefix, yielding 98 bytes,
and asserts the existing portable socket budget before binding. The same real
namespace-rejection assertion is retained; production socket policy and lifecycle
behavior are unchanged. Full Darwin owner acceptance requires a hosted recheck.

The first artifact omitted its explicitly named hidden workflow file because of
upload-artifact's default hidden-file filter. The upload step now enables hidden
files within its existing explicit path allowlist, which includes only that
workflow under a hidden directory and no broad hidden-file glob. This does not
include compiled executables or alter the running hosted jobs.
