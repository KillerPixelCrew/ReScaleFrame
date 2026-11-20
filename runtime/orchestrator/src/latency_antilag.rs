// SPDX-License-Identifier: MIT
use crate::render_abi::{RSF_PACING_NONE, RSF_REFLEX_OFF};
use std::ffi::c_void;
use std::ptr;

pub(crate) fn fps_from_interval(limit_us: u32) -> u32 {
    1_000_000_u32
        .checked_div(limit_us)
        .map_or(0, |fps| fps.max(1))
}

#[repr(C)]
struct Driver {
    initialize: unsafe extern "C" fn(*mut c_void, u32, *mut c_void) -> u32,
    update: unsafe extern "C" fn(*mut c_void, u32, u32) -> u32,
    end_render: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, u32) -> u32,
    shutdown: unsafe extern "C" fn(*mut c_void),
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct State {
    available: u32,
    enabled: u32,
    updates: u64,
    end_markers: u64,
    refused: u64,
}

struct Context {
    calls: &'static Driver,
    driver: *mut c_void,
    device: *mut c_void,
    api: u32,
    vendor: u32,
    last_update: u64,
    last_end: u64,
    state: State,
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { (self.calls.shutdown)(self.driver) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_antilag_create(calls: &'static Driver, driver: *mut c_void) -> *mut Context {
    crate::allocate_handle(Context {
        calls,
        driver,
        device: ptr::null_mut(),
        api: 0,
        vendor: 0,
        last_update: 0,
        last_end: 0,
        state: State::default(),
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_destroy(context: *mut Context) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_bind(
    context: *mut Context,
    api: u32,
    device: *mut c_void,
    vendor: u32,
) {
    let Some(context) = (unsafe { context.as_mut() }) else {
        return;
    };
    if (context.api, context.device, context.vendor) == (api, device, vendor) {
        return;
    }
    unsafe { (context.calls.shutdown)(context.driver) };
    context.api = api;
    context.device = device;
    context.vendor = vendor;
    context.last_update = 0;
    context.last_end = 0;
    context.state.available = 0;
    context.state.enabled = 0;
    if vendor == 0x1002 && !device.is_null() && matches!(api, 11 | 12) {
        context.state.available =
            unsafe { (context.calls.initialize)(context.driver, api, device) };
        if context.state.available == 0 {
            context.state.refused = context.state.refused.wrapping_add(1);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_pace(
    context: *mut Context,
    id: u64,
    mode: u32,
    limit_us: u32,
    other_owner: u32,
) -> u32 {
    let Some(context) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if context.state.available == 0 || id == 0 || id <= context.last_update {
        return 0;
    }
    context.last_update = id;
    let enabled = mode != RSF_REFLEX_OFF && other_owner == RSF_PACING_NONE;
    let fps = if enabled {
        fps_from_interval(limit_us)
    } else {
        0
    };
    if unsafe { (context.calls.update)(context.driver, u32::from(enabled), fps) } == 0 {
        context.state.refused = context.state.refused.wrapping_add(1);
        context.state.available = 0;
        context.state.enabled = 0;
        unsafe { (context.calls.shutdown)(context.driver) };
        return 0;
    }
    context.state.enabled = u32::from(enabled);
    context.state.updates = context.state.updates.wrapping_add(1);
    context.state.enabled
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_end_render(
    context: *mut Context,
    id: u64,
    chain: *mut c_void,
    generated: u32,
) -> u32 {
    let Some(context) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if context.state.available == 0
        || context.api != 12
        || chain.is_null()
        || id == 0
        || id == context.last_end
    {
        return 0;
    }
    context.last_end = id;
    if unsafe {
        (context.calls.end_render)(context.driver, chain, context.state.enabled, generated)
    } == 0
    {
        context.state.refused = context.state.refused.wrapping_add(1);
        context.state.enabled = 0;
        return 0;
    }
    context.state.end_markers = context.state.end_markers.wrapping_add(1);
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_state(context: *const Context, out: *mut State) {
    let state = unsafe { context.as_ref() }.map_or(State::default(), |context| context.state);
    unsafe { out.write(state) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_reset_sources(context: *mut Context) {
    let Some(context) = (unsafe { context.as_mut() }) else {
        return;
    };
    if context.state.available != 0
        && context.state.enabled != 0
        && unsafe { (context.calls.update)(context.driver, 0, 0) } == 0
    {
        context.state.refused = context.state.refused.wrapping_add(1);
        context.state.available = 0;
    }
    context.state.enabled = 0;
    context.last_update = 0;
    context.last_end = 0;
}
