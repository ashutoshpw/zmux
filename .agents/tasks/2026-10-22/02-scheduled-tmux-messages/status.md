# Status

- **State:** planned
- **Updated:** 2026-10-22
- **Authorization:** Planning documents only; implementation is not authorized.

## Notes
- Persistent remote scheduling is the selected direction. Linux with a systemd user
  service is a proposed first platform; compatibility and linger requirements remain
  implementation decisions to verify.
- Only planning documents have changed for this task. No application/help or runtime
  feature code or feature-specific test code has been added; no remote service setup,
  connection, or scheduled job has been attempted.
- Existing portable validation, run at root's request, passed with exit 0:
  `RUST_TEST_THREADS=2 scripts/check`. Log:
  `/tmp/zmux-scheduled-tmux-messages-plan-check.log`. This does not validate the
  proposed feature; no feature-specific tests or code were added.
