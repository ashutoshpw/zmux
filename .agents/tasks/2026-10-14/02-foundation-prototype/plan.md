# Zmux foundation and portable terminal prototype

**Date:** 2026-10-14
**Task:** 02-foundation-prototype

## Objective
Implement and commit the first independently authored MIT foundation: a Rust workspace,
shared workspace actions, a bounded terminal-session feasibility prototype, and an explicit
native bridge. Provide runnable portable evidence while keeping native macOS gates open.

## Context
- Implements early work from [the project plan](../01-build-zmux/plan.md).
- User authorized implementation and cohesive commits on `main`; no push/deployment requested.
- Linux has Rust 1.95 and C compilers, but no Swift/Xcode/Zig or native macOS UI environment.
- Assume copyright Ashutosh Kumar and Zmux contributors, macOS 14+ initial target,
  and reviewed permissive dependencies. These defaults are documented for later review.
- This prototype is not a full terminal app, production engine selection, or cmux hang fix.

## Approach
Build public Rust action APIs, then isolate PTY/terminal work and exercise the bridge
through real C callers. Record dependency provenance and actual checks with each slice.
Do not add speculative cloud/iOS modules or claim native validation from portable tests.

## Steps
- [x] Commit MIT bootstrap, Rust workspace, original core model and behavioral coverage.
- [x] Commit bounded PTY/terminal prototype with output, resize and cleanup evidence.
- [x] Commit handle-safe native bridge and independent C ABI lifecycle checks.
- [x] Update project status and record native/engine/resource gates still pending.

## Acceptance criteria
- [x] Workspace actions enforce focus, split/close and failed-action atomicity invariants.
- [x] Portable prototype runs a real shell, parses output and shuts down owned processes.
- [x] Explicit limits bound prototype queues and terminal state; timeout paths are observable.
- [x] C callers exercise ownership, invalid handles and bounded bridge data without Rust helpers.
- [x] `cargo fmt`, scoped tests and Clippy pass for each completed code slice.
- [x] Dependency inventory records resolved packages, available notices, and distribution review gaps.
- [x] Completed slices are committed on `main`; native acceptance remains clearly pending.

## References
- [Implementation evidence](references/implementation.md) — authoring gates, commands, and results.
