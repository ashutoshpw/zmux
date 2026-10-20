# Status

- **State:** done
- **Updated:** 2026-10-20

## Notes
- Replacement macOS workflow [39419255495](https://github.com/ashutoshpw/zmux/actions/runs/39419255495)
  passed on pushed source `ddc5373161bd3d0b2f31a63ee3eec528fbc65c3c`; remote parity was confirmed.
  Artifact [12265031238](https://github.com/ashutoshpw/zmux/actions/runs/39419255495/artifacts/12265031238)
  and its machine-readable verification are recorded in
  [artifact-verification.json](references/artifact-verification.json). The archive SHA-256 is
  `007b62ddccbe0c1c7ef54030b2cd5c4419616cde634d7b0c51850021db5fa6d6`; the outer artifact
  SHA-256 is `4eccca166b2e8d827d83d7539c2704e5c3176a5d5896db7fa30f85a4cd802a24`.
- Packaged smoke receipts passed for `ctrl_tab_navigation`, `ctrl_tab_single_tab_no_input`,
  `ctrl_tab_multipane_scoping`, `terminal_tab_shortcut`, `close_tab_shortcut`,
  `reopen_tab_shortcut`, and shared navigation actions. All three packaged app binaries are
  universal (`arm64` and `x86_64`); 340 retained source files and strict code-signature
  verification passed. The app targets macOS 14+, is ad-hoc signed, and is not notarized.
- Native save-dialog/accessibility coverage was user-deferred; the dependent browser-native download
  path remains excluded. No physical user-Mac verification was performed.
- The original hosted run [39417663909](https://github.com/ashutoshpw/zmux/actions/runs/39417663909)
  failed before app packaging in the unrelated CLI `agent_messages` fixture: the producer created
  `wrapper-status` before writing its exit code, while the consumer waited only for file existence.
  The test-only repair publishes the captured status through a temporary file and atomic rename;
  the strict `0` assertion remains. The replacement run above passed.
- Implementation review cleared the shared focused-pane navigation path and native regression. The
  focused test passed all five scenarios, and `RUST_TEST_THREADS=2 scripts/check` exited 0 at
  `/tmp/zmux-ctrl-tab-status-publication-check.log`. The packaged smoke exercises real NSApp
  dispatch, ordering, wraparound, pane scoping, bracket aliases, and a raw PTY input probe.
