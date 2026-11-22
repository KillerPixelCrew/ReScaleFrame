// SPDX-License-Identifier: MIT
//! Standalone NGX tracing. The optional SDK leaf owns typed parameter access.
use crate::Handle;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::{ffi::c_void, mem::transmute, ptr};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::core::{s, w};

type Create = unsafe extern "C" fn(Handle, usize, *const c_void, Handle) -> u32;
type Evaluate = unsafe extern "C" fn(Handle, Handle, Handle, Handle) -> u32;

static CREATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static EVALUATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

unsafe extern "C" {
    fn rsf_probe_ngx_available() -> i32;
    fn rsf_probe_ngx_create_parameters(parameters: *const c_void);
    fn rsf_probe_ngx_evaluate_parameters(parameters: Handle);
}

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
        unsafe { rsf_probe_ngx_create_parameters(parameters) };
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
        unsafe { rsf_probe_ngx_evaluate_parameters(parameters) };
    }
    let original = unsafe { transmute::<Handle, Evaluate>(EVALUATE.load(Ordering::Acquire)) };
    unsafe { original(context, feature, parameters, callback) }
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
