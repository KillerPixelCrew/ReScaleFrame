// SPDX-License-Identifier: MIT
use super::d3d::Modules;
use super::manifest::{Manifest, Root, admit_pack};
use super::{Bytes, Request};
use crate::runtime_module::{export, rsf_anomaly_runtime_module};
use rsf_game_sys::shader_patch_abi::{
    RSF_PATCH_ARGUMENT, RSF_PATCH_OK, RSF_PATCH_SOURCE, RSF_SHADER_PATCH_VERSION,
    rsf_get_shader_patch_api_fn, rsf_patch_bytes, rsf_patch_define, rsf_patch_output,
    rsf_patch_request, rsf_patch_root, rsf_patch_session, rsf_shader_patch_api,
};
use std::ffi::{CStr, CString, OsString, c_char, c_void};
use std::os::windows::ffi::OsStringExt;
use std::ptr::{NonNull, null_mut};
use windows_sys::Win32::Foundation::FARPROC;

struct Session {
    api: rsf_shader_patch_api,
    handle: NonNull<rsf_patch_session>,
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { (self.api.close.unwrap())(self.handle.as_ptr()) };
    }
}

struct RuntimePack {
    session: Session,
    manifest: Manifest,
    includes: Includes,
    // The Runtime's session borrows the compiler. Drop the session before releasing the DLLs.
    modules: Modules,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Includer {
    table: *mut c_void,
    open: unsafe extern "C" fn(*mut c_void, *const c_char, *mut *const c_void, *mut u32) -> bool,
    copy: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool,
    close: unsafe extern "C" fn(*mut c_void, *const c_void),
}

const _: () = assert!(size_of::<Includer>() == 32);

struct OpenInclude<'a> {
    reader: &'a Includer,
    data: *const c_void,
}

impl Drop for OpenInclude<'_> {
    fn drop(&mut self) {
        unsafe { (self.reader.close)(self.reader.table, self.data) };
    }
}

struct Includes {
    reader: Includer,
    bytes: Vec<u8>,
}

unsafe extern "C" fn resolve_include(
    user: *mut c_void,
    name: *const c_char,
    output: *mut rsf_patch_bytes,
) -> i32 {
    if name.is_null() || output.is_null() {
        return RSF_PATCH_SOURCE;
    }
    let includes = unsafe { &mut *user.cast::<Includes>() };
    let mut name = unsafe { CStr::from_ptr(name) }.to_bytes_with_nul().to_vec();
    for byte in &mut name {
        if *byte == b'/' {
            *byte = b'\\';
        }
    }
    let mut data = std::ptr::null();
    let mut size = 0;
    let reader = &includes.reader;
    if !unsafe { (reader.open)(reader.table, name.as_ptr().cast(), &mut data, &mut size) }
        || data.is_null()
    {
        return RSF_PATCH_SOURCE;
    }
    let opened = OpenInclude { reader, data };
    let size = size as usize;
    includes.bytes.clear();
    if includes.bytes.try_reserve(size).is_err() {
        return RSF_PATCH_SOURCE;
    }
    includes.bytes.resize(size, 0);
    if !unsafe { (reader.copy)(includes.bytes.as_mut_ptr().cast(), opened.data, size) } {
        return RSF_PATCH_SOURCE;
    }
    // The Runtime copies these bytes before the next include callback can reuse the buffer.
    unsafe {
        output.write(rsf_patch_bytes {
            data: includes.bytes.as_ptr().cast(),
            size,
        })
    };
    RSF_PATCH_OK
}

fn query_api() -> Option<rsf_shader_patch_api> {
    // The carrier keeps the Runtime loaded, and shader_patch.h specifies this entry.
    let query = unsafe {
        export::<rsf_get_shader_patch_api_fn>(
            rsf_anomaly_runtime_module(),
            c"rsf_get_shader_patch_api".to_bytes_with_nul(),
        )
    }
    .flatten()?;
    let mut api = rsf_shader_patch_api {
        struct_size: size_of::<rsf_shader_patch_api>() as u32,
        abi_version: RSF_SHADER_PATCH_VERSION,
        ..Default::default()
    };
    if unsafe { query(&mut api) } != RSF_PATCH_OK
        || api.abi_version != RSF_SHADER_PATCH_VERSION
        || api.struct_size < size_of::<rsf_shader_patch_api>() as u32
        || api.open.is_none()
        || api.hash.is_none()
        || api.root_count.is_none()
        || api.root.is_none()
        || api.prepare.is_none()
        || api.close.is_none()
        || api.attach_d3d11.is_none()
        || api.create_d3d11.is_none()
        || api.constant_buffer.is_none()
        || api.upload_constant.is_none()
        || api.bind_constant.is_none()
        || api.counters.is_none()
        || api.dump.is_none()
    {
        return None;
    }
    Some(api)
}

unsafe fn wide_path(path: *const u16) -> Option<OsString> {
    if path.is_null() {
        return None;
    }
    // The native filesystem facade lends a NUL-terminated Windows path.
    let len = (0..32768).find(|offset| unsafe { path.add(*offset).read() } == 0)?;
    Some(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(path, len)
    }))
}

unsafe extern "C" fn read_root(source: *const c_void, index: u32, out: *mut Root) -> bool {
    let session = unsafe { &*source.cast::<Session>() };
    let mut root = rsf_patch_root {
        struct_size: size_of::<rsf_patch_root>() as u32,
        abi_version: RSF_SHADER_PATCH_VERSION,
        ..Default::default()
    };
    if unsafe { (session.api.root.unwrap())(session.handle.as_ptr(), index, &mut root) }
        != RSF_PATCH_OK
    {
        return false;
    }
    let text = |value: *const c_char| {
        if value.is_null() {
            Bytes::from(&[][..])
        } else {
            // The Runtime lends its manifest strings for this session.
            Bytes::from(unsafe { CStr::from_ptr(value) }.to_bytes())
        }
    };
    unsafe {
        out.write(Root {
            name: text(root.name),
            original: text(root.source_sha256),
            motion: text(root.patched_sha256),
            tag: text(root.tag),
            stage: root.stage,
        })
    };
    true
}

