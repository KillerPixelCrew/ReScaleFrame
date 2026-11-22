// SPDX-License-Identifier: MIT
//! Standalone NGX tracing. The optional SDK leaf owns typed parameter access.
use crate::{Handle, sdk_abi::*, strtof};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::{
    ffi::c_void,
    io::{self, Write},
    mem::transmute,
    ptr,
};
use windows::Win32::Graphics::Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Resource, ID3D11Texture2D};
use windows::Win32::System::Environment::GetEnvironmentVariableA;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::core::{Interface, PCSTR, s, w};

type Create = unsafe extern "C" fn(Handle, usize, *const c_void, Handle) -> u32;
type Evaluate = unsafe extern "C" fn(Handle, Handle, Handle, Handle) -> u32;

static CREATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static EVALUATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: Handle, detour: Handle, original: *mut Handle) -> i32;
    fn MH_EnableHook(target: Handle) -> i32;
    fn MH_DisableHook(target: Handle) -> i32;
    fn MH_Uninitialize() -> i32;
}

unsafe extern "C" fn create(
    context: Handle,
    second: usize,
    third: *const c_void,
    fourth: Handle,
) -> u32 {
    // The component can omit the frontend's feature ID. Forward every argument unchanged.
    let parameters = if second < 16 {
        third
    } else {
        second as *const c_void
    };
    if !parameters.is_null() {
        unsafe { trace_create(parameters) };
    }
    let original = unsafe { transmute::<Handle, Create>(CREATE.load(Ordering::Acquire)) };
    unsafe { original(context, second, third, fourth) }
}

unsafe extern "C" fn evaluate(
    context: Handle,
    feature: Handle,
    parameters: Handle,
    callback: Handle,
) -> u32 {
    if !parameters.is_null() {
        unsafe { trace_evaluate(parameters) };
    }
    let original = unsafe { transmute::<Handle, Evaluate>(EVALUATE.load(Ordering::Acquire)) };
    unsafe { original(context, feature, parameters, callback) }
}

fn override_value(name: PCSTR) -> Option<f32> {
    let mut text = [0_u8; 64];
    let size = unsafe { GetEnvironmentVariableA(name, Some(&mut text)) } as usize;
    if size == 0 || size >= text.len() {
        return None;
    }
    let value = unsafe { strtof(text.as_ptr().cast(), ptr::null_mut()) };
    (value > 0.0).then_some(value)
}

unsafe fn trace_create(params: *const c_void) {
    let mut flags = 0;
    let result = unsafe { rsf_probe_ngx_get_flags(params, &mut flags) };
    let mut ldr = [0_u8; 8];
    let size = unsafe { GetEnvironmentVariableA(s!("RSF_PROBE_LDR"), Some(&mut ldr)) } as usize;
    if size != 0 && size < ldr.len() && ldr[0] == b'1' {
        flags &= !(RSF_PROBE_NGX_HDR as i32);
        unsafe { rsf_probe_ngx_set_flags(params, flags) };
    }
    let flags = flags as u32;
    let has = |bit| u32::from(flags & bit != 0);
    let _ = writeln!(
        io::stderr().lock(),
        "NGX_CREATE flags_get={result:x} flags={flags:x} HDR={} AutoExposure={} DepthInverted={}",
        has(RSF_PROBE_NGX_HDR),
        has(RSF_PROBE_NGX_AUTO_EXPOSURE),
        has(RSF_PROBE_NGX_DEPTH_INVERTED),
    );
}

unsafe fn trace_evaluate(params: Handle) {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    let pre = override_value(s!("RSF_PROBE_PRE_EXPOSURE"));
    let scale = override_value(s!("RSF_PROBE_EXPOSURE_SCALE"));
    if pre.is_some() || scale.is_some() {
        unsafe {
            rsf_probe_ngx_set_exposure(
                params,
                pre.as_ref().map_or(ptr::null(), ptr::from_ref),
                scale.as_ref().map_or(ptr::null(), ptr::from_ref),
            )
        };
    }
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let (mut pre, mut scale) = (0.0, 0.0);
    unsafe { rsf_probe_ngx_get_exposure(params, &mut pre, &mut scale) };
    let _ = writeln!(
        io::stderr().lock(),
        "NGX_EVALUATE preExposure={pre} exposureScale={scale}"
    );
    for (output, name) in [(0, "Color"), (1, "Output")] {
        let raw = unsafe { rsf_probe_ngx_get_resource(params, output) };
        let resource = unsafe { ID3D11Resource::from_raw_borrowed(&raw) };
        if let Some(texture) = resource.and_then(|resource| resource.cast::<ID3D11Texture2D>().ok())
        {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { texture.GetDesc(&mut desc) };
            let _ = writeln!(
                io::stderr().lock(),
                "NGX_RESOURCE {name} {}x{} format={} bind={:x}",
                desc.Width,
                desc.Height,
                desc.Format.0 as u32,
                desc.BindFlags
            );
        }
    }
}

fn hook(target: Handle, detour: Handle, original: &AtomicPtr<c_void>) -> bool {
    let mut trampoline = ptr::null_mut();
    if unsafe { MH_CreateHook(target, detour, &mut trampoline) } != 0 || trampoline.is_null() {
        return false;
    }
    original.store(trampoline, Ordering::Release);
    unsafe { MH_EnableHook(target) == 0 }
}

pub(super) struct Trace;

impl Trace {
    pub(super) fn install() -> Option<Self> {
        if unsafe { rsf_probe_ngx_available() } == 0 {
            return None;
        }
        let module = unsafe { GetModuleHandleW(w!("nvngx_dlss.dll")) };
        let creation = module.as_ref().ok().and_then(|&module| unsafe {
            GetProcAddress(module, s!("NVSDK_NGX_D3D11_CreateFeature"))
        });
        let Some(creation) = creation else {
            eprintln!("NGX trace unavailable: component not loaded");
            return Some(Self);
        };
        if unsafe { MH_Initialize() } == 0 && hook(creation as Handle, create as Handle, &CREATE) {
            eprintln!("NGX creation trace installed in standalone probe");
        }
        if let Ok(module) = module
            && let Some(evaluation) =
                unsafe { GetProcAddress(module, s!("NVSDK_NGX_D3D11_EvaluateFeature")) }
        {
            hook(evaluation as Handle, evaluate as Handle, &EVALUATE);
        }
        Some(Self)
    }
}

impl Drop for Trace {
    fn drop(&mut self) {
        unsafe {
            MH_DisableHook(ptr::null_mut()); // MH_ALL_HOOKS
            MH_Uninitialize();
        }
    }
}
