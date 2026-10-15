# Browser, content, and customization parity audit

Compared cmux `3ba653b9e5afb11d0bc578031142950d6136697e` with zmux
`69342a8f036037736c6eb9a500e479ac41e32d93`. This is a source audit frozen on
2026-10-15. Paths below are relative to the named repository. The companion
[`audit-browser-content.json`](audit-browser-content.json) is the structured
feature ledger with source locations, gaps, acceptance criteria, priorities, and
dependencies.

## Evidence boundary

This audit traces declarations to handlers and state owners. It does not infer
behavior from file names or help text alone. cmux CLI leaves were followed from
`CLI/cmux.swift:18781-20577` and the main dispatcher at `CLI/cmux.swift:9821`,
through the v2 router at `Sources/TerminalController.swift:1951-1973` and
`Sources/TerminalController.swift:3599-3655`, to the browser worker and error
paths at `Sources/TerminalController.swift:10519-10586` and
`Sources/TerminalController.swift:11957-12015`.

The result is 45 feature records: **35 missing**, **6 partial**, and **4 verified
subsets**. By implementation priority, there are **17 P0**, **23 P1**, and **5
P2** records. `verified-subset` means the cited zmux behavior is present in
source; it does not mean cmux parity.

No runtime command, native visual journey, permission prompt, network journey,
or resource soak was run. Existing zmux native-smoke source is evidence that a
test exists, not a fresh passing result. Public WebKit constraints are reported
separately from zmux implementation gaps.

## Current zmux subset

Zmux currently has a public `WKWebView` surface with a separate nonpersistent
website data store per tab. Its CLI exposes only `browser open PANE_ID URL` and
`browser navigate|goto SURFACE_ID URL`. Native chrome supports address entry,
back, forward, reload/stop, find, and zoom. Windowless WebKit navigation creates
a sibling browser tab, and close/reopen retains the observed URL and title.
URL parsing accepts HTTP(S) and `about:blank`, rejects credentials and other
schemes, and prefixes host-like input with `https://`.

The native smoke source loads real WebKit content, captures one snapshot, and
checks hidden-tab form state retention. The WebKit navigation delegate explicitly
cancels download navigations and unknown MIME responses. There is no profile
registry, persistent browsing data, dialog/permission owner, automation socket
surface, content viewer, global search, settings model, command palette, custom
or right sidebar, terminal import, theme selection, or configurable shortcut
registry.

## Feature findings

The following table mirrors every JSON record. “Close when” is the leading
acceptance condition; the JSON ledger contains the complete condition set and
source-to-owner evidence.

