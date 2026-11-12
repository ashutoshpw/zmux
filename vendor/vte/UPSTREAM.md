# Vendored VTE parser

Upstream: <https://github.com/alacritty/vte>
Registry version: 0.15.0, Apache-2.0 OR MIT. Source commit recorded by the
crates.io archive: `3b3da71c34cc1256c7e20981cf03f8eb95e08ffc`.
Cargo.lock registry checksum before vendoring:
`a5924018406ce0063cd67f8e008104968b74b563ee1b85dde3ed1f7cb87d3dbd`.
The upstream `LICENSE-APACHE` and `LICENSE-MIT` files are retained verbatim.

The local change in `src/lib.rs` adds a bounded raw OSC callback alongside the
existing normalized `Perform::osc_dispatch` callback. It preserves bytes that
the legacy parser ignores, reports exact body length and BEL / completed
ESC-backslash / cancelled / invalid-ESC termination, and delays only the new raw
callback until the ST byte is present. The existing callback order and parameter
semantics remain available for other OSC consumers.
