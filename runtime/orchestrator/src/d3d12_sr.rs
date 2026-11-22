// SPDX-License-Identifier: MIT
//! D3D12 SR provider owners, loader policy and resource admission.
use crate::allocate_handle;
use crate::ffx_version::{MAX_VERSIONS, Version as ProviderVersion};
use crate::sdk_module::{SdkModule, d3d12_library_path, load_system_library};
use crate::sys::{rsf_sr_frame as Frame, rsf_sr_open_desc as Open};
use rsf_game_sys::backend_result::{
    FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, LOAD_FAILED, MISSING_ENTRY_POINT, NOT_SUPPORTED,
    OK,
};
use std::ffi::{c_char, c_void};
use std::mem::MaybeUninit;
use std::ptr::null_mut;

pub(crate) use rsf_fsr4_compat::{ffx_abi, ffx_resource};
mod fsr;
mod legacy;
mod xess;

type Raw = *mut c_void;
type Extent = [u32; 2];
const ULTRA_PERFORMANCE: u32 = 4;

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
        return INVALID_ARGUMENT;
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
    let result = unsafe { crate::sr_session::rsf_core_validate_sr_frame(frame) };
    if result != OK {
        return result;
    }
    if session.is_null() || commands.is_null() {
        return INVALID_ARGUMENT;
    }
    let session = session.cast::<Session>();
    let frame = unsafe { &*frame };
    let result = validate_resources(unsafe { (*session).device }, frame);
    if result != OK {
        return result;
    }
    match unsafe { &mut (*session).backend } {
        Backend::Fsr(owner) => owner.evaluate(commands, frame),
        Backend::Xess(owner) => owner.evaluate(commands, frame),
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_d3d12_sr_provider_release(session: Raw) -> i32 {
    // The caller retires GPU work before close. There is no separate SDK release.
    if session.is_null() {
        INVALID_ARGUMENT
    } else {
        OK
    }
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
        return INVALID_ARGUMENT;
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

pub(super) fn validate_resources(device: Raw, frame: &Frame) -> i32 {
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::core::Interface;
    let record = unsafe { &*frame.record };
    let render = [record.render_width, record.render_height];
    let output = [record.output_width, record.output_height];
    for (binding, size, state) in [
        (
            &frame.color,
            render,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &frame.depth,
            render,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &frame.motion,
            render,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (&frame.output, output, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
        (
            &frame.exposure,
            [1, 1],
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &frame.reactive,
            render,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        (
            &frame.transparency,
            render,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
    ] {
        let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&binding.resource) })
        else {
            continue;
        };
        let desc = unsafe { resource.GetDesc() };
        if !crate::d3d12_host::owned_by(resource, device)
            || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || desc.Width < u64::from(size[0])
            || desc.Height < size[1]
            || desc.DepthOrArraySize != 1
            || desc.SampleDesc.Count != 1
            || binding.state != state.0 as u32
        {
            return INVALID_ARGUMENT;
        }
    }
    if frame.color.resource == frame.output.resource {
        INVALID_ARGUMENT
    } else {
        OK
    }
}

fn load_runtime(desc: &Open, name: &str) -> Option<SdkModule> {
    if desc.runtime_directory_utf8.is_null() {
        return None;
    }
    let directory = unsafe { std::ffi::CStr::from_ptr(desc.runtime_directory_utf8) }
        .to_str()
        .ok()?;
    let path = d3d12_library_path(directory, name.encode_utf16())?;
    let (module, error) = unsafe { load_system_library(&path) };
    if module.is_null() {
        crate::input_dump::Log::new(desc.log, desc.log_user).say(&format!(
            "could not load {directory}\\{name} (Windows error {error})"
        ));
    }
    SdkModule::adopt_system(module)
}