| ID | Feature | Status / priority | Principal gap | Close when |
| --- | --- | --- | --- | --- |
| BC-01 | Browser creation, routing, URL policy, open aliases | verified-subset / P0 | Only `open PANE_ID URL` and `navigate|goto SURFACE_ID URL`; no typed routing, split/new, focus, profile, availability, or external policy | Every UI/CLI/popup/restore path uses one creation action with typed errors |
| BC-02 | Chrome navigation, reload/stop, address focus | verified-subset / P0 | Basic UI exists; no hard reload, CLI navigation leaves, shared dispatcher, or configurable bindings | Menu, toolbar, shortcut, palette, and CLI share one owner and state model |
| BC-03 | Omnibar search, suggestions, link routing | partial / P1 | Host prefixing exists; search engines, suggestions, terminal interception, host policy, hover URL, and insecure-host prompt do not | URL/search resolution and routing policy apply consistently to every entrypoint |
| BC-04 | Tabs, popups, moves, close/reopen, placement | verified-subset / P0 | Popup-to-tab and URL/title reopen exist; tab CLI, workspace move, history/profile restore, and dialog ownership do not | Stable descriptors preserve placement, profile, supported history, and focus |
| BC-05 | Lifecycle, memory budget, process recovery | verified-subset / P0 | Hidden views persist; termination only shows text; no budget/timer discard or recovery state machine | Measured memory policy and generation-safe restore pass lifecycle soak cases |
| BC-06 | Persistent profiles and isolation | partial / P0 | Per-tab ephemeral stores are isolated only while alive; no named persistent profile owner or CLI | Profiles persist and bind atomically across open, popup, and restore |
| BC-07 | Browser data import | missing / P1 | No UI or CLI import, source/profile selection, domain filter, preview, or timeout contract | Cookies import into an explicit profile with warnings, progress, cancellation, and counts |
| BC-08 | Downloads | missing / P0 | Zmux cancels downloads; no destination policy, progress, history, stable ID, list, or wait | `WKDownload` has safe destinations, bounded history, and visible failure/cancel states |
| BC-09 | Dialogs, permissions, auth, certificates, trust | missing / P0 | No page-owned dialogs, permissions, HTTP auth, client certificates, SSL bypass, or insecure-HTTP UX | UI and CLI respond to the correct page without blocking actors or leaking credentials |
| BC-10 | Cookies, storage, state save/load | missing / P1 | No cookie/storage operations or versioned state file contract | Origin/profile-scoped operations and atomic state files have explicit scope errors |
| BA-01 | Snapshot, eval, get, predicates | missing / P0 | No DOM automation, element references, typed output, or JavaScript error contract | Explicit surface/frame/selector resolution returns bounded structured output |
| BA-02 | Semantic locators | missing / P0 | No role/text/label/placeholder/alt/title/testid/first/last/nth locators | Every locator has exact semantics and generation-scoped reusable references |
| BA-03 | Pointer, form, selection, highlight, scroll | missing / P0 | No click/form/selection/highlight/scroll operations or `--snapshot-after` | Operations share resolved context and return native-focus/action errors |
| BA-04 | Keyboard automation and waits | missing / P0 | No key replay or selector/text/URL/load/function waits | Waits cancel on timeout/navigation/close and trusted input is never silently downgraded |
| BA-05 | File input and browser file drop | missing / P0 | No `set-input-files`, limit checks, access lifetime, remote distinction, or drop owner | Picker/drop/CLI share regular-file checks and explicit transfer semantics |
| BA-06 | Browser screenshot | partial / P1 | Test-only snapshot exists; no user/CLI action, artifact lifecycle, dimensions, crop, or errors | Explicit surfaces capture without focus mutation and write bounded atomic artifacts |
| BA-07 | Frame and dialog automation | missing / P1 | No frame context or accept/dismiss state; cross-origin behavior absent | Frame/dialog state is surface and document-generation scoped with `not_supported` where needed |
| BA-08 | Devtools, JavaScript console, console/errors | missing / P1 | No inspector lifecycle, console window/list/clear, error capture, or bounded history | Inspector and logs survive the supported lifecycle with explicit unavailable states |
| BA-09 | Focus mode, design mode, React Grab | missing / P2 | No mode owners, aliases, return-surface behavior, or cleanup | UI, shortcut, palette, and CLI share a surface-scoped controller |
| BA-10 | Page zoom and viewport | partial / P1 | UI zoom 0.5–3.0 exists; no CLI/default persistence or exact viewport emulation | Per-tab/default zoom and viewport geometry have validated ranges and restore behavior |
| BA-11 | REPL, MCP bridge, scripts/styles | missing / P2 | No sessions, guide/list/reset/MCP, scripts, styles, bounded output, timeout, or cleanup | Isolated sessions have explicit target, timeout, output, reset, and idle expiry |
| BA-12 | Public-WebKit-limited automation leaves | missing / P1 | No geo/offline/trace/network/screencast/raw-input command surface or honest errors | Every leaf exists and returns stable capability-specific `not_supported` |
| BC-11 | Remote and mobile browser surfaces | missing / P2 | No remote browser identity, stream/input protocol, reconnect mask, or host-execution proof | Local/remote identity is explicit and reconnect/input paths are bounded and authenticated |
| CV-01 | Markdown open/render/live reload | missing / P1 | No Markdown panel, CLI shorthand, file links, watcher, or unavailable-file state | Resolved files open in a real non-PTY panel and reload safely |
| CV-02 | Markdown edit/find/typography/zoom | missing / P1 | No text mode, save/conflict, rendered find, copy modes, typography, zoom, or persistence | Save detects conflicts and settings/default changes have defined panel semantics |
| CV-03 | Diff inputs/layout/navigation/sidecar | missing / P1 | No patch/stdin/git sources, trusted sidecar, layouts, file/hunk navigation, or empty/error states | Inputs render without shell injection and trust is repo/session scoped |
| CV-04 | Diff comments/viewed/submission | missing / P1 | No review store, viewed state, comment bridge, handoff, rollback, or filtering | Comments persist per canonical repo and one-shot handoff restores on failure |
| CV-05 | File preview and live reload | missing / P1 | No text/PDF/image/media/Quick Look surface or generation-safe reload | File type/size selects an explicit mode and close releases all resources |
| CV-06 | Text editor/viewer settings | missing / P1 | No atomic save, wrap, syntax, line UI, tab width, encoding/conflict handling, or zoom | Shared actions save atomically and apply validated editor settings |
| CV-07 | File explorer | missing / P0 | No local/remote tree, search, mutations, preview/editor routing, drag, or path insertion | Exact-root providers use generation-safe refresh and ownership-aware path transfer |
| CV-08 | Global search | missing / P1 | No bounded cross-surface index, snippets, activation route, inline find, or menubar search | Results index bounded live content and activate the exact stable target |
| CU-01 | JSONC settings, validation, reload | missing / P0 | Only `sidebarCollapsed` is stored in UserDefaults; no schema/store/watcher/error owner | Complete documents validate before atomic write and reload with path-specific errors |
| CU-02 | Settings window/search/CLI targets | missing / P1 | No window, searchable sections, CLI target aliases, paths, docs, or shortcut launcher | Every canonical target/alias routes predictably and offline operations stay offline |
| CU-03 | Browser settings | missing / P0 | All search, theme, zoom, memory, download, routing, security, import-hint, and React Grab keys are absent | Every schema key validates and has one tested runtime owner |
| CU-04 | Viewer and file-routing settings | missing / P1 | Markdown, editor, explorer, diff layout, preferred editor, and in-app routing keys absent | Settings apply live with documented defaults and explicit unsupported-key errors |
| CU-05 | Configurable shortcut action registry | partial / P0 | Fixed menu equivalents exist for a subset; no bindings/chords/unbind/conditions registry | All enumerated actions route to shared owners with visible conflicts |
| CU-06 | Shortcut settings UI | missing / P1 | No recorder, search, preset preview, conflicts, all-shortcuts view, or scoped reset | Recording cannot steal active content input and reset/presets preserve unrelated state |
| CU-07 | Themes | missing / P1 | Dark appearance and palette are hard-coded; no discovery/preview/set/clear | Preview rolls back and applied light/dark choices reach all surfaces |
| CU-08 | Terminal settings import | missing / P2 | No supported-source discovery, dry run, preview, confirmation, or conversion report | Parsers never execute source config and write only owned files atomically |
| CU-09 | Config-defined actions/layouts/buttons | missing / P1 | No global/project merge, action registry, workspace layouts, tab buttons, plus menu, or palette contributions | Every configured entrypoint invokes one typed action with explicit focus/errors |
| CU-10 | Custom sidebars | missing / P2 | No templates/try/new/validate/reload/select/open, discovery, renderer, last-good state, or bounded actions | Validation precedes selection and hot reload retains the last working render |
| CU-11 | Right sidebar | missing / P1 | No visibility/mode/focus model, panes, aliases, persistence, or open-as-pane | Window/workspace routing and alias normalization report canonical state |
| CU-12 | Command palette | missing / P0 | No palette, contributions, ranking, disabled reasons, settings toggles, or focus restore | Built-in/configured actions execute through shared owners with deterministic focus |
| CU-13 | Left sidebar customization | partial / P1 | Fixed 224-point collapsible sidebar; no resize, focus, search, appearance, metadata, or schema | Schema-backed appearance/search remains coherent with selection, unread, and drag state |
| CU-14 | Open/file routing and editor | missing / P1 | No mixed file/directory/URL router, preferred editor, preview/Markdown rules, or aggregate errors | Each input is classified safely with per-item result and stable created handle |

