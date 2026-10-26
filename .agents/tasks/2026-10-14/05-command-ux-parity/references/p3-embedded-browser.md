# P3a first slice: embedded WebKit surfaces

## Delivered contracts

- Model v3 discriminates terminal/browser surfaces. Browser surfaces carry no session ID,
  never start a PTY, and use independent four-tab admission. Mixed tabs share select/move/
  rename/close/reopen paths. Browser restore creates a validated descriptor immediately;
  terminal restore still waits for successful real launches. Web content/network failures
  remain visible in the restored browser and can be retried with Reload.
- System WKWebView hosts remain alive across hidden-tab switches. Address entry, back/forward,
  reload/stop, page find, zoom, popups-as-tabs and page process failure/reload use public APIs.
  A globe icon/menu creates tabs; ⌘L focuses the address. Rust owns explicit navigation intent;
  native observations update URL/title without changing its generation or causing reload loops.
- Browser title/attention updates refresh tab labels without rebuilding the terminal/split tree.
  UI focus is routed to the owning surface. A frame-based split host owns child geometry and its accessible draggable divider,
  retains ratios across tab/workspace view rebuilds and discards stale geometry cache entries. Browser views are lazy-created and released on close.
- Only HTTP(S) and about:blank are allowed. No page message handler or privileged socket bridge
  exists. Local/script/credential URLs are rejected, TLS failures use WebKit defaults. ATS
  permits user-selected HTTP web content only; cloud URLSession remains HTTPS-only.
- Each tab currently has private nonpersistent WebKit storage. Unknown MIME/download requests
  are rejected visibly. CLI browser open/navigate drives the same Rust surface model.

## Evidence and limits

`scripts/check` passes on Linux: browser descriptors never allocate PTYs, preserve URL/title on
reopen, have independent admission, reject privileged URLs atomically, and work through the
actual CLI/socket transport. The universal native workflow compiles WebKit and its smoke loads
an actual document, edits/reads a form field, switches tabs and verifies page state is retained.
Hosted native checks and downloaded visual review passed ([receipt](native-delivery.md)). A prior native compile exposed a missing NSImage
symbol-weight argument in the new tab UI; that SDK compatibility fix is included here.

This is the first P3a slice. Persistent/shared profiles, download handling, dialog/permissions,
full browser automation, remote browsing, external-link policy UI, cross-origin fixtures and
resource/process-termination soak remain open. Browser descriptors restore URL/profile-default,
not a serialized back-forward list or page memory after closure. Interactive WebKit focus,
forms/IME, VoiceOver, real HTTP redirects/TLS and tab drag still require native dogfood.

Native visual review includes a balanced-browser-width assertion. A separate public-WebKit
snapshot records page pixels because cached AppKit rendering does not reliably include remote
WebContent layers on runners without WindowServer capture permission.
