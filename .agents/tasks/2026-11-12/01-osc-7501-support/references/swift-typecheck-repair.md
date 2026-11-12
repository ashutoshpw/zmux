# Swift typecheck repair

**Date:** 2026-11-12

Hosted macOS run `43299857734`, job `131467799355`, reported unused tuple
expressions for each `statusStyle` switch case and could not infer the
`.systemOrange` / `.systemRed` bases. The availability guard makes the helper a
multi-statement function, so the following switch needs an explicit result
context.

Changed `switch state` to `return switch state` in
`apps/macos/Sources/ProgramSidebarViews.swift`. The switch still returns the
same symbol and color pair for every state, preserving sidebar rendering.

Local inspection found this was the only matching tuple-case switch in the
macOS Swift sources. `git diff --check` is the local source check. Native Swift
typechecking is unavailable here: this host is Linux and has neither `swiftc`
nor `xcrun`. No tests were run for this syntax-only repair; hosted macOS
typechecking remains the compile verification.

## Follow-up: Programs smoke receipt dictionary

Hosted macOS run `43301472720` passed the `statusStyle` repair, then failed
strict typechecking at `App.swift:1253` and `App.swift:1255`. In the nested
`[String: Any]` record dictionaries, contextual `Any` coercion obscured the
optional `title` and `progress` values before their `??` defaults, producing
implicit-optional warnings and a dead-coalescing diagnostic.

Mapped each present optional to `Any` before applying its existing fallback,
following the nearby receipt pattern `optional.map { $0 as Any } ?? NSNull()`.
The existing `""` title and `0` progress fallbacks remain unchanged, so nil and
present values keep the same JSON behavior. The adjacent receipt fields showed
no additional instances of this warning pattern.

`git diff --check` is the local source check. Native Swift typechecking remains
unavailable on this Linux host (`swiftc` and `xcrun` are absent); no tests were
run for this cast-only repair. The hosted macOS typecheck must verify it.
