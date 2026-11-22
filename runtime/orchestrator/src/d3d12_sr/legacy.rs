// SPDX-License-Identifier: MIT
#[path = "legacy_abi.rs"]
mod abi;

use self::abi::{rsf_fg_provider as Fg, rsf_fg_swapchain_result as FgResult};
use super::*;
use crate::render_abi::{
    RSF_API_D3D12, RSF_BACKEND_ABI_VERSION, rsf_backend_caps as Caps,
    rsf_backend_probe_desc as Probe, rsf_fg_frame as FgFrame, rsf_fg_swapchain_desc as Chain,
    rsf_sr_provider as Sr,
};
use std::sync::OnceLock;

const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;
const NOT_READY: i32 = -8;
const AMD: u32 = 3;
const INTEL: u32 = 2;
type OpenFn = unsafe extern "C" fn(*const Open, *mut Raw) -> i32;

unsafe extern "C" fn probe<const FSR: bool, const COMPILED: bool>(
    desc: *const Probe,
    caps: *mut Caps,
) -> i32 {
    if desc.is_null() || caps.is_null() {
        return INVALID;
    }
    let desc_size = unsafe { (&raw const (*desc).struct_size).read() };
    let caps_size = unsafe { (&raw const (*caps).struct_size).read() };
    if (desc_size as usize) < size_of::<Probe>() || (caps_size as usize) < size_of::<Caps>() {
        return INVALID;
    }
    let desc = unsafe { &*desc };
    if desc.abi_version != RSF_BACKEND_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let (name, vendor, unavailable, message) = if FSR {
        (
            c"FidelityFX",
            AMD,
            c"built without the FidelityFX headers",
            if COMPILED {
                c"fsr: SR compiled in, D3D12 only; legacy FG unavailable"
            } else {
                c"fsr: not compiled in"
            },
        )
    } else {
        (
            c"XeSS",
            INTEL,
            c"built without the XeSS headers",
            if COMPILED {
                c"xess: SR compiled in, D3D12; native D3D11 and legacy FG unavailable"
            } else {
                c"xess: not compiled in"
            },
        )
    };
    // Remaining fields are integer flags and a nullable pointer.
    let mut result: Caps = unsafe { std::mem::zeroed() };
    result.struct_size = caps_size;
    result.vendor = vendor;
    result.name = name.as_ptr();
    if COMPILED {
        result.available = 1;
        result.sr_apis = RSF_API_D3D12;
        result.supported_lifetimes = 1; // Only Now
    } else {
        result.refusal_utf8 = unavailable.as_ptr();
    }
    unsafe { caps.write(result) };
    if let Some(log) = desc.log {
        unsafe { log(desc.log_user, message.as_ptr()) };
    }
    if COMPILED { OK } else { NOT_COMPILED }
}

extern "C" fn absent_open(_: *const Open, _: *mut Raw) -> i32 {
    NOT_COMPILED
}
extern "C" fn absent_plan(_: Raw, _: u32, _: *mut u32, _: *mut u32) -> i32 {
    NOT_COMPILED
}
extern "C" fn absent_evaluate(_: Raw, _: Raw, _: *const Frame) -> i32 {
    NOT_COMPILED
}
extern "C" fn unavailable<const ERROR: i32>(_: Raw) -> i32 {
    ERROR
}
extern "C" fn noop(_: Raw) {}
extern "C" fn fg_create<const ERROR: i32>(_: *const Chain, _: *mut FgResult, _: *mut Raw) -> i32 {
    ERROR
}
extern "C" fn fg_options<const ERROR: i32>(_: Raw, _: u32, _: u32, _: u32) -> i32 {
    ERROR
}
extern "C" fn fg_tag<const ERROR: i32>(_: Raw, _: Raw, _: *const FgFrame) -> i32 {
    ERROR
}
extern "C" fn fg_resize<const ERROR: i32>(_: Raw, _: u32, _: u32, _: u32) -> i32 {
    ERROR
}
extern "C" fn fg_marker<const ERROR: i32>(_: Raw, _: u32, _: u64) -> i32 {
    ERROR
}
extern "C" fn fg_count<const ERROR: i32>(_: Raw, _: *mut u64) -> i32 {
    ERROR
}