## CLI reconciliation

The original 428-row command inventory is a historical discovery ledger. The
following mapping accounts for every original row touched by this domain audit.
Rows marked “adjacent” remain owned by the broader CLI or another domain audit;
they are listed so their omission here cannot be mistaken for parity.

### Non-browser rows

| Original row | Mapping | Current zmux result |
| --- | --- | --- |
| CMD-006 `open` | CU-14, with BC-01/CV-01/CV-05 routing | missing |
| CMD-082 `diff` | CV-03/CV-04 | missing |
| CMD-113 `sidebar-state` | CU-13 plus general inspection contract | missing |
| CMD-114 `markdown open` | CV-01/CV-02 | missing |
| CMD-115 `guide`; CMD-116 `--skill`; CMD-117 `welcome` | adjacent offline help/onboarding contract | not counted among the 45 features |
| CMD-118 `docs settings` | CU-01/CU-02 | missing |
| CMD-119 `docs shortcuts` | CU-05/CU-06 | missing |
| CMD-120 `docs api` | adjacent socket/API documentation contract | not counted among the 45 features |
| CMD-121 `docs browser` | BC/BA family documentation | missing |
| CMD-122 `docs capture` | BA-06 plus adjacent window/pane capture | browser portion missing |
| CMD-123 `docs agents` | adjacent agent domain | not counted among the 45 features |
| CMD-124 `docs dock` | CU-11 mode documentation plus adjacent dock domain | sidebar portion missing |
| CMD-125 `docs sidebars` | CU-10/CU-11/CU-13 | missing |
| CMD-126 `settings open`; CMD-127 `settings path`; CMD-128 `settings docs`; CMD-129 `settings <target>` | CU-02 | missing |
| CMD-130 `config doctor`; CMD-131 `config check`; CMD-132 `config validate`; CMD-133 `config path`; CMD-134 `config paths`; CMD-135 `config docs`; CMD-136 `config documentation`; CMD-137 `config reload` | CU-01 | missing |
| CMD-138 `shortcuts` | CU-05/CU-06 | missing |
| CMD-139 `feedback`; CMD-140 `feed tui`; CMD-141 `feed clear` | adjacent feedback/feed domain; feed is also a CU-11 mode name | not counted as sidebar implementation |
| CMD-142 `themes list`; CMD-143 `themes set`; CMD-144 `themes clear` | CU-07 | missing |
| CMD-145 `import` | CU-08 | missing |
| CMD-146 `reload-config` | CU-01 | missing |
| CMD-147 `right-sidebar toggle`; CMD-148 `right-sidebar show`; CMD-149 `right-sidebar hide`; CMD-150 `right-sidebar focus`; CMD-151 `right-sidebar set`; CMD-152 `right-sidebar mode` | CU-11 | missing |
| CMD-153 `right-sidebar files`; CMD-154 `right-sidebar find`; CMD-155 `right-sidebar vault`; CMD-156 `right-sidebar sessions`; CMD-157 `right-sidebar feed`; CMD-158 `right-sidebar dock`; CMD-159 `right-sidebar cloud`; CMD-160 `right-sidebar devices` | CU-11 | missing |
| CMD-161 `sidebar templates`; CMD-162 `sidebar try`; CMD-163 `sidebar new`; CMD-164 `sidebar validate`; CMD-165 `sidebar reload`; CMD-166 `sidebar select`; CMD-167 `sidebar open` | CU-10 | missing |