impl RuntimePack {
    unsafe fn open(
        pack: *const u16,
        cache: *const u16,
        compiler_name: *const u16,
        includer: Includer,
    ) -> Result<Self, String> {
        // Live recompiles use the loaded engine compiler, including its identity in Runtime's cache key.
        let modules = unsafe { Modules::load(compiler_name, false) };
        let Some(compiler) = modules.compile().filter(|_| modules.reflect().is_some()) else {
            return Err(String::from("engine D3DCompiler unavailable"));
        };
        let api = query_api().ok_or_else(|| String::from("Runtime shader service unavailable"))?;
        let path =
            unsafe { wide_path(pack) }.ok_or_else(|| String::from("invalid motion pack path"))?;
        let data =
            std::fs::read(path).map_err(|error| format!("motion pack read refused: {error}"))?;
        let mut handle = null_mut();
        let status = unsafe {
            (api.open.unwrap())(
                rsf_patch_bytes {
                    data: data.as_ptr().cast(),
                    size: data.len(),
                },
                cache,
                compiler as *mut c_void,
                &mut handle,
            )
        };
        // Close any handle the Runtime returned, even if admission failed.
        let session = NonNull::new(handle).map(|handle| Session { api, handle });
        if status != RSF_PATCH_OK {
            return Err(format!("Runtime pack refused: {status}"));
        }
        let session =
            session.ok_or_else(|| String::from("Runtime returned no shader pack session"))?;
        let hash = unsafe { (session.api.hash.unwrap())(session.handle.as_ptr()) };
        let hash = if hash.is_null() {
            &[][..]
        } else {
            unsafe { CStr::from_ptr(hash) }.to_bytes()
        };
        let count = unsafe { (session.api.root_count.unwrap())(session.handle.as_ptr()) };
        let manifest =
            unsafe { admit_pack(hash, count, std::ptr::from_ref(&session).cast(), read_root) }?;
        Ok(Self {
            session,
            manifest,
            includes: Includes {
                reader: includer,
                bytes: Vec::new(),
            },
            modules,
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_runtime_pack_open(
    pack: *const u16,
    cache: *const u16,
    compiler_name: *const u16,
    includer: &Includer,
    error: *mut c_char,
    capacity: usize,
) -> *mut RuntimePack {
    match unsafe { RuntimePack::open(pack, cache, compiler_name, *includer) } {
        Ok(pack) => Box::into_raw(Box::new(pack)),
        Err(message) => {
            if !error.is_null() && capacity != 0 {
                let mut len = message.len().min(capacity - 1);
                while !message.is_char_boundary(len) {
                    len -= 1;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(message.as_ptr(), error.cast(), len);
                    error.add(len).write(0);
                }
            }
            null_mut()
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_runtime_pack_close(pack: *mut RuntimePack) {
    if !pack.is_null() {
        drop(unsafe { Box::from_raw(pack) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_runtime_pack_manifest(pack: &RuntimePack) -> *const Manifest {
    &pack.manifest
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_runtime_pack_api(pack: &RuntimePack) -> *const rsf_shader_patch_api {
    &pack.session.api
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_runtime_pack_reflector(pack: &RuntimePack) -> FARPROC {
    pack.modules.reflect()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_runtime_pack_prepare(
    pack: &mut RuntimePack,
    input: &Request,
    output: &mut rsf_patch_output,
) -> i32 {
    *output = rsf_patch_output {
        struct_size: size_of::<rsf_patch_output>() as u32,
        abi_version: RSF_SHADER_PATCH_VERSION,
        ..Default::default()
    };
    let (Ok(entry), Ok(target)) = (
        CString::new(unsafe { input.entry.as_slice() }),
        CString::new(unsafe { input.target.as_slice() }),
    ) else {
        return RSF_PATCH_ARGUMENT;
    };
    let defines = unsafe { input.defines.as_slice() };
    let mut text = Vec::with_capacity(defines.len());
    for define in defines {
        let (Ok(name), Ok(value)) = (
            CString::new(unsafe { define.name.as_slice() }),
            CString::new(unsafe { define.value.as_slice() }),
        ) else {
            return RSF_PATCH_ARGUMENT;
        };
        text.push((name, value));
    }
    let defines: Vec<_> = text
        .iter()
        .map(|(name, value)| rsf_patch_define {
            name: name.as_ptr(),
            value: value.as_ptr(),
        })
        .collect();
    let request = rsf_patch_request {
        struct_size: size_of::<rsf_patch_request>() as u32,
        abi_version: RSF_SHADER_PATCH_VERSION,
        source: rsf_patch_bytes {
            data: input.source.data.cast(),
            size: input.source.len,
        },
        original_bytecode: rsf_patch_bytes {
            data: input.bytecode.data.cast(),
            size: input.bytecode.len,
        },
        entry: entry.as_ptr(),
        target: target.as_ptr(),
        flags: input.flags,
        defines: defines.as_ptr(),
        define_count: defines.len(),
        resolve: Some(resolve_include),
        user: std::ptr::from_mut(&mut pack.includes).cast(),
    };
    unsafe { (pack.session.api.prepare.unwrap())(pack.session.handle.as_ptr(), &request, output) }
}
