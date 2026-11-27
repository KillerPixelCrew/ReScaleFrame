// SPDX-License-Identifier: MIT
//! The texture-dump request records and log sink shared by the capture owner and the leaves.
use std::ffi::{CStr, CString, c_char, c_void};

/// The D3D11 backend log callback (render_abi.h rsf_backend_log_fn).
#[allow(non_camel_case_types)]
pub type rsf_backend_log_fn =
    ::core::option::Option<unsafe extern "C" fn(user: *mut c_void, message: *const c_char)>;

pub const ENCODED_MOTION: u32 = 1;
pub const DECODED_MOTION: u32 = 2;

pub const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

#[repr(C)]
pub struct TextureOptions {
    struct_size: u32,
    pub abi_version: u32,
    pub prefix: *const c_char,
    pub view: u32,
    pub scale: f32,
    pub log: rsf_backend_log_fn,
    pub log_user: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MotionEncoding {
    pub scale: [f32; 2],
    pub bias: [f32; 2],
    pub zero_x_unwritten: u32,
}

#[repr(C)]
pub struct MotionTextureOptions {
    pub base: TextureOptions,
    pub motion: MotionEncoding,
}

impl MotionTextureOptions {
    pub const ABI_VERSION: u32 = 3;

    pub fn new(mut base: TextureOptions, motion: MotionEncoding) -> Self {
        base.struct_size = size_of::<Self>() as u32;
        base.abi_version = Self::ABI_VERSION;
        Self { base, motion }
    }
}

impl TextureOptions {
    pub const ABI_VERSION: u32 = 2;

    pub fn new(prefix: &CStr, view: u32, scale: f32, log: Log) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_version: Self::ABI_VERSION,
            prefix: prefix.as_ptr(),
            view,
            scale,
            log: log.callback,
            log_user: log.user as *mut c_void,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Log {
    pub callback: rsf_backend_log_fn,
    // An opaque caller-supplied token. Rust never dereferences it.
    pub user: usize,
}

impl Log {
    pub fn new(callback: rsf_backend_log_fn, user: *mut c_void) -> Self {
        Self {
            callback,
            user: user as usize,
        }
    }

    pub fn say(self, text: &str) {
        if let Some(callback) = self.callback
            && let Ok(message) = CString::new(text)
        {
            unsafe { callback(self.user as *mut c_void, message.as_ptr()) };
        }
    }
}