`find-window` (CMD-417) is the tmux-compatible window selector; it is not CV-08
global content search.

### Original browser rows

| Original rows and exact commands | Feature | Current zmux result |
| --- | --- | --- |
| CMD-209 `disable-browser`; CMD-210 `enable-browser`; CMD-211 `browser-status`; CMD-212 `browser disable`; CMD-213 `browser enable`; CMD-214 `browser status` | BC-01/CU-03 availability policy | missing |
| CMD-215 `browser open` | BC-01 | verified subset with required numeric pane ID |
| CMD-216 `browser open-split` | BC-01 | missing |
| CMD-217 `browser goto`; CMD-218 `browser navigate` | BC-01 | verified subset with required numeric surface ID |
| CMD-219 `browser back`; CMD-220 `browser forward`; CMD-221 `browser reload` | BC-02 | CLI missing; fixed native UI behavior exists |
| CMD-222 `browser react-grab toggle` | BA-09 | missing |
| CMD-223 `browser devtools toggle`; CMD-224 `browser devtools console` | BA-08 | missing |
| CMD-225 `browser focus-mode enter`; CMD-226 `browser focus-mode exit`; CMD-227 `browser focus-mode toggle` | BA-09 | missing |
| CMD-228 `browser design-mode enable`; CMD-229 `browser design-mode disable`; CMD-230 `browser design-mode toggle`; CMD-231 `browser design-mode status` | BA-09 | missing |
| CMD-232 `browser zoom in`; CMD-233 `browser zoom out`; CMD-234 `browser zoom reset`; CMD-235 `browser zoom <factor>` | BA-10 | CLI missing; fixed native UI zoom exists |
| CMD-236 `browser history clear` | BC-04 | missing |
| CMD-237 `browser url`; CMD-238 `browser get-url` | BA-01 | missing |
| CMD-239 `browser snapshot`; CMD-240 `browser eval` | BA-01 | missing |
| CMD-241 `browser repl`; CMD-242 `browser repl guide` | BA-11 | missing |
| CMD-243 `browser wait` | BA-04 | missing |
| CMD-244 `browser click`; CMD-245 `browser dblclick`; CMD-246 `browser hover`; CMD-247 `browser focus`; CMD-248 `browser check`; CMD-249 `browser uncheck`; CMD-250 `browser scroll-into-view` | BA-03 | missing |
| CMD-251 `browser type`; CMD-252 `browser fill` | BA-03 | missing |
| CMD-253 `browser press`; CMD-254 `browser keydown`; CMD-255 `browser keyup` | BA-04 | missing |
| CMD-256 `browser select`; CMD-257 `browser scroll` | BA-03 | missing |
| CMD-258 `browser screenshot` | BA-06 | missing public action; test-only snapshot source exists |
| CMD-259 `browser get url`; CMD-260 `browser get title`; CMD-261 `browser get text`; CMD-262 `browser get html`; CMD-263 `browser get value`; CMD-264 `browser get attr`; CMD-265 `browser get count`; CMD-266 `browser get box`; CMD-267 `browser get styles` | BA-01 | missing |
| CMD-268 `browser is visible`; CMD-269 `browser is enabled`; CMD-270 `browser is checked` | BA-01 | missing |
| CMD-271 `browser find role`; CMD-272 `browser find text`; CMD-273 `browser find label`; CMD-274 `browser find placeholder`; CMD-275 `browser find alt`; CMD-276 `browser find title`; CMD-277 `browser find testid`; CMD-278 `browser find first`; CMD-279 `browser find last`; CMD-280 `browser find nth` | BA-02 | missing |
| CMD-281 `browser frame` | BA-07 | missing |
| CMD-282 `browser dialog accept`; CMD-283 `browser dialog dismiss` | BA-07/BC-09 | missing |
| CMD-284 `browser download`; CMD-285 `browser download list`; CMD-286 `browser download wait` | BC-08 | missing; downloads are explicitly cancelled |
| CMD-287 `browser profiles list`; CMD-288 `browser profiles add`; CMD-289 `browser profiles rename`; CMD-290 `browser profiles clear`; CMD-291 `browser profiles delete` | BC-06 | missing CLI and persistent owner |
| CMD-292 `browser import` | BC-07 | missing |
| CMD-293 `browser cookies get`; CMD-294 `browser cookies set`; CMD-295 `browser cookies clear` | BC-10 | missing |
| CMD-296 `browser storage local get`; CMD-297 `browser storage local set`; CMD-298 `browser storage local clear`; CMD-299 `browser storage session get`; CMD-300 `browser storage session set`; CMD-301 `browser storage session clear` | BC-10 | missing |
| CMD-302 `browser tab new`; CMD-303 `browser tab list`; CMD-304 `browser tab switch`; CMD-305 `browser tab close`; CMD-306 `browser tab <index>` | BC-04 | missing CLI; generic native tabs are only a subset |
| CMD-307 `browser console list`; CMD-308 `browser console clear`; CMD-309 `browser errors list`; CMD-310 `browser errors clear` | BA-08 | missing |
| CMD-311 `browser highlight` | BA-03 | missing |
| CMD-312 `browser state save`; CMD-313 `browser state load` | BC-10 | missing |
| CMD-314 `browser addinitscript`; CMD-315 `browser addscript`; CMD-316 `browser addstyle` | BA-11 | missing |
| CMD-317 `browser identify` | BC-01 | missing |

