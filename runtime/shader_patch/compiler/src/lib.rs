// SPDX-License-Identifier: MIT
//! D3DCompiler ownership for standalone helpers and engine-supplied compiler calls.
#![cfg(windows)]

use std::ffi::{CStr, c_void};
use std::ptr;
use windows::Win32::Graphics::Direct3D::{D3D_SHADER_MACRO, ID3DBlob};
use windows::core::{HRESULT, PCSTR};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

type D3DCompile = unsafe extern "system" fn(
    *const c_void,
    usize,
    PCSTR,
    *const D3D_SHADER_MACRO,
    *mut c_void,
    PCSTR,
    PCSTR,
    u32,
    u32,
    *mut Option<ID3DBlob>,
    *mut Option<ID3DBlob>,
) -> HRESULT;

/// Keeps the compiler loaded until its last call has copied and released both blobs.
pub struct Compiler(HMODULE);

impl Compiler {
    pub fn system(name: &str) -> Option<Self> {
        let name = rsf_game_sys::ffi_util::wide(name);
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        (!module.is_null()).then(|| Self(module))
    }

    pub fn entry(&self) -> Option<*mut c_void> {
        unsafe { GetProcAddress(self.0, c"D3DCompile".as_ptr().cast()) }
            .map(|proc| proc as *mut c_void)
    }
}

impl Drop for Compiler {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

pub fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    let size = unsafe { blob.GetBufferSize() };
    if size == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), size) }
    }
}

pub struct Request<'a> {
    pub source: &'a [u8],
    pub name: &'a CStr,
    pub entry: &'a CStr,
    pub target: &'a CStr,
    pub flags: u32,
}

pub struct Output {
    pub status: i32,
    pub code: Option<Vec<u8>>,
    pub errors: Option<Vec<u8>>,
}

/// # Safety
///
/// `entry` points at a live D3DCompile-compatible export. A nonempty macro table must be
/// terminated with a null name. `include` must be null or a valid ID3DInclude for this call.
pub unsafe fn compile(
    entry: *mut c_void,
    request: Request<'_>,
    macros: &[D3D_SHADER_MACRO],
    include: *mut c_void,
) -> Output {
    let compile: D3DCompile = unsafe { std::mem::transmute(entry) };
    let mut code = None;
    let mut errors = None;
    let macros = if macros.is_empty() {
        ptr::null()
    } else {
        macros.as_ptr()
    };
    let hr = unsafe {
        compile(
            request.source.as_ptr().cast(),
            request.source.len(),
            PCSTR(request.name.as_ptr().cast()),
            macros,
            include,
            PCSTR(request.entry.as_ptr().cast()),
            PCSTR(request.target.as_ptr().cast()),
            request.flags,
            0,
            &mut code,
            &mut errors,
        )
    };
    Output {
        status: hr.0,
        code: code.as_ref().map(|blob| blob_bytes(blob).to_vec()),
        errors: errors.as_ref().map(|blob| blob_bytes(blob).to_vec()),
    }
}

/// Compiles through System32 `d3dcompiler_47.dll` and returns the bytecode; `None` when the
/// compiler is missing or refuses the source.
pub fn compile_system(request: Request<'_>) -> Option<Vec<u8>> {
    compile_system_checked(request).ok()
}

/// [`compile_system`] with the reason for a refusal: the missing compiler or export, or the
/// failing HRESULT and the compiler's log.
pub fn compile_system_checked(request: Request<'_>) -> Result<Vec<u8>, String> {
    let module = Compiler::system("d3dcompiler_47.dll").ok_or("d3dcompiler_47 is not available")?;
    let entry = module.entry().ok_or("D3DCompile is not available")?;
    let output = unsafe { compile(entry, request, &[], ptr::null_mut()) };
    match output.code {
        Some(code) if output.status >= 0 => Ok(code),
        _ => {
            // E_FAIL when the compiler reported success without bytecode.
            let status = if output.status < 0 {
                output.status as u32
            } else {
                0x8000_4005
            };
            let errors = output.errors.unwrap_or_default();
            let end = errors
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(errors.len());
            Err(format!(
                "compile failed 0x{status:08x} {}",
                String::from_utf8_lossy(&errors[..end])
            ))
        }
    }
}
