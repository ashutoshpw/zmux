// SPDX-License-Identifier: MIT
// Include accepted code unchanged; one Resource/Integrated/Metadata owner only.
#[allow(dead_code)]
mod accepted {
    include!("../terminal-feasibility/integrated.rs");
    include!("../terminal-feasibility/metadata-owner.rs");
    include!("pty-owner.rs");
}
fn main() {
    accepted::pty_main();
}
