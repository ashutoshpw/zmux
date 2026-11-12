# Checkpoint state during natural final drain

The independent checkpoint recheck identified an `Exited` snapshot before
`execute()` committed the attached-child-exit transition. Parent repaired this
small finding by publishing `Running` from the run-loop checkpoint, matching
ordinary frames before final retirement. Completed-history checkpoints still
publish `Exited` after retirement. No drain or deadline semantics changed.

The existing private real-PTY fixture now also runs with its retained child
observed exited before releasing the worker gate. It asserts the checkpoint
has `Running` plus the final working report, then the eventual `Exited`
snapshot has no live record and includes its ChildExit transition. The two
cases share setup and add no production test seam.

Negative control restoring the old early-Exited selection failed at the
intended state assertion: actual Exited, expected Running. Repaired private
checkpoint cases passed 2/2, and strict Session library Clippy passed.
Log: `/tmp/zmux-osc7501-final-drain-state-red.log`.

The final snapshot observation handles the public API's documented Busy
result within the same original deadline and fails on other errors. Combined
Session checks passed 44 with two pre-existing ignores after graceful-close
repair. Final integrated and hosted acceptance remains pending. Bounded process/snapshot observation uses test-owned deadlines;
it makes no guarantee about all bytes written after a read-drain observation.
