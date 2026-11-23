// SPDX-License-Identifier: MIT
//! The adapter's one host log sink. Every call runs behind the shared foreign-exception guard
//! (`rsf_game_log_guarded`), so a throwing sink cannot unwind into Rust.
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{CStr, c_void};
use std::fmt::Display;

#[derive(Clone, Copy, Default)]
pub(crate) struct Log {
    pub(crate) callback: rsf_game_log_fn,
    pub(crate) user: usize,
}

impl Log {
    pub(crate) fn text(self, text: &CStr) {
        unsafe {
            rsf_game_sys::rsf_game_log_guarded(
                self.callback,
                self.user as *mut c_void,
                text.as_ptr(),
            )
        };
    }

    /// Interior NULs are dropped.
    pub(crate) fn say(self, text: impl Display) {
        if self.callback.is_some() {
            // SAFETY: the host handed over the callback with its user pointer.
            unsafe {
                rsf_game_sys::log_guarded(
                    self.callback,
                    self.user as *mut c_void,
                    &text.to_string(),
                )
            };
        }
    }
}
