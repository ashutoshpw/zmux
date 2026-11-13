# Final receipt classification repair

Parent addressed both P2 findings in the independent Claude checkpoint Event review.

- A worker Event can commit before the response reserve expires, then return an execution timeout. The control mutation serializer now reports this Timeout using `outcome_unknown`, consistent with the outer Desktop timeout. The CLI independently treats generic post-dispatch errors as Unknown; only enumerated admission failures (authorization/policy, invalid/elapsed deadline, and full queue) establish non-commit and remain fallback eligible. No Event retry is introduced.
- Missing, false or incorrectly typed `accepted` data in a successful control response now yields Unknown. A valid `accepted: true` with explicit `not_handled` retains fallback. No malformed receipt is fabricated into a no-commit result.

The existing production Adapter Unix-socket regression was extended, not duplicated. It now supplies missing/false/string accepted fields, generic post-apply rejection, explicit outcome_unknown, valid NotHandled, a definitive authorization failure, and a lost reply. It asserts the public outcome and fallback policy. These cases protect receipt schema and transport ambiguity contracts that the prior disconnect-only case could not reach, use the actual transport and Adapter callers, and introduce no production seam.

Red/green evidence:

1. `/tmp/zmux-osc7501-malformed-receipt-red.log`: pre-fix response parser returns Receipt(NotHandled) for `accepted: false`; the intended Unknown assertion fails.
2. `/tmp/zmux-osc7501-post-apply-receipt-red.log`: after fixing malformed accepted data, the old generic-rejection path returns a fallback-worthy CliError for command_rejected; the intended outcome assertion fails.
3. `/tmp/zmux-osc7501-review-classification-green.log`: the restored final policy passes the extended production Adapter transport test, including both no-fallback and retained-fallback controls. Workspace formatting and whitespace checks pass.

The post-apply peer models a response whose error cannot establish non-commit. It does not mutate Desktop state or reproduce an actual native descheduling event. The guarantee is conservative classification after dispatch; the existing real-PTY owner tests separately cover mutation, pre-owner cancellation, and normal receipt behavior. No native acceptance is inferred from this transport case. Full aggregate verification is rerunning on stable source before the scoped runtime commit.
