//! Audited, worker-bound foreign owner for exact hash-accepted Ghostty VT bytes.
//! The `linked` feature requires an explicit retained artifact root at build time.
#[cfg(feature = "linked")]
mod ffi;
#[cfg(feature = "linked")]
mod metadata;
#[cfg(feature = "linked")]
mod owner;
#[cfg(feature = "linked")]
pub use ffi::{RawCell, RawColor, RawFrame};
#[cfg(feature = "linked")]
pub use owner::{Engine, Error, Limits};
