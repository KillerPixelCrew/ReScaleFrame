// SPDX-License-Identifier: MIT
use super::reflection::{Snapshot, reflect};
use super::{
    Bytes, Compatibility, Entry, Manifest, Outcome, Request, Span, prepare_tree,
    rsf_anomaly_shader_match_request, rsf_anomaly_shader_skin_mode,
};
use crate::runtime_module::Module;
use rsf_shader_compiler::{Request as CompileRequest, compile};
use std::collections::HashMap;
use std::ffi::CString;
use std::ffi::{CStr, OsString, c_void};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::windows::{ffi::OsStringExt, fs::OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Mutex;
use windows::Win32::Graphics::Direct3D::{D3D_INCLUDE_TYPE, D3D_SHADER_MACRO, ID3DInclude_Vtbl};
use windows::core::{HRESULT as ComResult, PCSTR};
use windows_sys::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_OUTOFMEMORY, FARPROC, S_OK};
use windows_sys::Win32::Globalization::{
    CP_ACP, CP_OEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar,
};
use windows_sys::Win32::Storage::FileSystem::{AreFileApisANSI, FILE_SHARE_READ, FILE_SHARE_WRITE};
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
unsafe extern "C" fn rsf_anomaly_shader_d3d_load(name: *const u16) -> *mut Modules {
    let modules = unsafe { Modules::load(name, true) };
    if modules.compile().is_none() || modules.reflect().is_none() {
        return null_mut();
    }
    allocate(modules)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_d3d_unload(modules: *mut Modules) {
    if !modules.is_null() {
        drop(unsafe { Box::from_raw(modules) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_d3d_reflect(
    modules: &Modules,
    code: Bytes,
) -> *mut Snapshot {
    reflect(modules.reflect(), unsafe { code.as_slice() }).map_or(null_mut(), allocate)
}

#[derive(Default)]
struct Includes {
    buffers: Mutex<HashMap<usize, Vec<u8>>>,
}

// The deployed trees are Windows paths. The engine's includer names arrive as narrow bytes, which the
// previous native leaf converted with the STL's file-name rules: the ANSI or OEM code page chosen by
// AreFileApisANSI, strict decoding and preferred separators. Appending follows the same drive/UNC/root
// replacement rules as Path::join.
fn include_name(name: &CStr) -> Result<PathBuf, HRESULT> {
    let narrow = name.to_bytes();
    if narrow.is_empty() {
        return Ok(PathBuf::new());
    }
    let code_page = if unsafe { AreFileApisANSI() } != 0 {
        CP_ACP
    } else {
        CP_OEMCP
    };
    let length = i32::try_from(narrow.len()).map_err(|_| E_FAIL)?;
    let wide_length = unsafe {
        MultiByteToWideChar(
            code_page,
            MB_ERR_INVALID_CHARS,
            narrow.as_ptr(),
            length,
            null_mut(),
            0,
        )
    };
    let capacity = usize::try_from(wide_length)
        .ok()
        .filter(|capacity| *capacity != 0)
        .ok_or(E_FAIL)?;
    let mut wide = Vec::new();
    wide.try_reserve_exact(capacity)
        .map_err(|_| E_OUTOFMEMORY)?;
    wide.resize(capacity, 0u16);
    let written = unsafe {
        MultiByteToWideChar(
            code_page,
            MB_ERR_INVALID_CHARS,
            narrow.as_ptr(),
            length,
            wide.as_mut_ptr(),
            wide_length,
        )
    };
    if written != wide_length {
        return Err(E_FAIL);
    }
    for unit in &mut wide {
        if *unit == u16::from(b'/') {
            *unit = u16::from(b'\\');
        }
    }
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

// Read access with shared read and write, as the previous CreateFileW call. None when the
// candidate could not be opened.
fn open_candidate(tree: &Path, prefix: Option<&str>, name: &CStr) -> Result<Option<File>, HRESULT> {
    let mut path = tree.to_path_buf();
    if let Some(prefix) = prefix {
        path.push(prefix);
    }
    path.push(include_name(name)?);
    Ok(OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
        .ok())
}

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
    fn open(&self, tree: &Path, name: &CStr) -> Result<(*const c_void, u32), HRESULT> {
        // X-Ray r4.cpp:1170 first tries the requested name under r3, then the shared name.
        for prefix in [Some("r3"), None] {
            let Some(file) = open_candidate(tree, prefix, name)? else {
                continue;
            };
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
struct CompilerIncludes<'a> {
    vtable: &'static ID3DInclude_Vtbl,
    tree: &'a Path,
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
    match this
        .storage
        .open(this.tree, unsafe { CStr::from_ptr(name.0.cast()) })
    {
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

fn compile_tree(
    entry: FARPROC,
    tree: &Path,
    request: &Request,
    source: &[u8],
) -> Result<Vec<u8>, Vec<u8>> {
    let Some(entry) = entry else {
        return Err(b"D3DCompile unavailable".to_vec());
    };
    let (Ok(function), Ok(target)) = (
        CString::new(unsafe { request.entry.as_slice() }),
        CString::new(unsafe { request.target.as_slice() }),
    ) else {
        return Err(b"compiler entry or target contains a NUL".to_vec());
    };
    let mut defines = Vec::new();
    for d in unsafe { request.defines.as_slice() } {
        let (Ok(name), Ok(value)) = (
            CString::new(unsafe { d.name.as_slice() }),
            CString::new(unsafe { d.value.as_slice() }),
        ) else {
            return Err(b"compiler macro contains a NUL".to_vec());
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
        storage: Includes::default(),
    };
    let result = unsafe {
        compile(
            entry as *mut c_void,
            CompileRequest {
                source,
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
        return Ok(code);
    }
    let mut error = format!("D3DCompile 0x{}", result.status as u32).into_bytes();
    if let Some(bytes) = result.errors {
        error.extend(b": ");
        error.extend(&bytes[..bytes.len().min(400)]);
    }
    Err(error)
}

type PublishCompile = unsafe extern "C" fn(*mut c_void, bool, Bytes) -> bool;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_d3d_compile(
    modules: &Modules,
    tree: Span<u16>,
    request: &Request,
    user: *mut c_void,
    publish: PublishCompile,
) -> bool {
    let source = unsafe { request.source.as_slice() };
    let tree = PathBuf::from(OsString::from_wide(unsafe { tree.as_slice() }));
    match compile_tree(modules.compile(), &tree, request, source) {
        Ok(code) => unsafe { publish(user, true, Bytes::from(code.as_slice())) },
        Err(error) => unsafe { publish(user, false, Bytes::from(error.as_slice())) },
    }
}

// Compiler and reflection snapshots stay owned by Rust. The fixture projects the
// immutable result into its existing assertions during one synchronous callback.
type PublishPrepared = unsafe extern "C" fn(
    *mut c_void,
    &Entry,
    Outcome,
    Compatibility,
    i32,
    Bytes,
    Bytes,
    *mut Snapshot,
    *mut Snapshot,
);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_shader_d3d_prepare(
    modules: &Modules,
    manifest: &Manifest,
    request: &Request,
    deployment: Span<u16>,
    user: *mut c_void,
    stock_sink: Option<PublishCompile>,
    publish: PublishPrepared,
) {
    let deployment = PathBuf::from(OsString::from_wide(unsafe { deployment.as_slice() }));
    let (original, motion) = (deployment.join("original"), deployment.join("motion"));
    // Ordinary fixture builds use two independent original-tree compiles. The
    // first supplies the simulated engine bytecode, the second proves it.
    // Refusal cases supply their own mutated or foreign engine bytecode.
    let stock;
    let replay;
    let request = if let Some(sink) = stock_sink {
        let source = unsafe { request.source.as_slice() };
        match compile_tree(modules.compile(), &original, request, source) {
            Ok(bytes) => {
                stock = bytes;
                if !unsafe { sink(user, true, Bytes::from(stock.as_slice())) } {
                    return;
                }
            }
            Err(error) => {
                unsafe { sink(user, false, Bytes::from(error.as_slice())) };
                return;
            }
        }
        replay = Request {
            target: request.target,
            entry: request.entry,
            flags: request.flags,
            defines: request.defines,
            source: request.source,
            bytecode: Bytes::from(stock.as_slice()),
        };
        &replay
    } else {
        request
    };
    let Some(entry) = (unsafe {
        rsf_anomaly_shader_match_request(Some(manifest), request.source, request.target)
    }) else {
        return;
    };
    // Entry spans come from immutable manifest-owned backing storage.
    let id = entry.identity(manifest.namespace());
    let result = prepare_tree(
        request,
        &id,
        &deployment,
        None,
        |_, code| reflect(modules.reflect(), code),
        Snapshot::view,
        |patched, code| {
            compile_tree(
                modules.compile(),
                if patched { &motion } else { &original },
                request,
                code,
            )
        },
    );
    unsafe {
        publish(
            user,
            entry,
            result.outcome,
            result.compatibility,
            rsf_anomaly_shader_skin_mode(request.defines),
            Bytes::from(result.detail.as_slice()),
            Bytes::from(result.bytes.as_slice()),
            result.stock.map_or(null_mut(), allocate),
            result.reflection.map_or(null_mut(), allocate),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn include_names_use_preferred_separators() {
        assert_eq!(
            include_name(c"dx11/tess.hs").unwrap(),
            PathBuf::from(r"dx11\tess.hs")
        );
        assert_eq!(
            include_name(c"common.h").unwrap(),
            PathBuf::from("common.h")
        );
        assert_eq!(include_name(c"").unwrap(), PathBuf::new());
    }

    fn contents(includes: &Includes, tree: &Path, name: &CStr) -> Result<Vec<u8>, HRESULT> {
        let (data, len) = includes.open(tree, name)?;
        Ok(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len as usize) }.to_vec())
    }

    #[test]
    fn include_lookup_tries_r3_before_the_shared_tree() {
        let tree =
            std::env::temp_dir().join(format!("rsf-anomaly-includes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tree);
        fs::create_dir_all(tree.join("r3").join("dx11")).unwrap();
        fs::create_dir_all(tree.join("dx11")).unwrap();
        fs::write(tree.join("r3").join("both.h"), b"r3").unwrap();
        fs::write(tree.join("both.h"), b"shared").unwrap();
        fs::write(tree.join("shared_only.h"), b"shared only").unwrap();
        fs::write(tree.join("r3").join("dx11").join("tess.h"), b"nested").unwrap();
        let includes = Includes::default();
        assert_eq!(contents(&includes, &tree, c"both.h").unwrap(), b"r3");
        assert_eq!(
            contents(&includes, &tree, c"shared_only.h").unwrap(),
            b"shared only"
        );
        assert_eq!(
            contents(&includes, &tree, c"dx11/tess.h").unwrap(),
            b"nested"
        );
        // A directory or a missing file never opens as an include.
        assert_eq!(contents(&includes, &tree, c"dx11").unwrap_err(), E_FAIL);
        assert_eq!(
            contents(&includes, &tree, c"missing.h").unwrap_err(),
            E_FAIL
        );
        let _ = fs::remove_dir_all(&tree);
    }
}
