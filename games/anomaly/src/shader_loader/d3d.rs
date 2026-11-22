// SPDX-License-Identifier: MIT
use super::{Bytes, Request};
use crate::runtime_module::Module;
use rsf_shader_compiler::{Request as CompileRequest, compile};
use std::collections::HashMap;
use std::ffi::CString;
use std::ffi::{CStr, c_char, c_void};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::os::windows::io::FromRawHandle;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D::{D3D_INCLUDE_TYPE, D3D_SHADER_MACRO, ID3DInclude_Vtbl};
use windows::core::{HRESULT as ComResult, PCSTR};
use windows_sys::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, E_OUTOFMEMORY, FARPROC, HANDLE, S_FALSE, S_OK,
};
use windows_sys::core::{HRESULT, w};

fn allocate<T>(value: T) -> *mut T {
    const { assert!(size_of::<T>() != 0) }
    let raw = unsafe { std::alloc::alloc(std::alloc::Layout::new::<T>()) }.cast::<T>();
    if !raw.is_null() {
        unsafe { raw.write(value) };
    }
    raw
}

struct ModuleExport {
    _module: Module,
    entry: unsafe extern "system" fn() -> isize,
}

impl ModuleExport {
    fn new(module: Module, name: &CStr) -> Option<Self> {
        let entry = module.symbol(name)?;
        Some(Self {
            _module: module,
            entry,
        })
    }
}

#[derive(Default)]
pub(super) struct Modules {
    compiler: Option<ModuleExport>,
    reflector: Option<ModuleExport>,
}

impl Modules {
    pub(super) unsafe fn load(name: *const u16, allow_system_fallback: bool) -> Self {
        if name.is_null() {
            return Self::default();
        }
        let mut compiler = unsafe { Module::loaded(name) };
        if compiler.is_none() && allow_system_fallback {
            compiler = unsafe { Module::system(name) };
        }
        let compiler = compiler.and_then(|module| ModuleExport::new(module, c"D3DCompile"));
        // _43 exposes an older reflection IID. Reflect with _47 for the current SDK interface.
        let reflector = unsafe { Module::system(w!("d3dcompiler_47.dll")) }
            .and_then(|module| ModuleExport::new(module, c"D3DReflect"));
        Self {
            compiler,
            reflector,
        }
    }

    pub(super) fn compile(&self) -> FARPROC {
        self.compiler.as_ref().map(|export| export.entry)
    }

