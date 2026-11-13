# macOS Claude fallback failure diagnosis

**Date:** 2026-11-12

**Scope:** Read-only diagnosis of universal macOS development run
`43317951167` for source `b11b7ab`. The only file added by this diagnosis is
this reference. Existing source, tests, task records, and the in-progress
provider-state repair were preserved.

## Conclusion

The uncommitted provider-state repair does not establish a fix for this failure.
It extends `Register` and `ProviderObservation`; the tracked Claude notification
path submits an ordinary lifecycle `Event`, which still has a 150 ms CLI bound
while Desktop may hold that action behind a Session checkpoint with a two-second
deadline. The Event is not deadline guarded or cancelled by the current repair.

The strongest specific candidate for the extra `Claude needs attention` fallback
is the tracked main-session `Notification(permission_prompt)` after
`UserPromptSubmit`. That hook maps to `Event::NeedsUser` and should receive an
`AlertCovered` receipt when the lifecycle owner creates its needs-user alert. If
the inner Event request reaches its 150 ms deadline, the adapter maps the error
to `NotHandled`; the notice callback then writes an `attention_silent` fallback.
Because the Desktop checkpoint for Event has no caller deadline or cancellation,
it can still apply the same lifecycle event later and create the lifecycle alert.
This interleaving can produce both notices for one hook.

The fixture's typed-invalid known permission notification is a separate,
intentional fallback: it has no `session_id`, cannot be decoded as a typed Claude
event, and is expected to produce one silent fallback. The failure output's
fallback IDs `6` and `8` are consistent with that fallback plus a fallback from
the tracked permission event, with a lifecycle notification between them. The
hosted log does not associate those notification IDs with hook IDs, so this
mapping remains an inference.

## Hosted evidence

The current task status reports Swift typecheck success and passing Linux
scheduler jobs before the Rust development fixture failure. At
`/tmp/zmux-osc7501-macos-development-failure.log:449-452`, the Desktop control
server logs four `Broken pipe` response writes. At lines 454-455 the child test
fails at `apps/cli/tests/provider_hooks.rs:2069`: it expected one silent
`osc777-inbox` fallback but observed two, with IDs `6` and `8`. The subsequent
provider-hook failures include a poisoned test mutex and are cascades from the
child assertion, not independent primary failures.

Those broken pipes are consistent with callers timing out before their control
responses are written, but the log has no request IDs or hook names. It does not
show which operations timed out, whether any Session checkpoint exceeded 150 ms,
or whether the four broken pipes belong to this fixture's lifecycle Events.
There is no native timing trace or post-repair hosted run in this evidence.

## Exact fixture and owner path

1. The fixture records `fallback_cursor` after its no-prompt phase, then runs
   unreadable Notification input, `auth_success`, a typed-invalid
   `permission_prompt` without `session_id`, `UserPromptSubmit`, a valid tracked
   `permission_prompt`, Stop, `idle_prompt`, and a late explicit Stop
   continuation (`apps/cli/tests/fixtures/claude-provider.rs:204-260`; parent
   cursor and assertion at `apps/cli/tests/provider_hooks.rs:1968-1977,
   2062-2084`). Of these post-cursor events, only the two permission notifications
   can create an `attention_silent` fallback. Stop and `idle_prompt` use the
   separate `stop_silent` fallback kind.
2. The typed-invalid notification fails typed decoding. For a known
   `permission_prompt`, `combined_event` selects the uncertain attention fallback
   on that error and writes it to the session notice FIFO
   (`apps/cli/src/hooks.rs:1062-1089,1110-1121`). This is the one fallback the
   assertion expects.
3. For the valid tracked notification, `ClaudeHookEvent::lifecycle_event` maps
   `permission_prompt` to `Event::NeedsUser`
   (`crates/desktop-runtime/src/agent_lifecycle.rs:521-537`). The adapter builds
   `LifecycleAction::Event` with the exact Zmux-generated `hook_id` and calls
   `lifecycle_operation` (`apps/cli/src/hooks.rs:1550-1609`). That helper caps
   the control request at `LIFECYCLE_BUDGET` (150 ms; `hooks.rs:1181-1207`).
