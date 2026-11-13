# Native profile lease aggregate diagnostic

The local aggregate completed **763 Rust tests, 0 failed, 7 ignored**, strict
Clippy, license inventory and the general C bridge checks, then failed the
profile-lease C fixture assertion at `profile_leases.c:151`. The assertion
expected a successful shared lease acquisition, but did not print its response.
Failed receipt/log are archived as `/tmp/zmux-osc7501-check-round18.{json,log}`.

Parent added error-response logging before that existing assertion. It does not
retry acquisition, suppress an assertion, alter a production deadline, or change
lease behavior. The focused `scripts/check-native-bridge` then passed both the
general C ABI and profile lease gate; output is retained at
`/tmp/zmux-osc7501-native-lease-diagnostic.log`. The original assertion's response
was not captured, so its cause remains unknown. The focused pass does not prove
a root-cause fix. The full check is rerunning with the diagnostic retained.
