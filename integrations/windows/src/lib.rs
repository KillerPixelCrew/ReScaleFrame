// SPDX-License-Identifier: MIT
#[cfg(windows)]
mod digest;
#[cfg(windows)]
pub mod minhook;
mod window_input;
#[cfg(windows)]
mod xinput_facts;

#[cfg(windows)]
pub use digest::sha256_file;

pub use window_input::classify_window_message;
#[cfg(windows)]
pub use window_input::rsf_win32_input_kind;
