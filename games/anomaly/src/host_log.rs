// SPDX-License-Identifier: MIT
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{CStr, CString, c_void};

#[derive(Clone, Copy, Default)]
pub(crate) struct Log {
    pub(crate) callback: rsf_game_log_fn,
    pub(crate) user: usize,
}

impl Log {
    pub(crate) fn text(self, text: &CStr) {
        if let Some(callback) = self.callback {
            unsafe { callback(self.user as *mut c_void, text.as_ptr()) };
        }
    }

    pub(crate) fn say(self, text: String) {
        if self.callback.is_some()
            && let Ok(text) = CString::new(text)
        {
            self.text(&text);
        }
    }
}
