// SPDX-License-Identifier: MIT
// Reuse the accepted integrated frame/raw ABI; no second engine owner.
#[allow(dead_code)]
mod integrated {
    include!("integrated.rs");
    include!("metadata-owner.rs");
}
fn main() {
    integrated::metadata_main();
}
