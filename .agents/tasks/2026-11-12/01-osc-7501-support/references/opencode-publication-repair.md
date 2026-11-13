# OpenCode fixture diagnostic publication barrier

The universal build at `be8ec91`, run `43349565567`, failed the newly added
`sanitized preflight timing diagnostic` assertion. Full failed logs are retained
at `/tmp/zmux-osc7501-be8-development-full.log`. The test waited for the
provider's calls file, then inspected a rendered terminal snapshot once. File
publication and PTY parsing/viewport publication are independent. The snapshot
can still contain the previous unsupported-mode fallback launch marker while
the new timed-preflight diagnostic is pending.

Parent changed the same existing case to wait for this invocation's complete
`run_standalone_help_preflight` timing field, including its closing delimiter,
within the original eight-second deadline. PID-termination, single fallback,
configuration preservation, lifecycle count, and 1,900–2,700 ms timing assertions
are retained. No production or timeout change was made.

A local experiment at 40 columns with the old one-shot parser still passed, so
it did not reproduce the hosted failure or establish a wrapping defect. That
experimental resize was removed. The final change addresses the source-proven
publication-ordering gap rather than normalizing display output speculatively.

The focused real-Desktop OpenCode case passes locally after the wait change.
The hosted failure is pre-fix evidence; no native post-fix acceptance is claimed.
This is a repair to the existing owner fixture, with no production seam or new
duplicate test. Final aggregate and native checks remain pending.
