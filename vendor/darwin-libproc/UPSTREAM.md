# Vendored darwin-libproc

Upstream: https://github.com/heim-rs/darwin-libproc
Registry version: 0.2.0, MIT OR Apache-2.0. Original license texts retained.
Verified registry archive SHA256:
`cc629b7cf42586fee31dae31f9ab73fa5ff5f0170016aa61be5fcbc12a90c516`.

Only source change: normalized Cargo.toml memchr requirement changes from `~2.3`
to `^2.3`, allowing the workspace's locked memchr2.8.3. The only called memchr API
in upstream source is `memchr(u8, &[u8]) -> Option<usize>`, which retains that API.
No Rust source, public SDK types, FFI or exact returned-size checks were changed.
This patch avoids an otherwise irresolvable conflict with vte's memchr^2.7.4.

The old experimental dependency requires native ABI/runtime acceptance. Repository
code calls only the reviewed task_all_info and pid_path wrappers. Do not use this
package's generic/ancillary APIs without separate review.
