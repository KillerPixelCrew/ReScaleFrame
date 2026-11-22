// SPDX-License-Identifier: MIT
use super::{rsf_ac7_capture_declared_constants, rsf_ac7_capture_observe_shader};
use crate::motion_capture_session::{CaptureInterval, rsf_ac7_capture_fail};
use std::ffi::c_void;
use std::sync::OnceLock;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{HRESULT, PCSTR, s, w};

type Disassemble =
    unsafe extern "system" fn(*const c_void, usize, u32, PCSTR, *mut Option<ID3DBlob>) -> HRESULT;

fn disassembler() -> Option<Disassemble> {
    static ENTRY: OnceLock<Option<Disassemble>> = OnceLock::new();
    *ENTRY.get_or_init(|| {
        // AC7 strips reflection but the DXBC declares used slots. Keep the compiler optional
        // and loaded for the process lifetime once its disassembly export has been resolved.
        let module =
            unsafe { LoadLibraryExW(w!("d3dcompiler_47.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .ok()?;
        let address = unsafe { GetProcAddress(module, s!("D3DDisassemble")) }?;
        // The System32 export is the SDK's D3DDisassemble entry.
        Some(unsafe { std::mem::transmute(address) })
    })
}

unsafe extern "C" fn disassemble_constants(bytes: *const c_void, size: usize) -> u32 {
    let mut code = None;
    if let Some(disassemble) = disassembler()
        && unsafe { disassemble(bytes, size, 0, PCSTR::null(), &mut code) }.is_ok()
        && let Some(code) = code
    {
        return unsafe {
            rsf_ac7_capture_declared_constants(code.GetBufferPointer().cast(), code.GetBufferSize())
        };
    }
    super::ALL_CONSTANTS
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_motion_capture_shader(
    shader: *mut c_void,
    stage: u32,
    bytes: *const c_void,
    size: u32,
) {
    // The observer carries live shader bytecode for this creation callback.
    if !unsafe { rsf_ac7_capture_observe_shader(shader, stage, bytes, size, disassemble_constants) }
    {
        rsf_ac7_capture_fail(CaptureInterval::load());
    }
}
