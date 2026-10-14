# Source review and architectural decisions

Read-only comparison on 2026-10-14. cmux SHA:
`3ba653b9e5afb11d0bc578031142950d6136697e`; zmux baseline: `5642b48`.
The user's supplied help is the minimum contract inventory. These observations are
source evidence, not proof from running the user's installed cmux or a complete API audit.

## Findings

| Area | Source inspected | Observed behavior / implication for zmux |
| --- | --- | --- |
| CLI help | cmux `CLI/CMUXCLI+TaskHelp.swift:1`, `:125`, `:190` | Task categories and offline help exist; command-specific contracts need dispatcher inspection. Current source adds session save/open and vm restart beyond the supplied list. |
| ⌘T | cmux `Sources/KeyboardShortcutSettings.swift:662` | New Surface maps to ⌘T. The internally named newTab action maps to new workspace/⌘N at `:461`; distinguish these product concepts. |
| Sidebar | cmux `Sources/KeyboardShortcutSettings.swift:455`, `Sources/Update/UpdateTitlebarAccessory.swift:1140` | Sidebar toggle has a titlebar control and ⌘B shortcut; notifications have a separate neighboring control. |
| Browser types | cmux `Sources/Panels/Panel.swift:7` | Browser and terminal are distinct panel kinds; browser ownership must not be modeled as terminal output. |
| Browser engine | cmux `Sources/Panels/BrowserPanel.swift:3134` | WebKit configuration, profile data stores and retained web views underpin embedded browsing. Public WKWebView is appropriate for zmux's existing native host. |
| Browser lifecycle | cmux `Sources/Panels/BrowserPanelView.swift:1906`, `:5513`; `BrowserPanel.swift:9076` | Host integration handles view replacement and popup delegates. zmux needs its own lifetime/navigation-generation policy, not merely a URL-opening button. |
| Browser profiles | cmux `Sources/Panels/BrowserPanel.swift:339`, `:452` | Per-profile website data stores provide storage isolation. |
| Notifications | cmux `Sources/TerminalNotificationStore.swift:42`, `:1156`, `:1570`, `:2077`, `:2400` | Central unread indexes, admission/delivery, focused read indicators and rebinding across moved surfaces are distinct responsibilities. A running-status dot is insufficient. |
| zmux model | `crates/workspace-core/src/lib.rs` (Pane, Workspace, Action) | One session per leaf, no window model or ordered per-pane surfaces. Requires a model change before real tabs and browser coexistence. |
| zmux native UI | `apps/macos/Sources/App.swift` buildLayout/buildMenus | Fixed 224-point sidebar; workspace and split actions, no ⌘T/surface tab bar or notifications inbox. |
| zmux CLI | `apps/cli/src/main.rs`, `crates/desktop-runtime/src/control.rs` | Small numeric-ID local command set; similarly named commands lack most cmux targeting/options. Mark partial, not equivalent. |
| zmux cloud | `services/control-plane/src/app.ts` | Identity/device registry and native auth exist; VM/relay/remote execution are unavailable. |

Links to pinned cmux source are constructed as
`https://github.com/ashutoshpw/cmux/blob/3ba653b9e5afb11d0bc578031142950d6136697e/<path>#L<line>`.

## Independent implementation boundary

cmux's root LICENSE states GPL-3.0-or-later by default and lists BUSL server directories.
Review was used to identify behavior, platform APIs and architectural responsibilities.
Do not copy/translate source, JS automation scripts, tests, help prose, assets or service
implementation into zmux. The command ledger contains functional identifiers, not code.
Author contracts and tests independently. Apple framework usage does not require copying
cmux adapters; use public API documentation when implementing WKWebView behavior.
This source-informed process is not a strict clean-room claim.

## Browser feasibility decisions

Keep Rust as topology/command owner and AppKit as native host. A new native browser host
owns WKWebView and delegates; Rust owns surface identity, routing, cancellation and events.
The main thread may issue WebKit calls but must never wait synchronously for navigation,
JavaScript, downloads or Rust/network work. Completion events carry surface and generation
IDs so stale callbacks cannot affect a replaced/closed tab.

Use system WebKit for the first browser implementation. Avoid private selectors/KVC for
Inspector toggles. Advanced browser command feasibility must be demonstrated with public
APIs; unresolved compatibility remains a tracked gap rather than a success stub. New
permissions/storage/import behaviors require original scoped implementations and tests.

## Notification interpretation

The user's requirement is attention awareness, not only whether a shell process is running.
Plan separate activity and needs-attention states. Explicit notify, bell, supported terminal
notification sequences and configured completion/agent hooks feed an in-app inbox. Mere
output can show activity without creating endless notifications. Native OS delivery is an
optional effect; the inbox and badges work when macOS permission is denied. Record any
future change to this policy in the task rather than silently conflating activity and unread.