const fn absent_sr<const FSR: bool>() -> Sr {
    Sr {
        struct_size: size_of::<Sr>() as u32,
        probe: Some(probe::<FSR, false>),
        open: Some(absent_open),
        plan: Some(absent_plan),
        evaluate: Some(absent_evaluate),
        release_resources: Some(unavailable::<NOT_COMPILED>),
        close: Some(noop),
        get_version: None,
    }
}

const fn compiled_sr<const FSR: bool>(open: OpenFn) -> Sr {
    Sr {
        struct_size: size_of::<Sr>() as u32,
        probe: Some(probe::<FSR, true>),
        open: Some(open),
        plan: Some(rsf_core_d3d12_sr_provider_plan),
        evaluate: Some(rsf_core_d3d12_sr_provider_evaluate),
        release_resources: Some(rsf_core_d3d12_sr_provider_release),
        close: Some(rsf_core_d3d12_sr_provider_close),
        get_version: Some(rsf_core_d3d12_sr_provider_version),
    }
}

const fn legacy_fg<const FSR: bool, const COMPILED: bool, const ERROR: i32>() -> Fg {
    Fg {
        struct_size: size_of::<Fg>() as u32,
        probe: Some(probe::<FSR, COMPILED>),
        create_swapchain: Some(fg_create::<ERROR>),
        set_options: Some(fg_options::<ERROR>),
        tag_frame: Some(fg_tag::<ERROR>),
        after_present: Some(unavailable::<ERROR>),
        resize: Some(fg_resize::<ERROR>),
        latency_marker: Some(fg_marker::<ERROR>),
        latency_sleep: Some(unavailable::<ERROR>),
        get_generated_count: Some(fg_count::<ERROR>),
        destroy_swapchain: Some(noop),
    }
}

unsafe fn validate_open<'a>(desc: *const Open, out: *mut Raw) -> Result<&'a Open, i32> {
    const ULTRA_QUALITY: u32 = 5;
    const WRONG_API: i32 = -12;
    let desc = unsafe { crate::sr_session::checked(desc) }.ok_or(INVALID)?;
    if out.is_null()
        || desc.device.is_null()
        || desc.output_width == 0
        || desc.output_height == 0
        || desc.quality > ULTRA_QUALITY
    {
        return Err(INVALID);
    }
    unsafe { out.write(null_mut()) };
    if desc.abi_version != RSF_BACKEND_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    if desc.api != RSF_API_D3D12 {
        return Err(WRONG_API);
    }
    Ok(desc)
}

unsafe extern "C" fn fsr_open_managed(desc: *const Open, out: *mut Raw) -> i32 {
    let desc = match unsafe { validate_open(desc, out) } {
        Ok(desc) => desc,
        Err(result) => return result,
    };
    let Some(compatibility) = FSR_COMPATIBILITY.get() else {
        return NOT_COMPILED;
    };
    fsr::open(desc, out, compatibility)
}

unsafe extern "C" fn fsr_open(desc: *const Open, out: *mut Raw) -> i32 {
    let result = unsafe { fsr_open_managed(desc, out) };
    if result == OK {
        let desc = unsafe { &*desc };
        if let Some(log) = desc.log {
            let session = unsafe { &*(*out).cast::<Session>() };
            unsafe { log(desc.log_user, session.version.name.as_ptr()) };
        }
    }
    result
}

unsafe extern "C" fn xess_open(desc: *const Open, out: *mut Raw) -> i32 {
    match unsafe { validate_open(desc, out) } {
        Ok(desc) => xess::open(desc, out),
        Err(result) => result,
    }
}

