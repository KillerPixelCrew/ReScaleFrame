// SPDX-License-Identifier: MIT
//! D3DCompiler ownership for standalone helpers and engine-supplied compiler calls.
#![cfg(windows)]

#[cfg(feature = "cpp-entry")]
use std::ffi::c_char;
use std::ffi::{CStr, c_void};
use std::ptr;
use windows::Win32::Graphics::Direct3D::{D3D_SHADER_MACRO, ID3DBlob};
use windows::core::{HRESULT, PCSTR};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

#[cfg(feature = "cpp-entry")]
const E_FAIL: i32 = 0x8000_4005_u32 as i32;

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
        let name: Vec<_> = name.encode_utf16().chain([0]).collect();
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

#[cfg(feature = "cpp-entry")]
type Sink = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32;

// The caller copies bytes during the callbacks. Memory never crosses allocator boundaries.
#[cfg(feature = "cpp-entry")]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_compile_system_shader(
    source: *const u8,
    size: usize,
    name: *const c_char,
    entry: *const c_char,
    target: *const c_char,
    flags: u32,
    user: *mut c_void,
    code: Sink,
    errors: Sink,
) -> i32 {
    let Some(module) = Compiler::system("d3dcompiler_47.dll") else {
        return 1;
    };
    let Some(compiler) = module.entry() else {
        return 2;
    };
    let output = unsafe {
        compile(
            compiler,
            Request {
                source: std::slice::from_raw_parts(source, size),
                name: CStr::from_ptr(name),
                entry: CStr::from_ptr(entry),
                target: CStr::from_ptr(target),
                flags,
            },
            &[],
            ptr::null_mut(),
        )
    };
    if output.status < 0 || output.code.is_none() {
        if let Some(bytes) = &output.errors {
            unsafe { errors(user, bytes.as_ptr(), bytes.len()) };
        }
        return if output.status < 0 {
            output.status
        } else {
            E_FAIL
        };
    }
    let bytes = output.code.as_deref().expect("code checked");
    if unsafe { code(user, bytes.as_ptr(), bytes.len()) } == 0 {
        0
    } else {
        E_FAIL
    }
}
