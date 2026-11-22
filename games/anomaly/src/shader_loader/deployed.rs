// SPDX-License-Identifier: MIT
//! Deployed-tree shader preparation for the standalone motion device fixtures.
//! The fixtures compile the stock permutation from `deployment/original`, hand it in as the engine
//! bytecode and prepare its variant from `deployment/motion` through the production tree path.
use super::d3d::Modules;
use super::reflection::{Snapshot, reflect, rsf_anomaly_shader_snapshot_keeps_buffer_sizes};
use super::tree::{prepare_tree, rsf_anomaly_shader_match_request};
use super::{Bytes, Define, Manifest, Request, Span, manifest, rsf_anomaly_shader_skin_mode};
use rsf_shader_compiler::{Request as CompileRequest, compile};
use std::collections::HashMap;
use std::ffi::{CStr, CString, OsString, c_void};
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
use windows_sys::core::HRESULT;

pub(crate) const NOT_ALLOWLISTED: u8 = 0;
pub(crate) const REPLACED: u8 = 1;
pub(crate) const REPLACED_FROM_CACHE: u8 = 2;
pub(crate) const ORIGINAL_MISMATCH: u8 = 6;

/// One permutation as `CResourceManager` asks for it: entry `main`, row-major packing.
pub(crate) struct ShaderRequest<'a> {
    pub(crate) target: &'a str,
    pub(crate) defines: &'a [(String, String)],
    pub(crate) source: &'a [u8],
}

pub(crate) struct Prepared {
    pub(crate) outcome: u8,
    pub(crate) family: Option<u8>,
    /// -1 SKIN_NONE, 0..4 SKIN_n, -2 none of the skin defines.
    pub(crate) skin: i32,
    pub(crate) variant: Vec<u8>,
    /// Reflection of the variant.
    pub(crate) reflection: Option<Snapshot>,
    /// Reflection of the engine bytecode.
    pub(crate) stock: Option<Snapshot>,
    pub(crate) detail: String,
}

impl Default for Prepared {
    fn default() -> Self {
        Self {
            outcome: NOT_ALLOWLISTED,
            family: None,
            skin: -2,
            variant: Vec::new(),
            reflection: None,
            stock: None,
            detail: String::new(),
        }
    }
}

pub(crate) fn outcome_name(outcome: u8) -> &'static str {
    super::outcome_name(outcome)
}

pub(crate) fn parse_manifest(text: &[u8]) -> Result<Manifest, String> {
    manifest::parse(text)
}

pub(crate) fn keeps_buffer_sizes(stock: &Snapshot, variant: &Snapshot) -> bool {
    rsf_anomaly_shader_snapshot_keeps_buffer_sizes(Some(stock), Some(variant))
}

/// D3D10_SHADER_PACK_MATRIX_ROW_MAJOR, what the engine passes for every stage.
const FLAGS: u32 = 8;

fn with_request<R>(
    input: &ShaderRequest<'_>,
    bytecode: &[u8],
    run: impl FnOnce(&Request) -> R,
) -> R {
    let defines: Vec<Define> = input
        .defines
        .iter()
        .map(|(name, value)| Define {
            name: Bytes::from(name.as_bytes()),
            value: Bytes::from(value.as_bytes()),
        })
        .collect();
    let request = Request {
        target: Bytes::from(input.target.as_bytes()),
        entry: Bytes::from(&b"main"[..]),
        flags: FLAGS,
        defines: Span::from(defines.as_slice()),
        source: Bytes::from(input.source),
        bytecode: Bytes::from(bytecode),
    };
    run(&request)
}

/// The engine's compiler and the system reflector, loaded once per fixture.
pub(crate) struct Deployed {
    modules: Modules,
}

impl Deployed {
    /// `compiler` is a NUL-terminated UTF-16 module name.
    pub(crate) fn load(compiler: &[u16]) -> Option<Self> {
        if compiler.last() != Some(&0) {
            return None;
        }
        let modules = unsafe { Modules::load(compiler.as_ptr(), true) };
        (modules.compile().is_some() && modules.reflect().is_some()).then_some(Self { modules })
    }

    pub(crate) fn compile(
        &self,
        tree: &Path,
        input: &ShaderRequest<'_>,
    ) -> Result<Vec<u8>, String> {
        with_request(input, &[], |request| {
            compile_tree(self.modules.compile(), tree, request, input.source)
        })
        .map_err(|error| String::from_utf8_lossy(&error).into_owned())
    }

    pub(crate) fn reflect(&self, code: &[u8]) -> Option<Snapshot> {
        reflect(self.modules.reflect(), code)
    }

    /// Prepares the motion variant for `bytecode`, the simulated engine bytecode of `input`.
    pub(crate) fn prepare(
        &self,
        manifest: &Manifest,
        deployment: &Path,
        input: &ShaderRequest<'_>,
        bytecode: &[u8],
    ) -> Prepared {
        with_request(input, bytecode, |request| {
            let Some(entry) = (unsafe {
                rsf_anomaly_shader_match_request(Some(manifest), request.source, request.target)
            }) else {
                return Prepared::default();
            };
            let id = entry.identity(manifest.namespace());
            let (original, motion) = (deployment.join("original"), deployment.join("motion"));
            let result = prepare_tree(
                request,
                &id,
                deployment,
                None,
                |_, code| reflect(self.modules.reflect(), code),
                Snapshot::view,
                |patched, code| {
                    let tree = if patched { &motion } else { &original };
                    compile_tree(self.modules.compile(), tree, request, code)
                },
            );
            Prepared {
                outcome: result.outcome as u8,
                family: Some(entry.family),
                skin: unsafe { rsf_anomaly_shader_skin_mode(request.defines) },
                variant: result.bytes,
                reflection: result.reflection,
                stock: result.stock,
                detail: String::from_utf8_lossy(&result.detail).into_owned(),
            }
        })
    }

    /// Two independent original-tree compiles: the first supplies the engine bytecode, the second
    /// (inside `prepare`) proves it before the variant is built.
    pub(crate) fn build(
        &self,
        manifest: &Manifest,
        deployment: &Path,
        input: &ShaderRequest<'_>,
    ) -> Result<(Vec<u8>, Prepared), String> {
        let stock = self.compile(&deployment.join("original"), input)?;
        let prepared = self.prepare(manifest, deployment, input, &stock);
        Ok((stock, prepared))
    }
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
