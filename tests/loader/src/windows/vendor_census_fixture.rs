// SPDX-License-Identifier: MIT
#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Mutex, MutexGuard};

type Ptr = *mut c_void;
type ConstPtr = *const c_void;

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Seen {
    context: usize,
    descriptor: usize,
    calls: i32,
}

impl Seen {
    const fn new() -> Mutex<Self> {
        Mutex::new(Self {
            context: 0,
            descriptor: 0,
            calls: 0,
        })
    }

    fn record(&mut self, context: Ptr, descriptor: ConstPtr) {
        self.context = context as usize;
        self.descriptor = descriptor as usize;
        self.calls += 1;
    }

    unsafe fn publish(&self, context: *mut Ptr, descriptor: *mut ConstPtr, calls: *mut i32) {
        unsafe {
            context.write(self.context as Ptr);
            descriptor.write(self.descriptor as ConstPtr);
            calls.write(self.calls);
        }
    }
}

static FSR2: Mutex<Seen> = Seen::new();

#[unsafe(no_mangle)]
extern "C" fn ffxFsr2ContextCreate(context: Ptr, descriptor: ConstPtr) -> i32 {
    lock(&FSR2).record(context, descriptor);
    0x2001
}

#[unsafe(no_mangle)]
extern "C" fn ffxFsr2ContextDispatch(context: Ptr, descriptor: ConstPtr) -> i32 {
    lock(&FSR2).record(context, descriptor);
    0x1234_abcd
}

#[unsafe(no_mangle)]
unsafe extern "C" fn ffxFsr2GetJitterOffset(
    x: *mut f32,
    y: *mut f32,
    index: i32,
    phase: i32,
) -> i32 {
    unsafe {
        x.write(0.5 * (index % phase) as f32);
        y.write(-0.25 * index as f32);
    }
    lock(&FSR2).calls += 1;
    0
}

#[unsafe(no_mangle)]
extern "C" fn ffxFsr2GetUpscaleRatioFromQualityMode(mode: i32) -> f32 {
    lock(&FSR2).calls += 1;
    if mode == 2 { 1.7 } else { 1.5 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_fsr2_seen(
    context: *mut Ptr,
    descriptor: *mut ConstPtr,
    calls: *mut i32,
) {
    unsafe { lock(&FSR2).publish(context, descriptor, calls) };
}

struct NgxSeen {
    command: usize,
    handle: usize,
    parameters: usize,
    feature: u32,
    creates: i32,
    evaluates: i32,
}

static NGX: Mutex<NgxSeen> = Mutex::new(NgxSeen {
    command: 0,
    handle: 0,
    parameters: 0,
    feature: 0,
    creates: 0,
    evaluates: 0,
});
static NGX_HANDLE: i32 = 0;

#[unsafe(no_mangle)]
unsafe extern "C" fn NVSDK_NGX_D3D12_CreateFeature(
    command: Ptr,
    feature: u32,
    parameters: Ptr,
    handle: *mut Ptr,
) -> i32 {
    let mut seen = lock(&NGX);
    seen.command = command as usize;
    seen.feature = feature;
    seen.parameters = parameters as usize;
    seen.creates += 1;
    unsafe { handle.write((&raw const NGX_HANDLE).cast_mut().cast()) };
    1
}

#[unsafe(no_mangle)]
extern "C" fn NVSDK_NGX_D3D12_EvaluateFeature(
    command: Ptr,
    handle: Ptr,
    parameters: Ptr,
    callback: Ptr,
) -> i32 {
    let mut seen = lock(&NGX);
    seen.command = command as usize;
    seen.handle = handle as usize;
    seen.parameters = parameters as usize;
    seen.evaluates += 1;
    i32::from(callback.is_null())
}

#[unsafe(no_mangle)]
extern "C" fn NVSDK_NGX_D3D12_ReleaseFeature(handle: Ptr) -> i32 {
    lock(&NGX).handle = handle as usize;
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_ngx_seen(
    command: *mut Ptr,
    handle: *mut Ptr,
    parameters: *mut Ptr,
    feature: *mut u32,
    creates: *mut i32,
    evaluates: *mut i32,
) {
    let seen = lock(&NGX);
    unsafe {
        command.write(seen.command as Ptr);
        handle.write(seen.handle as Ptr);
        parameters.write(seen.parameters as Ptr);
        feature.write(seen.feature);
        creates.write(seen.creates);
        evaluates.write(seen.evaluates);
    }
}

static NVAPI: Mutex<Seen> = Seen::new();

extern "C" fn nv_marker(device: Ptr, parameters: Ptr) -> i32 {
    lock(&NVAPI).record(device, parameters);
    0x5151
}

extern "C" fn nv_sleep_mode(device: Ptr, parameters: Ptr) -> i32 {
    let mut seen = lock(&NVAPI);
    seen.record(device, parameters);
    seen.calls += 9;
    0x5252
}

#[unsafe(no_mangle)]
extern "C" fn nvapi_QueryInterface(id: u32) -> Ptr {
    match id {
        0xd998_4c05 => nv_marker as Ptr,
        0xac1c_a9e0 => nv_sleep_mode as Ptr,
        _ => null_mut(),
    }
}

#[unsafe(no_mangle)]
extern "C" fn fixture_nvapi_raw_marker() -> Ptr {
    nv_marker as Ptr
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_nvapi_seen(device: *mut Ptr, parameters: *mut Ptr, calls: *mut i32) {
    unsafe { lock(&NVAPI).publish(device, parameters.cast(), calls) };
}

struct XessSeen {
    context: usize,
    x: f32,
    y: f32,
    calls: i32,
    _parameters: usize,
}
static XESS: Mutex<XessSeen> = Mutex::new(XessSeen {
    context: 0,
    x: 0.0,
    y: 0.0,
    calls: 0,
    _parameters: 0,
});

#[unsafe(no_mangle)]
extern "C" fn xessSetJitterScale(context: Ptr, x: f32, y: f32) -> i32 {
    let mut seen = lock(&XESS);
    seen.context = context as usize;
    seen.x = x;
    seen.y = y;
    seen.calls += 1;
    0
}

#[unsafe(no_mangle)]
extern "C" fn xessD3D12Execute(context: Ptr, command: Ptr, parameters: ConstPtr) -> i32 {
    let mut seen = lock(&XESS);
    seen.context = context as usize;
    seen._parameters = parameters as usize;
    seen.calls += 1;
    if command.is_null() { 0x78 } else { 0x77 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_xess_seen(
    context: *mut Ptr,
    x: *mut f32,
    y: *mut f32,
    calls: *mut i32,
) {
    let seen = lock(&XESS);
    unsafe {
        context.write(seen.context as Ptr);
        x.write(seen.x);
        y.write(seen.y);
        calls.write(seen.calls);
    }
}

mod creation;
mod fidelityfx;
mod ngx_params;
mod streamline;
mod vulkan;
