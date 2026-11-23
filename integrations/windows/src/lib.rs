// SPDX-License-Identifier: MIT
#[cfg(windows)]
mod digest;
#[cfg(windows)]
pub mod minhook;
mod window_input;
// The XInput facts are C exports (xinput_facts.h) for AC7's native platform table, which links them
// from the rsf_windows_input_facts archive. Built into every crate that uses this one, a binary with
// two such Rust archives would define them twice.
#[cfg(all(windows, feature = "xinput-facts"))]
mod xinput_facts;

#[cfg(windows)]
pub use digest::{sha256_file, sha256_file_hex, sha256_hex};

pub use window_input::classify_window_message;
#[cfg(windows)]
pub use window_input::rsf_win32_input_kind;
