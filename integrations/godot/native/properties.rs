// SPDX-License-Identifier: MIT
use crate::extension_abi::*;
use std::ffi::{CStr, c_char, c_void};

pub(crate) struct Properties {
    user: *mut c_void,
    emit: unsafe extern "C" fn(*mut c_void, *const c_char, u32, *const rsf_godot_property),
}

impl Properties {
    /// # Safety
    /// The sink and its user context must remain live for this writer. The sink
    /// can only borrow the payload for each synchronous emission.
    pub(crate) unsafe fn new(user: *mut c_void, emit: rsf_godot_property_emit) -> Option<Self> {
        Some(Self { user, emit: emit? })
    }

    fn emit(&self, name: &CStr, kind: u32, value: rsf_godot_property) {
        unsafe { (self.emit)(self.user, name.as_ptr(), kind, &value) };
    }

    pub(crate) fn integer(&self, name: &CStr, integer: i64) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_INTEGER,
            rsf_godot_property { integer },
        );
    }

    pub(crate) fn boolean(&self, name: &CStr, value: bool) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_BOOL,
            rsf_godot_property {
                integer: value.into(),
            },
        );
    }

    pub(crate) fn real(&self, name: &CStr, real: f64) {
        self.emit(name, RSF_GODOT_PROPERTY_REAL, rsf_godot_property { real });
    }

    pub(crate) fn text(&self, name: &CStr, text: &CStr) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_TEXT,
            rsf_godot_property {
                text: text.as_ptr(),
            },
        );
    }

    pub(crate) fn extent(&self, name: &CStr, extent: [i32; 2]) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_EXTENT,
            rsf_godot_property { extent },
        );
    }

    pub(crate) fn transform(&self, name: &CStr, matrix: &[f64; 12]) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_TRANSFORM,
            rsf_godot_property {
                matrix: matrix.as_ptr(),
            },
        );
    }

    pub(crate) fn projection(&self, name: &CStr, matrix: &[f64; 16]) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_PROJECTION,
            rsf_godot_property {
                matrix: matrix.as_ptr(),
            },
        );
    }

    pub(crate) fn renderer(&self, name: &CStr) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_RENDERER,
            rsf_godot_property { integer: 0 },
        );
    }

    pub(crate) fn driver(&self, name: &CStr) {
        self.emit(
            name,
            RSF_GODOT_PROPERTY_DRIVER,
            rsf_godot_property { integer: 0 },
        );
    }
}