### Nested browser grammar absent from the original ledger

The original rows collapse or omit executable leaves. cmux's current help at
`CLI/cmux.swift:23360-23428` and dispatcher require these to be tracked as
contracts rather than treated as new row counts:

- Creation is `open|open-split|new [url]` with `--workspace`, `--window`,
  `--focus <true|false>`, and `--profile <name|uuid>`; `identify` is surface
  optional. Zmux has none of those options and requires its numeric pane for
  `open`.
- Targeted navigation accepts `--surface` or a positional surface. Navigation,
  reload, pointer/form, keyboard, selection, and scrolling leaves accept
  `--snapshot-after` where documented.
- Snapshot options are `--interactive|-i`, `--cursor`, `--compact`,
  `--max-depth`, and `--selector`. Wait includes selector, text, exact/contains
  URL, load state, function, and millisecond/second timeout forms.
- Omitted leaves include `set-input-files` with repeatable `--file` or `--clear`
  (128 files, 32 MiB), the `key` alias, `focus-webview`,
  `is-webview-focused`, and exact `viewport` sizing.
- Get/find families have their own selector, attribute, property, exact-name,
  role, and index options. Download list constrains `--limit` to 1…25; wait
  accepts path and timeout. Cookie flags include name/value/URL/domain/path,
  expiry, secure, HTTP-only, and all-scope behavior.
