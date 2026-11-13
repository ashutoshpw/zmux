# Browser-link fixture nonblocking snapshot read

The combined local aggregate stopped with 456 passed, 1 failed and 5 ignored
in `explicit_cli_opener_routes_to_the_origin_and_private_events_stay_ephemeral`: the
fixture unwrapped Desktop::snapshot while the publisher held its lock and got
`DesktopError::Busy`. Source fingerprints and unrelated preservation matched.
The failed receipt/log are archived as `/tmp/zmux-osc7501-check-round17.{json,log}`.

Parent changed only the existing fixture's read loop to honor Busy under its
original absolute deadline with the existing two-millisecond cadence. Other
errors remain failures, predicates and diagnostic assertions are unchanged, and
commands are submitted only once. This matches the nonblocking snapshot contract
already handled by other Desktop fixtures. The two focused browser-link tests
pass (0.21 s), with no production changes or disabled assertions.

A first attempt imported the shared fixture reader but triggered an unused
convenience-wrapper warning; that import was removed and the local wait loop
handles Busy directly without suppressing warnings or altering shared helpers.
The corrected aggregate is rerunning on stable source.