static FSR_COMPATIBILITY: OnceLock<fsr::CompatibilityOps> = OnceLock::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_core_d3d12_fsr_sr_provider(
    managed: u32,
    acquire: Option<fsr::Acquire>,
    release: Option<fsr::Release>,
) -> *const Sr {
    static DISABLED: Sr = absent_sr::<true>();
    static DIRECT: Sr = compiled_sr::<true>(fsr_open);
    static MANAGED: Sr = compiled_sr::<true>(fsr_open_managed);
    let Some((acquire, release)) = acquire.zip(release) else {
        return &DISABLED;
    };
    let _ = FSR_COMPATIBILITY.set(fsr::CompatibilityOps { acquire, release });
    if managed != 0 { &MANAGED } else { &DIRECT }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_d3d12_xess_sr_provider(compiled: u32) -> *const Sr {
    static DISABLED: Sr = absent_sr::<false>();
    static ENABLED: Sr = compiled_sr::<false>(xess_open);
    if compiled != 0 { &ENABLED } else { &DISABLED }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_d3d12_legacy_fg_provider(fsr: u32, compiled: u32) -> *const Fg {
    static FSR_DISABLED: Fg = legacy_fg::<true, false, NOT_COMPILED>();
    static XESS_DISABLED: Fg = legacy_fg::<false, false, NOT_COMPILED>();
    static FSR_ENABLED: Fg = legacy_fg::<true, true, NOT_READY>();
    static XESS_ENABLED: Fg = legacy_fg::<false, true, NOT_READY>();
    match (fsr != 0, compiled != 0) {
        (true, false) => &FSR_DISABLED,
        (false, false) => &XESS_DISABLED,
        (true, true) => &FSR_ENABLED,
        (false, true) => &XESS_ENABLED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::ptr::null;

    #[test]
    fn backend_probe_and_legacy_fg_report_header_availability() {
        for fsr in [0, 1] {
            for compiled in [0, 1] {
                let mut desc: Probe = unsafe { std::mem::zeroed() };
                let mut caps: Caps = unsafe { std::mem::zeroed() };
                desc.struct_size = size_of::<Probe>() as u32;
                desc.abi_version = RSF_BACKEND_ABI_VERSION;
                caps.struct_size = size_of::<Caps>() as u32;

                let fg = unsafe { &*rsf_core_d3d12_legacy_fg_provider(fsr, compiled) };
                let probe = fg.probe.unwrap();
                assert!(fg.create_swapchain.is_some());
                assert_eq!(unsafe { probe(null(), &mut caps) }, INVALID);
                assert_eq!(unsafe { probe(&desc, null_mut()) }, INVALID);
                assert_eq!(
                    unsafe { probe(&desc, &mut caps) },
                    if compiled != 0 { OK } else { NOT_COMPILED }
                );
                assert_eq!(caps.struct_size, size_of::<Caps>() as u32);
                assert_eq!(caps.vendor, if fsr != 0 { AMD } else { INTEL });
                assert!(!caps.name.is_null());
                assert_eq!(caps.available, compiled);
                assert_eq!(caps.sr_apis, if compiled != 0 { RSF_API_D3D12 } else { 0 });
                assert_eq!(
                    caps.fg_apis
                        | caps.max_generated_frames
                        | caps.sr_fg_share_session
                        | caps.fg_requires_latency_markers
                        | caps.fg_owns_swapchain,
                    0
                );
                if compiled == 0 {
                    assert!(!caps.refusal_utf8.is_null());
                    assert!(
                        !unsafe { CStr::from_ptr(caps.refusal_utf8) }
                            .to_bytes()
                            .is_empty()
                    );
                }
                assert_eq!(
                    unsafe { fg.tag_frame.unwrap()(null_mut(), null_mut(), null()) },
                    if compiled != 0 {
                        NOT_READY
                    } else {
                        NOT_COMPILED
                    }
                );
                desc.abi_version += 1;
                assert_eq!(unsafe { probe(&desc, &mut caps) }, ABI_MISMATCH);
                caps.struct_size = size_of::<u32>() as u32;
                assert_eq!(unsafe { probe(&desc, &mut caps) }, INVALID);
            }
        }
    }
}