- Profile compatibility aliases include `ls`, `add|new`, and `remove|rm` in
  addition to the canonical list/create/delete operations.
- REPL includes eval/interactive/guide/list/reset/MCP, named session, workspace,
  timeout, maximum output, eval text, and stdin modes.
- Advanced leaves are `geolocation|geo`, `offline`, `trace start|stop`,
  `network route|unroute|requests`, `screencast start|stop`, and
  `input mouse|keyboard|touch`, plus `input_mouse`, `input_keyboard`, and
  `input_touch` aliases.
- Top-level compatibility shims at `CLI/cmux.swift:9821-9850` are
  `open-browser`, `navigate`, `browser-back`, `browser-forward`,
  `browser-reload`, `get-url`, `focus-webview`, and `is-webview-focused`.
  Legacy `--panel` is rewritten to `--surface` for the targeted shims.

The right-sidebar canonical runtime modes are `files`, `find`, `sessions`,
`feed`, `dock`, `machines`, and `custom-sidebar`. CLI compatibility spellings
also include `vault`, `cloud`, and `devices`; those names must normalize to an
explicit canonical result rather than become extra internal modes. Settings
targets and aliases are enumerated in `CLI/CMUXCLI+DocsSettings.swift:760-834`;
the exact browser keys and enums begin at `web/data/cmux.schema.json:2152`.

