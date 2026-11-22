// SPDX-License-Identifier: MIT
//! Shared D3D12 SR provider owners. Native leaves keep the vendor types and COM calls.
use crate::allocate_handle;
use crate::ffx_version::{MAX_VERSIONS, Version as ProviderVersion};
use crate::render_abi::{rsf_sr_frame as Frame, rsf_sr_open_desc as Open};
use crate::sdk_module::SdkModule;
use std::ffi::{c_char, c_void};
use std::mem::MaybeUninit;
use std::ptr::null_mut;

#[path = "d3d12_sr/fsr_abi.rs"]
pub(crate) mod ffx_abi;
pub(super) mod ffx_resource;
mod fsr;
mod xess;

type Raw = *mut c_void;
type Extent = [u32; 2];
type Load = unsafe extern "C" fn(*const Open) -> Raw;
type ValidateFrame = unsafe extern "C" fn(*const Frame) -> i32;
type ValidateResources = unsafe extern "C" fn(*const Frame, Raw) -> i32;
static VALIDATION: std::sync::OnceLock<(ValidateFrame, ValidateResources)> =
    std::sync::OnceLock::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_core_d3d12_sr_bind_validation(
    frame: ValidateFrame,
    resources: ValidateResources,
) {
    let _ = VALIDATION.set((frame, resources));
}

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
) -> i32 {
    let Some(&(validate_frame, validate_resources)) = VALIDATION.get() else {
        return INVALID;
    };
    let result = unsafe { validate_frame(frame) };
    if result != OK {
        return result;
    }
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

pub(super) unsafe fn query_d3d12(
    query: ffx_abi::PfnFfxQuery,
    device: *mut std::ffi::c_void,
    kind: u64,
    versions: &mut [ProviderVersion; MAX_VERSIONS],
) -> Option<&[ProviderVersion]> {
    use crate::ffx_version::from_sdk;
    use ffx_abi::*;
    use std::ptr::{null, null_mut};

    let mut ids = [0; MAX_VERSIONS];
    let mut names = [null(); MAX_VERSIONS];
    let mut count = MAX_VERSIONS as u64;
    let mut desc = ffxQueryDescGetVersions {
        header: ffxApiHeader {
            type_: u64::from(FFX_API_QUERY_DESC_TYPE_GET_VERSIONS),
            pNext: null_mut(),
        },
        createDescType: kind,
        device,
        outputCount: &mut count,
        versionIds: ids.as_mut_ptr(),
        versionNames: names.as_mut_ptr(),
    };
    if unsafe { query?(null_mut(), (&raw mut desc).cast()) } != FFX_API_RETURN_OK as u32 {
        return None;
    }
    let versions = versions.get_mut(..usize::try_from(count).ok()?)?;
    for ((version, id), name) in versions.iter_mut().zip(ids).zip(names) {
        *version = unsafe { from_sdk(id, name) };
    }
    Some(versions)
}
