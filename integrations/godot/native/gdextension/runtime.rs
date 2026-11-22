// SPDX-License-Identifier: MIT
//! `ReScaleFrameRuntime`: a `RefCounted` with no state of its own. Its methods read the option
//! Dictionary and the RenderingServer names for `extension.rs` and reach the Runtime control.
use super::api::{self, Bind, Godot, StringName, Variant};
use super::sys::*;
use super::{Dictionary, RUNTIME_CLASS, Sink};
use crate::extension::{self, REAL, VARIANT_BOOL, VARIANT_INT};
use crate::platform::{OptionField, OptionValue};
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

/// `VARIANT_OTHER`: any Variant type the checked fields refuse.
const VARIANT_OTHER: u32 = 0;

/// `GDExtensionClassCreateInstance3`. The class has no fields, so the instance is a non-null
/// marker the engine hands back to `free`.
pub(super) unsafe extern "C" fn create(_: *mut c_void, notify_postinitialize: GdBool) -> ObjectPtr {
    let Some(godot) = api::godot() else {
        return std::ptr::null_mut();
    };
    let object = godot.construct(c"RefCounted");
    if object.is_null() {
        return object;
    }
    let class = StringName::new(godot, RUNTIME_CLASS);
    let marker = NonNull::<u8>::dangling().as_ptr().cast();
    unsafe { (godot.interface.object_set_instance)(object, class.ptr(), marker) };
    if notify_postinitialize != 0 {
        godot.postinitialize(object);
    }
    object
}

pub(super) unsafe extern "C" fn free(_: *mut c_void, _: *mut c_void) {}

/// Forward+ on an admitted driver, from the RenderingServer's current names.
fn admission(godot: &'static Godot, rendering: ObjectPtr) -> u32 {
    if rendering.is_null() {
        return 0;
    }
    let name = |method| unsafe { godot.string(method, rendering, &[]) }.map(|name| name.utf8());
    let driver = name(Bind::GET_RENDERING_DRIVER_NAME);
    let method = name(Bind::GET_RENDERING_METHOD);
    extension::admission(
        driver.as_deref(),
        method.as_deref(),
        crate::host::native_platform().vulkan_supported(),
    )
}

/// `configure(options, viewport)`.
///
/// # Safety
/// `options` is a live Dictionary.
pub(super) unsafe fn configure(
    godot: &'static Godot,
    options: ConstTypePtr,
    viewport: u64,
) -> bool {
    if viewport == 0 || options.is_null() {
        return false;
    }
    let admitted = admission(godot, godot.rendering_server());
    let reader = Reader { godot, options };
    unsafe {
        extension::rsf_godot_extension_configure(
            admitted,
            viewport,
            (&raw const reader).cast(),
            read_option,
        ) != 0
    }
}

struct Reader {
    godot: &'static Godot,
    options: ConstTypePtr,
}

/// One option: absent leaves the default, otherwise the Variant type and godot-cpp's
/// `int64_t(Variant)` or `double(Variant)` conversion.
unsafe extern "C" fn read_option(
    reader: *const c_void,
    field: *const OptionField,
    value: *mut OptionValue,
) -> u32 {
    let (Some(reader), Some(field), Some(value)) = (
        unsafe { reader.cast::<Reader>().as_ref() },
        unsafe { field.as_ref() },
        unsafe { value.as_mut() },
    ) else {
        return 0;
    };
    let godot = reader.godot;
    let key = Variant::string(godot, unsafe { CStr::from_ptr(field.name) });
    let Some(raw) = (unsafe { api::dictionary_get(godot, reader.options, &key) }) else {
        return 1;
    };
    value.present = 1;
    value.variant = match unsafe { (godot.interface.variant_get_type)(raw) } {
        TYPE_INT => VARIANT_INT,
        TYPE_BOOL => VARIANT_BOOL,
        _ => VARIANT_OTHER,
    };
    if field.conversion == REAL {
        unsafe { (godot.to_float)((&raw mut value.real).cast(), raw) };
    } else {
        unsafe { (godot.to_int)((&raw mut value.integer).cast(), raw) };
    }
    1
}

/// `get_status()`.
pub(super) fn get_status(godot: &'static Godot) -> Dictionary {
    let mut dictionary = Dictionary::new(godot);
    let rendering = godot.rendering_server();
    let admitted = admission(godot, rendering);
    let mut sink = Sink {
        godot,
        dictionary: &mut dictionary,
        rendering,
    };
    let (user, emit) = sink.emitter();
    unsafe { extension::rsf_godot_extension_status(admitted, user, emit) };
    dictionary
}

pub(super) fn reset_history() {
    crate::rsfg_reset_history();
}

pub(super) fn request_latency_ping() -> bool {
    crate::rsfg_request_latency_ping() != 0
}

/// `request_frame_dump()`: the Runtime capture request runs on the render thread.
pub(super) fn request_frame_dump(godot: &'static Godot) {
    let rendering = godot.rendering_server();
    if rendering.is_null() {
        return;
    }
    let callable = api::Callable::custom(godot, capture, no_arguments);
    unsafe { godot.call(Bind::CALL_ON_RENDER_THREAD, rendering, &[callable.ptr()]) };
}

unsafe extern "C" fn capture(
    _: *mut c_void,
    _: *const *const c_void,
    count: GdInt,
    _: *mut c_void,
    error: *mut CallError,
) {
    let Some(error) = (unsafe { error.as_mut() }) else {
        return;
    };
    if count != 0 {
        *error = CallError {
            error: CALL_ERROR_TOO_MANY_ARGUMENTS,
            argument: 0,
            expected: 0,
        };
        return;
    }
    *error = CallError {
        error: CALL_OK,
        argument: 0,
        expected: 0,
    };
    crate::runtime::rsf_godot_extension_capture();
}

unsafe extern "C" fn no_arguments(_: *mut c_void, valid: *mut GdBool) -> GdInt {
    if let Some(valid) = unsafe { valid.as_mut() } {
        *valid = 1;
    }
    0
}