## Shortcut reconciliation

These are all shortcut-ledger rows directly owned by the 45 feature records.
Generic workspace/pane/surface movement shortcuts remain in their respective
domain audit even though they may move a browser or viewer. A source enum entry
only proves discovery; status below requires a cited zmux owner.

| UI action rows | Feature | Current zmux result |
| --- | --- | --- |
| UX-ACTION-001 `openSettings` | CU-02 | missing |
| UX-ACTION-003 `reloadConfiguration` | CU-01 | missing |
| UX-ACTION-005 `globalSearch` | CV-08 | missing |
| UX-ACTION-010 `toggleSidebar` | CU-13 | partial: fixed sidebar toggle exists, no configurable action |
| UX-ACTION-011 `focusSidebar` | CU-13 | missing |
| UX-ACTION-015 `newBrowserWorkspace` | BC-01 | missing |
| UX-ACTION-024 `commandPalette`; UX-ACTION-026 `commandPaletteNext`; UX-ACTION-027 `commandPalettePrevious` | CU-12 | missing |
| UX-ACTION-036 `focusRightSidebar`; UX-ACTION-037 `switchRightSidebarToFiles`; UX-ACTION-038 `switchRightSidebarToFind`; UX-ACTION-039 `switchRightSidebarToSessions`; UX-ACTION-040 `switchRightSidebarToFeed`; UX-ACTION-041 `switchRightSidebarToDock`; UX-ACTION-042 `switchRightSidebarToMachines` | CU-11 | missing |
| UX-ACTION-055 `nextSidebarTab`; UX-ACTION-056 `prevSidebarTab` | CU-13/shared navigation | partial generic navigation only; no configurable action owner |
| UX-ACTION-078 `reopenClosedBrowserPanel` | BC-04 | partial URL/title-only reopen behavior |
| UX-ACTION-079 `newSurface` | BC-01/shared surface creation | partial shared creation only; no configurable action owner |
| UX-ACTION-124 `toggleRightSidebar` | CU-11 | missing |
| UX-ACTION-125 `fileExplorerOpenSelection`; UX-ACTION-126 `fileExplorerOpenSelectionFinderAlias` | CV-07 | missing |
| UX-ACTION-127 `saveFilePreview`; UX-ACTION-128 `toggleFileEditorWordWrap` | CV-06 | missing |
| UX-ACTION-129 `openBrowser` | BC-01 | partial browser creation, no compatible workspace/focus/profile action |
| UX-ACTION-130 `focusBrowserAddressBar`; UX-ACTION-131 `browserBack`; UX-ACTION-132 `browserForward`; UX-ACTION-133 `browserReload` | BC-02 | partial fixed native menu/toolbar behavior |
| UX-ACTION-134 `browserHardReload` | BC-02 | missing |
| UX-ACTION-135 `browserZoomIn`; UX-ACTION-136 `browserZoomOut`; UX-ACTION-137 `browserZoomReset` | BA-10 | partial fixed native menu behavior |
| UX-ACTION-138 `markdownZoomIn`; UX-ACTION-139 `markdownZoomOut`; UX-ACTION-140 `markdownZoomReset` | CV-02 | missing |
| UX-ACTION-141 `find` | BC-02/CV-02/CV-06 | partial browser-only fixed find |
| UX-ACTION-142 `findInDirectory`; UX-ACTION-143 `findNext`; UX-ACTION-144 `findPrevious`; UX-ACTION-145 `hideFind`; UX-ACTION-146 `useSelectionForFind` | CV-02/CV-06/CV-07 | missing shared/content owners |
| UX-ACTION-147 `toggleBrowserDeveloperTools`; UX-ACTION-148 `showBrowserJavaScriptConsole` | BA-08 | missing |
| UX-ACTION-149 `toggleBrowserFocusMode`; UX-ACTION-150 `toggleBrowserDesignMode`; UX-ACTION-151 `toggleReactGrab` | BA-09 | missing |
| UX-ACTION-152 `openDiffViewer`; UX-ACTION-153 `diffViewerScrollDown`; UX-ACTION-154 `diffViewerScrollUp`; UX-ACTION-155 `diffViewerScrollHalfPageDown`; UX-ACTION-156 `diffViewerScrollHalfPageUp`; UX-ACTION-157 `diffViewerScrollDownEmacs`; UX-ACTION-158 `diffViewerScrollUpEmacs`; UX-ACTION-159 `diffViewerScrollToBottom`; UX-ACTION-160 `diffViewerScrollToTop`; UX-ACTION-161 `diffViewerOpenFileSearch` | CV-03 | missing |
| UX-ACTION-167 `diffViewerNextFile`; UX-ACTION-168 `diffViewerPreviousFile`; UX-ACTION-169 `diffViewerNextHunk`; UX-ACTION-170 `diffViewerPreviousHunk`; UX-ACTION-171 `diffViewerToggleViewed` | CV-03/CV-04 | missing |