    pub(super) fn reflect(&self) -> FARPROC {
        self.reflector.as_ref().map(|export| export.entry)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_d3d_load(
    name: *const u16,
    compile: &mut FARPROC,
    reflect: &mut FARPROC,
) -> *mut Modules {
    let modules = unsafe { Modules::load(name, true) };
    *compile = modules.compile();
    *reflect = modules.reflect();
    if compile.is_none() && reflect.is_none() {
        return null_mut();
    }
    let owned = allocate(modules);
    if owned.is_null() {
        *compile = None;
        *reflect = None;
    }
    owned
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_d3d_unload(modules: *mut Modules) {
    if !modules.is_null() {
        drop(unsafe { Box::from_raw(modules) });
    }
}

#[derive(Default)]
struct Includes {
    buffers: Mutex<HashMap<usize, Vec<u8>>>,
}

// The native path leaf opens a single candidate, yielding an owned handle on S_OK or S_FALSE
// when the candidate could not be opened.
type OpenFile =
    unsafe extern "C" fn(*const c_void, *const c_char, *const c_char, *mut HANDLE) -> HRESULT;

fn read_include(file: File) -> Result<(Vec<u8>, u32), HRESULT> {
    let max = u64::from(u32::MAX);
    if file.metadata().is_ok_and(|metadata| metadata.len() > max) {
        return Err(E_FAIL);
    }
    let mut bytes = Vec::new();
    // Check the actual read as well, since the file can grow after metadata is read.
    file.take(max + 1).read_to_end(&mut bytes).map_err(|err| {
        if err.kind() == ErrorKind::OutOfMemory {
            E_OUTOFMEMORY
        } else {
            E_FAIL
        }
    })?;
    let len = u32::try_from(bytes.len()).map_err(|_| E_FAIL)?;
    bytes.try_reserve(1).map_err(|_| E_OUTOFMEMORY)?;
    bytes.push(0);
    Ok((bytes, len))
}

impl Includes {
    fn open(
        &self,
        tree: *const c_void,
        name: *const c_char,
        open_file: OpenFile,
    ) -> Result<(*const c_void, u32), HRESULT> {
        // X-Ray r4.cpp:1170 first tries the requested name under r3, then the shared name.
        for prefix in [c"r3".as_ptr(), null()] {
            let mut handle = null_mut();
            match unsafe { open_file(tree, prefix, name, &mut handle) } {
                S_FALSE => continue,
                S_OK => {}
                error => return Err(error),
            }
            // S_OK transfers ownership of a valid file handle from the native leaf.
            let file = unsafe { File::from_raw_handle(handle) };
            let (bytes, len) = read_include(file)?;
            let mut buffers = self.buffers.lock().map_err(|_| E_FAIL)?;
            buffers.try_reserve(1).map_err(|_| E_OUTOFMEMORY)?;
            let data = bytes.as_ptr().cast::<c_void>();
            buffers.insert(data.addr(), bytes);
            return Ok((data, len));
        }
        Err(E_FAIL)
    }
}

#[repr(C)]
struct CompilerIncludes {
    vtable: &'static ID3DInclude_Vtbl,
    tree: *const c_void,
    open_file: OpenFile,
    storage: Includes,
}

static INCLUDE_VTABLE: ID3DInclude_Vtbl = ID3DInclude_Vtbl {
    Open: include_open,
    Close: include_close,
};

unsafe extern "system" fn include_open(
    this: *mut c_void,
    _: D3D_INCLUDE_TYPE,
    name: PCSTR,
    _: *const c_void,
    data: *mut *mut c_void,
    size: *mut u32,
) -> ComResult {
    if name.is_null() || data.is_null() || size.is_null() {
        return ComResult(E_INVALIDARG);
    }
    let this = unsafe { &*this.cast::<CompilerIncludes>() };
    match this.storage.open(this.tree, name.0.cast(), this.open_file) {
        Ok((bytes, len)) => {
            unsafe {
                data.write(bytes.cast_mut());
                size.write(len);
            }
            ComResult(S_OK)
        }
        Err(error) => ComResult(error),
    }
}

unsafe extern "system" fn include_close(this: *mut c_void, data: *const c_void) -> ComResult {
    if data.is_null() {
        return ComResult(S_OK);
    }
    let this = unsafe { &*this.cast::<CompilerIncludes>() };
    let Ok(mut buffers) = this.storage.buffers.lock() else {
        return ComResult(E_FAIL);
    };
    ComResult(if buffers.remove(&data.addr()).is_some() {
        S_OK
    } else {
        E_INVALIDARG
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_compile_tree(
    entry: FARPROC,
    tree: *const c_void,
    open_file: OpenFile,
    request: &Request,
    user: *mut c_void,
    publish: unsafe extern "C" fn(*mut c_void, bool, Bytes) -> bool,
) -> bool {
    let publish = |ok, bytes: &[u8]| unsafe { publish(user, ok, Bytes::from(bytes)) };
    let Some(entry) = entry else {
        return publish(false, b"D3DCompile unavailable");
    };
    let (Ok(function), Ok(target)) = (
        CString::new(unsafe { request.entry.as_slice() }),
        CString::new(unsafe { request.target.as_slice() }),
    ) else {
        return publish(false, b"compiler entry or target contains a NUL");
    };
    let mut defines = Vec::new();
    for d in unsafe { request.defines.as_slice() } {
        let (Ok(name), Ok(value)) = (
            CString::new(unsafe { d.name.as_slice() }),
            CString::new(unsafe { d.value.as_slice() }),
        ) else {
            return publish(false, b"compiler macro contains a NUL");
        };
        defines.push((name, value));
    }
    let mut macros: Vec<_> = defines
        .iter()
        .map(|(name, value)| D3D_SHADER_MACRO {
            Name: PCSTR(name.as_ptr().cast()),
            Definition: PCSTR(value.as_ptr().cast()),
        })
        .collect();
    macros.push(D3D_SHADER_MACRO::default());
    let mut includes = CompilerIncludes {
        vtable: &INCLUDE_VTABLE,
        tree,
        open_file,
        storage: Includes::default(),
    };
    let result = unsafe {
        compile(
            entry as *mut c_void,
            CompileRequest {
                source: request.source.as_slice(),
                name: c"",
                entry: &function,
                target: &target,
                flags: request.flags,
            },
            &macros,
            (&raw mut includes).cast(),
        )
    };
    if let Some(code) = result.code.filter(|_| result.status >= 0) {
        return publish(true, &code);
    }
    let mut error = format!("D3DCompile 0x{}", result.status as u32).into_bytes();
    if let Some(bytes) = result.errors {
        error.extend(b": ");
        error.extend(&bytes[..bytes.len().min(400)]);
    }
    publish(false, &error)
}
