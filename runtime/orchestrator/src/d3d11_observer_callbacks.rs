// SPDX-License-Identifier: MIT
use crate::d3d11_observer_abi::*;
use crate::d3d11_observer_capture;
use crate::d3d11_observer_hooks::installed;
use crate::scene_abi::rsf_observer_present_event as Event;
use crate::sr_session::has_capacity;
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError};

type Handle = *mut c_void;
type Layout = rsf_observer_layout_element;
type Options = rsf_observer_options;

#[repr(C)]
pub(super) struct TextureFacts {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) format: u32,
    pub(super) mip_levels: u32,
    pub(super) array_size: u32,
    pub(super) sample_count: u32,
    pub(super) bind_flags: u32,
    pub(super) misc_flags: u32,
}

#[repr(C)]
pub(super) struct Callbacks {
    log: unsafe extern "C" fn(rsf_dump_log_fn, Handle, *const c_char),
    layout: unsafe extern "C" fn(rsf_observer_layout_fn, Handle, Handle, *const Layout, u32, u32),
    shader: unsafe extern "C" fn(rsf_observer_shader_fn, Handle, Handle, u32, *const c_void, u32),
    texture: unsafe extern "C" fn(rsf_observer_texture_fn, Handle, Handle, &TextureFacts),
    buffer: unsafe extern "C" fn(rsf_observer_buffer_fn, Handle, Handle, *const c_void, u32, u32),
    present: unsafe extern "C" fn(rsf_observer_present_fn, Handle, Handle),
    event: unsafe extern "C" fn(rsf_observer_present_event_fn, Handle, &Event),
}

const _: () = {
    assert!(size_of::<TextureFacts>() == 32);
    assert!(size_of::<Callbacks>() == 7 * size_of::<Handle>());
};

#[derive(Clone, Copy)]
pub(super) struct Registration {
    options: Options,
    calls: &'static Callbacks,
}

// The application supplies its callback context for invocation by creation
// and presenting threads. Rust never dereferences those opaque pointers.
unsafe impl Send for Registration {}

static OPTIONS: Mutex<Option<Registration>> = Mutex::new(None);
static CREATION: Mutex<()> = Mutex::new(());
thread_local! { static IN_CREATION: Cell<bool> = const { Cell::new(false) }; }

struct CreationCall;

impl Drop for CreationCall {
    fn drop(&mut self) {
        IN_CREATION.set(false);
    }
}

pub(super) fn in_creation_callback() -> bool {
    IN_CREATION.get()
}

// Skip nested notifications while the application's creation callback runs.
fn creation(run: impl FnOnce(&Registration)) {
    if in_creation_callback() {
        return;
    }
    let _serialized = CREATION.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(config) = snapshot().filter(|_| installed() != 0) else {
        return;
    };
    IN_CREATION.set(true);
    let _call = CreationCall;
    run(&config);
}

fn lock() -> MutexGuard<'static, Option<Registration>> {
    OPTIONS.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn snapshot() -> Option<Registration> {
    *lock()
}

pub(super) fn clear() {
    let _serialized = CREATION.lock().unwrap_or_else(PoisonError::into_inner);
    *lock() = None;
}

impl Registration {
    fn log(&self, text: *const c_char) -> bool {
        let callback = self.options.log;
        if callback.is_none() {
            return false;
        }
        unsafe { (self.calls.log)(callback, self.options.log_user, text) };
        true
    }

    pub(super) fn say(&self, text: &CStr) -> bool {
        self.log(text.as_ptr())
    }

    pub(super) fn present(&self, chain: Handle) {
        let callback = self.options.on_present;
        if callback.is_some() {
            unsafe { (self.calls.present)(callback, self.options.on_present_user, chain) };
        }
    }

    pub(super) fn event(&self, event: &Event) {
        let callback = self.options.on_present_event;
        if callback.is_some() {
            unsafe { (self.calls.event)(callback, self.options.on_present_event_user, event) };
        }
    }
}

pub(super) unsafe fn validate(options: *const Options) -> i32 {
    if !unsafe { has_capacity(options) } {
        return -1;
    }
    if unsafe { (*options).abi_version } != RSF_OBSERVER_ABI_VERSION {
        return -2;
    }
    0
}

pub(super) fn configure(options: &Options, calls: &'static Callbacks) {
    *lock() = Some(Registration {
        options: *options,
        calls,
    });
    d3d11_observer_capture::configure(options, options.log.map(|_| relay_log as _));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_logging() -> i32 {
    i32::from(snapshot().is_some_and(|config| config.options.log.is_some()))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer11_log(text: *const c_char) {
    if let Some(config) = snapshot() {
        config.log(text);
    }
}

unsafe extern "C" fn relay_log(_: Handle, text: *const c_char) {
    rsf_core_observer11_log(text);
}

// Creation callbacks are serialized, as required by d3d11_observer.h.
pub(super) unsafe fn texture_created(device: Handle, texture: Handle, facts: &TextureFacts) {
    creation(|config| {
        let callback = config.options.on_texture;
        if callback.is_some() {
            unsafe {
                (config.calls.texture)(callback, config.options.on_texture_user, texture, facts)
            };
        }
        unsafe {
            d3d11_observer_capture::record_texture(
                device,
                texture,
                facts.width,
                facts.format,
                facts.bind_flags,
            )
        };
    });
}

pub(super) unsafe fn buffer_created(
    device: Handle,
    buffer: Handle,
    initial: *const c_void,
    size: u32,
    flags: u32,
) {
    creation(|config| {
        let callback = config.options.on_buffer;
        if callback.is_some() {
            unsafe {
                (config.calls.buffer)(
                    callback,
                    config.options.on_buffer_user,
                    buffer,
                    initial,
                    size,
                    flags,
                )
            };
        }
        unsafe { d3d11_observer_capture::record_buffer(device, buffer, size, flags) };
    });
}

pub(super) unsafe fn shader_created(shader: Handle, stage: u32, bytes: *const c_void, size: u32) {
    creation(|config| {
        if config.options.on_shader.is_some() {
            unsafe {
                (config.calls.shader)(
                    config.options.on_shader,
                    config.options.on_shader_user,
                    shader,
                    stage,
                    bytes,
                    size,
                )
            };
        }
    });
}

pub(super) unsafe fn layout_created(
    layout: Handle,
    elements: *const Layout,
    copied: u32,
    count: u32,
) {
    creation(|config| {
        if config.options.on_layout.is_some() {
            unsafe {
                (config.calls.layout)(
                    config.options.on_layout,
                    config.options.on_layout_user,
                    layout,
                    elements,
                    copied,
                    count,
                )
            };
        }
    });
}
