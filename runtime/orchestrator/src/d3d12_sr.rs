// SPDX-License-Identifier: MIT
//! Shared D3D12 SR provider owners. Native leaves keep the vendor types and COM calls.
use crate::allocate_handle;
use crate::render_abi::{rsf_sr_frame as Frame, rsf_sr_open_desc as Open};
use crate::sdk_module::SdkModule;
use std::ffi::{c_char, c_void};
use std::mem::MaybeUninit;
use std::ptr::null_mut;

#[path = "d3d12_sr/fsr_abi.rs"]
pub(crate) mod ffx_abi;
mod fsr;
mod xess;

type Raw = *mut c_void;
type Extent = [u32; 2];
type Load = unsafe extern "C" fn(*const Open) -> Raw;
type ValidateResources = unsafe extern "C" fn(*const Frame, Raw) -> i32;

const OK: i32 = 0;
const INVALID: i32 = -1;
const LOAD_FAILED: i32 = -4;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const ULTRA_PERFORMANCE: u32 = 4;
const FEATURE_FAILED: i32 = -9;
const MISSING_ENTRY: i32 = -5;

enum Backend {
    Fsr(fsr::Owner),
    Xess(xess::Owner),
}

struct Version {
    id: u64,
    name: [c_char; 128],
}

impl Version {
    fn new(id: u64, name: &[u8]) -> Self {
        let mut version = Self { id, name: [0; 128] };
        copy_bytes(&mut version.name[..127], name);
        version
    }
}

struct Session {
    device: Raw,
    output: Extent,
    version: Version,
    backend: Backend,
}

fn checked(result: i32) -> Result<(), i32> {
    if result == OK { Ok(()) } else { Err(result) }
}

fn copy_bytes(out: &mut [c_char], source: &[u8]) {
    for (out, &byte) in out.iter_mut().zip(source) {
        *out = byte as c_char;
    }
}

fn open_session(
    desc: &Open,
    out: *mut Raw,
    initialize: impl FnOnce(Extent) -> Result<(Backend, Version), i32>,
) -> i32 {
    // Reserve the handle before loading the SDK. Failed opens leave *out null.
    let session = allocate_handle(MaybeUninit::<Session>::uninit());
    if session.is_null() {
        return INIT_FAILED;
    }
    let session = unsafe { Box::from_raw(session) };
    let device = desc.device;
    let output = [desc.output_width, desc.output_height];
    let (backend, version) = match initialize(output) {
        Ok(value) => value,
        Err(result) => return result,
    };
    let session = Box::write(
        session,
        Session {
            device,
            output,
            version,
            backend,
        },
    );
    unsafe { out.write(Box::into_raw(session).cast()) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_sr_provider_plan(
    session: Raw,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    if session.is_null() || width.is_null() || height.is_null() {
        return INVALID;
    }
    let session = session.cast::<Session>();
    let output = unsafe { &(*session).output };
    match unsafe { &mut (*session).backend } {
        Backend::Fsr(owner) => owner.plan(output, quality, width, height),
        Backend::Xess(owner) => owner.plan(output, quality, width, height),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_sr_provider_evaluate(
    session: Raw,
    commands: Raw,
    frame: *const Frame,
    validate_resources: ValidateResources,
) -> i32 {
    // The native facade validates the frame before checking the session/list.
    if session.is_null() || commands.is_null() {
        return INVALID;
    }
    let session = session.cast::<Session>();
    let result = unsafe { validate_resources(frame, (*session).device) };
    if result != OK {
        return result;
    }
    let frame = unsafe { &*frame };
    match unsafe { &mut (*session).backend } {
        Backend::Fsr(owner) => owner.evaluate(commands, frame),
        Backend::Xess(owner) => owner.evaluate(commands, frame),
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_d3d12_sr_provider_release(session: Raw) -> i32 {
    // The caller retires GPU work before close. There is no separate SDK release.
    if session.is_null() { INVALID } else { OK }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_sr_provider_close(session: Raw) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session.cast::<Session>()) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_sr_provider_version(
    session: Raw,
    id: *mut u64,
    name: *mut *const c_char,
) -> i32 {
    if session.is_null() || id.is_null() || name.is_null() {
        return INVALID;
    }
    let version = unsafe { &(*session.cast::<Session>()).version };
    unsafe {
        id.write(version.id);
        name.write(version.name.as_ptr());
    }
    OK
}
