# Contributing

Read [AGENTS.md](AGENTS.md) and the active task before changing implementation. Submit
original work under MIT and disclose any imported code, assets or substantive references.
Keep required third-party notices and record pinned versions in `docs/dependencies.md`.
Do not use cmux source as a translation or copy template.

Use `scripts/check` for portable validation. Add behavioral coverage at the module that
owns the contract. Native linkage, UI, signing and real-device checks require their own
evidence. Record results, limitations and next gates in the task's implementation notes.

Keep commits moderate in scope: one coherent capability with its relevant tests and docs.
Do not bundle unrelated subsystems or bypass hooks. Avoid logging terminal content,
commands, credentials or connection secrets by default.