4. If that request fails, `try_lifecycle_event` returns an error and `hook`
   converts it to `HookDisposition::NotHandled` (`hooks.rs:1612-1642,1680-1682`).
   For a valid permission notification, `combined_event` treats that disposition
   as a reason to write `attention_silent` (`hooks.rs:1071-1086,1092-1108`). The
   fallback FIFO record contains only the fallback name, not the lifecycle
   `hook_id` (`hooks.rs:1132-1163`), so this path cannot deduplicate itself
   against a late lifecycle receipt.
5. Desktop checkpoints `Register`, `Event`, `ProviderObservation`, and
   `ProviderRouteStarted` before applying the lifecycle command; Event uses the
   default two-second `PROGRAM_STATUS_CHECKPOINT_TIMEOUT`
   (`crates/desktop-runtime/src/lib.rs:595-629,3047-3092`). The deadline and
   cancellation classification currently includes Register, ProviderObservation,
   and ProviderRouteStarted, but not Event. Consequently an Event checkpoint is
   not expired or cancelled when the CLI's 150 ms request ends; if it later
   completes, Desktop can apply the Event, store its hook receipt, and publish
   the needs-user alert (`lib.rs:2617-2678,3010-3046,3823-3855`; lifecycle
   receipt handling at `agent_lifecycle.rs:1659-1678,1887-1923`).

## Does the current repair resolve it?

The current dirty repair gives authoritative `Register` and
`ProviderObservation` requests an absolute caller deadline capped by the
2.7-second hook event budget, then carries a Desktop deadline and cancellation
through their Session checkpoints. It intentionally leaves `Event` on the
150 ms helper and does not add Event to the Desktop deadline/cancellation path
(`apps/cli/src/hooks.rs:1181-1221,1528-1538`; `hooks/provider.rs:849-880`;
`crates/desktop-runtime/src/lib.rs:614-629,962-995,3031-3046`).

The `Register` change could indirectly affect when the Claude lifecycle
capability becomes available in this fixture. It does not directly close the
tracked notification Event interleaving above, and source review cannot show
whether the change affects the timing in the hosted macOS run. The run itself
used the earlier pushed source, not the dirty repair.

The provider-state repair's delayed transport test proves only that the
registration and observation CLI requests can outlive 150 ms; its real-PTY
checkpoint tests exercise those two state actions. Its ordinary Event transport
control confirms that the generic lifecycle helper remains at 150 ms, but does
not delay a Desktop Event checkpoint (`provider-state-budget-repair.md`,
“Regression evidence” and “Proof limits”). Those tests do not establish this
failure's cause or prove the current repair resolves it.

## Minimal repair recommendation

Keep ordinary Claude lifecycle Events bounded at their existing 150 ms hook
contract; do not expand every Event to the 2.7-second provider-state budget. Add
an Event-specific absolute deadline and cancellation to the Desktop checkpoint
path, with a response margin inside that 150 ms budget. Expiry must cancel the
pending checkpoint and fence the Event before it mutates lifecycle state or
publishes an alert. A timed-out, uncommitted hook can then produce exactly one
silent fallback; a committed hook must return the lifecycle disposition receipt
for that exact `hook_id` and select the lifecycle alert path. Do not retry an
Event after an uncertain response.

Preserve a single terminal disposition per `hook_id`: `AlertCovered`,
`HandledNoAlert`, or a definitive no-commit `NotHandled`. If a response can be
lost after commit, the fallback decision also needs to be correlated with that
same hook ID (or withheld for an ambiguous post-send outcome); the current
unkeyed FIFO fallback cannot distinguish that case. The focused owner regression
should delay a real Session checkpoint beyond the ordinary Event deadline and
assert the chosen no-commit/receipt result, no late lifecycle alert, exactly one
fallback when appropriate, and no retry. This diagnosis did not implement that
repair or run tests.

## Proof limits

The source proves a concrete budget and cancellation gap for checkpointed
ordinary Events and identifies a fixture operation that can take the fallback
branch. The log proves two extra silent attention fallbacks and several broken
pipe response writes, but it does not link any one pipe, fallback ID, or
checkpoint to a specific hook. The 150 ms versus two-second mismatch is a
source-supported hypothesis, not a proven sole root cause. No source or test
changes, Linux tests, aggregate check, workflow action, commit, or push were
performed for this diagnosis.
