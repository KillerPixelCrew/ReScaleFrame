// SPDX-License-Identifier: MIT
//! Facts from the actual NGX evaluation callback and the matching RAGE worker.
use super::detours::copy_resource;
use super::install::{Native, owner};
use super::metadata::*;
use super::recordings::resource_state;
use super::shots::{self, input_shot};
use super::{Capture, has_inputs};
use crate::{
    layout,
    platform::{lock, read_bytes, worker_tls_slots},
};
use std::ffi::{CStr, c_void};
use windows_sys::Win32::System::{
    Performance::QueryPerformanceCounter, SystemInformation::GetTickCount64,
    Threading::GetCurrentThreadId,
};

const GET_POINTER: u32 = 8;
const GET_D3D12: u32 = 9;
const GET_INT: u32 = 11;
const GET_UINT: u32 = 12;
const GET_FLOAT: u32 = 14;

unsafe fn get<T: Default>(parameters: *const c_void, slot: u32, key: &CStr) -> Option<T> {
    let mut value = T::default();
    unsafe {
        (owner().read_ngx)(
            parameters,
            slot,
            key.as_ptr(),
            (&mut value as *mut T).cast(),
        )
    }
    .then_some(value)
}

fn camera(state: &Native, input: &InputId) {
    let mut qpc = 0;
    unsafe { QueryPerformanceCounter(&mut qpc) };
    let mut facts = CameraFacts {
        frame: state
            .read(state.base + layout::FRAME_COUNTER)
            .unwrap_or_default(),
        thread: unsafe { GetCurrentThreadId() },
        tick: unsafe { GetTickCount64() },
        qpc: qpc as u64,
        ..CameraFacts::default()
    };
    let camera = state
        .read::<u32>(state.base + layout::TLS_INDEX)
        .filter(|&index| index < layout::block::TLS_LIMIT)
        .and_then(|index| {
            let address =
                unsafe { worker_tls_slots() }.checked_add(index as usize * size_of::<usize>())?;
            state.read::<usize>(address)
        })
        .and_then(|block| {
            facts.view = state
                .read(block.checked_add(layout::block::VIEW)?)
                .unwrap_or_default();
            facts.draw_context = state
                .read(block.checked_add(layout::block::DRAW)?)
                .unwrap_or_default();
            facts.job = state
                .read(block.checked_add(layout::camera_probe::JOB)?)
                .unwrap_or_default();
            let mut bytes = [0u8; CAMERA_BYTES];
            read_bytes(facts.view as usize, &mut bytes).then_some(bytes)
        });
    unsafe { input_camera(&state.capture, input, &facts, camera) };
}

unsafe fn read_scalar(parameters: *const c_void, key: &CStr, floating: bool) -> Option<f64> {
    if floating {
        unsafe { get::<f32>(parameters, GET_FLOAT, key) }.map(f64::from)
    } else {
        unsafe { get::<u32>(parameters, GET_UINT, key) }
            .map(f64::from)
            .or_else(|| unsafe { get::<i32>(parameters, GET_INT, key) }.map(f64::from))
    }
}

fn resource(
    capture: &Capture,
    list: *mut c_void,
    parameters: *const c_void,
    input: &InputId,
    role: u32,
    key: &CStr,
    required: bool,
) -> bool {
    let resource = unsafe { get::<*mut c_void>(parameters, GET_D3D12, key) }
        .or_else(|| unsafe { get::<*mut c_void>(parameters, GET_POINTER, key) })
        .filter(|resource| !resource.is_null());
    let Some(resource) = resource else {
        if required {
            unsafe { input_resource_refused(capture, input, role, true) };
        }
        return true;
    };
    let before = unsafe { resource_state(capture, list, resource) };
    match unsafe { input_shot(capture, input, role, list, resource, before, copy_resource) } {
        shots::NO_SLOT => false,
        shots::REFUSED => {
            unsafe { input_resource_refused(capture, input, role, false) };
            true
        }
        _ => true,
    }
}

pub(super) unsafe extern "C" fn created(
    handle: *mut c_void,
    parameters: *const c_void,
    feature: u32,
) {
    let flags = (feature == 1)
        .then(|| {
            let key = c"DLSS.Feature.Create.Flags";
            unsafe { get::<i32>(parameters, GET_INT, key) }
                .map(|value| value as u32)
                .or_else(|| unsafe { get::<u32>(parameters, GET_UINT, key) })
        })
        .flatten();
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { record_feature(&state.capture, handle, flags) };
}

pub(super) unsafe extern "C" fn released(handle: *mut c_void) {
    let state = owner();
    let _guard = lock(&state.observations);
    unsafe { record_feature(&state.capture, handle, None) };
}

pub(super) unsafe extern "C" fn evaluate(
    list: *mut c_void,
    handle: *mut c_void,
    parameters: *const c_void,
    phase: u32,
    result: u64,
) {
    if !track_evaluation(phase != 0) {
        return;
    }
    let state = owner();
    let capture = &state.capture;
    if phase == 0 && (list.is_null() || !has_inputs(capture)) {
        return;
    }
    let guard = lock(&state.observations);
    let no_slot = if phase == 0 {
        let Some(input) = (unsafe { begin_input(capture, list, handle) }) else {
            return;
        };
        camera(state, &input);
        unsafe {
            input_scalars(capture, &input, |key, floating| {
                read_scalar(parameters, key, floating)
            })
        };
        let copied = [
            (0, c"Color", true),
            (1, c"Depth", true),
            (2, c"MotionVectors", true),
            (3, c"ExposureTexture", false),
        ]
        .into_iter()
        .all(|(role, key, required)| {
            resource(capture, list, parameters, &input, role, key, required)
        });
        if !copied || !unsafe { commit_input(capture) } {
            unsafe { finish_input(capture) };
        }
        !copied
    } else {
        let copied = unsafe { input_result(capture, list, result) }
            .is_none_or(|input| resource(capture, list, parameters, &input, 4, c"Output", true));
        unsafe { finish_input(capture) };
        !copied
    };
    drop(guard);
    if no_slot {
        state.log(format_args!("NGX input observation failed"));
    }
}
