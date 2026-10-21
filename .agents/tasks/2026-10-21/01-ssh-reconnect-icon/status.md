# Status

- **State:** done
- **Updated:** 2026-10-21

## Notes
- Source `2fb5db683be30c7353542d0d4f059863b73e83f2` is pushed to `origin/main`.
  Hosted macOS run [39672577235](https://github.com/ashutoshpw/zmux/actions/runs/39672577235)
  passed, including both Swift source slices, Rust/ABI validation, universal packaging,
  terminal selection/browser links, packaged app smoke, sidebar, and status menu.
- [Artifact 12381162329](https://github.com/ashutoshpw/zmux/actions/runs/39672577235/artifacts/12381162329)
  contains `zmux-dev-universal-2fb5db6.zip`. The outer/embedded source receipts, ZIP checksum,
  three universal binaries, and 341 retained source hashes were independently verified.
  See [artifact-verification.json](references/artifact-verification.json).
- `ssh_reconnect_native_icon` is true: packaged smoke observes the stopped SSH tab icon,
  clicks the real AppKit button, preserves tab identity/order/title/route, observes a new
  session and terminal view, and types into the fresh shell without replaying the startup command.
  The stopped-icon screenshot was visually inspected. Existing Ctrl+Tab/create/close/reopen
  receipts passed; all 41 terminal-selection cases matched and both browser-link HTTP requests arrived.
- Rust owner tests exercise `Worker::action` for live/pending/local/stale rejection, exited
  process replacement, settled launch-failure retry, and the 32-terminal limit. Exit releases
  process capacity while its scrollback worker remains asynchronously retired. File drops
  pin their originating surface/session so old callbacks cannot target a replacement shell.
- Independent production review found no confirmed correctness or security defects.
  The generic completion path is covered separately from the settled launch-failure action test.
- `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 at
  `/tmp/zmux-ssh-reconnect-check.log`. Focused reconnect tests passed (3), and the tracked-close
  Swift contract test passed (1). The first full run exposed that contract's stale command
  allowlist; its repair includes an acknowledged reconnect bridge guard and passed the full rerun.
- macOS 14+, Apple Silicon and Intel; ad-hoc signed, not notarized. Hosted strict codesign passed.
  Save-dialog/accessibility and dependent native-browser-download checks remain user-deferred.
  This does not establish physical user-Mac, two-host, or production-provider verification.
- Implementation is limited to a manual reconnect icon for managed SSH tabs. It starts a fresh
  login using the saved endpoint; automatic reconnect, durable sessions, menus and CLI reconnect
  were not added. Unrelated dirty cloud-origin and profile-lifecycle work was preserved.