## Public API feasibility

Several advertised cmux leaves are intentionally unavailable on public
`WKWebView`. At the frozen cmux baseline, the handlers return typed
`not_supported` results with capability-specific details:

| Leaf | cmux result and reason |
| --- | --- |
| `browser.geolocation.set` | No per-tab Playwright/CDP-equivalent geolocation spoofing hook |
| `browser.offline.set` | No reliable per-tab offline emulation |
| `browser.trace.start|stop` | Playwright trace artifacts are unavailable |
| `browser.network.route|unroute` | No CDP-style request interception/mocking |
| `browser.network.requests` | No interception log; the error may include recorded unsupported requests |
| `browser.screencast.start|stop` | No CDP screencast stream |
| `browser.input_mouse|input_keyboard|input_touch` | Raw CDP injection unavailable; supported semantic input leaves are named in the error where applicable |
| cross-origin `frame select` | Explicit `not_supported`, not a false success |

Zmux parity for these leaves means exposing the grammar, capability discovery,
and stable honest errors. It does not justify private WebKit API use. Exact
viewport sizing is a separate public implementation path in cmux and must not be
grouped with the unsupported CDP features.

## Owner and failure-path requirements

The cmux router demonstrates distinct owners for main-actor actions, socket
worker automation, VM/profile work, asynchronous dialog completion, and download
waiting. That separation is part of the behavior contract: a browser wait or
blocked JavaScript dialog cannot hold the main actor, and a stale/wrong-kind
surface cannot fall through to a generic success.

Zmux needs an original shared action model before expanding its CLI or shortcut
surface. Each action should resolve a stable typed target, state its focus effect,
run on the appropriate executor, and return specific invalid-target, stale,
unsupported, permission, timeout, cancellation, and lifecycle errors. UI,
shortcut, palette, popup, restore, and CLI entrypoints should call that same
owner. Adding help-only leaves would widen the compatibility claim without
implementing it.

## Remaining evidence gates

Closing source gaps still requires focused runtime evidence on the current SHA:

1. Parser and owner tests for every advertised leaf, alias, option, target form,
   focus effect, output schema, and failure path.
2. Native visual and accessibility checks for browser chrome, settings,
   permissions/dialogs, viewers, palette, and sidebars.
3. Real network, download, file-access, authentication, certificate, and
   permission journeys with cancellation and recovery.
4. Multiwindow and remote ownership tests that prove commands cannot target an
   unrelated object or execute local effects for a remote surface.
5. Browser process termination, hidden-tab memory, media, forms, downloads,
   inspector, close/reopen, and long-running resource soak measurements.

Until those gates pass, the four verified subsets remain source-bounded building
blocks rather than browser/content/customization parity.
