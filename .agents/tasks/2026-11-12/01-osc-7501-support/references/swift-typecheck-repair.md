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
