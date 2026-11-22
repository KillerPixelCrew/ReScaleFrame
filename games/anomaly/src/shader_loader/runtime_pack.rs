// SPDX-License-Identifier: MIT
use super::d3d::Modules;
use super::manifest::{Manifest, Root, admit_pack};
use super::reflection::{Snapshot, reflect};
use super::{
    Bytes, Compatibility, EngineIncludeOps, EngineShaderProfile, Outcome, Request,
    rsf_anomaly_shader_compatible, rsf_anomaly_shader_skin_mode, runtime_refusal,
};
use crate::motion_device::ConstantOps;
use crate::runtime_module::{export, rsf_anomaly_runtime_module};
use rsf_game_sys::shader_patch_abi::{
    RSF_PATCH_ARGUMENT, RSF_PATCH_OK, RSF_PATCH_SOURCE, RSF_SHADER_PATCH_VERSION,
    rsf_get_shader_patch_api_fn, rsf_patch_bytes, rsf_patch_counters, rsf_patch_define,
    rsf_patch_output, rsf_patch_request, rsf_patch_root, rsf_patch_session, rsf_shader_patch_api,
};
use std::ffi::{CStr, CString, OsString, c_char, c_void};
use std::os::windows::ffi::OsStringExt;
use std::ptr::{NonNull, null_mut};

struct Session {
    api: rsf_shader_patch_api,
    handle: NonNull<rsf_patch_session>,
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { (self.api.close.unwrap())(self.handle.as_ptr()) };
    }
}

pub(crate) struct RuntimePack {
    session: Session,
    manifest: Manifest,
    constant_ops: ConstantOps,
    includes: Includes,
    // The Runtime's session borrows the compiler. Drop the session before releasing the DLLs.
    modules: Modules,
}

struct Includer {
    table: *mut c_void,
    open: IncludeOpen,
    copy: GuardedCopy,
    close: IncludeClose,
}

type IncludeOpen =
    unsafe extern "C" fn(*mut c_void, *const c_char, *mut *const c_void, *mut u32) -> bool;
type IncludeClose = unsafe extern "C" fn(*mut c_void, *const c_void);

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
        let constant_ops = ConstantOps::from_api(&api)
            .ok_or_else(|| String::from("pack constant operations unavailable"))?;
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
            constant_ops,
            includes: Includes {
                reader: includer,
                bytes: Vec::new(),
            },
            modules,
        })
    }
}

type GuardedCopy = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool;

fn read_bytes<const N: usize>(read: GuardedCopy, address: usize) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    unsafe { read(bytes.as_mut_ptr().cast(), address as *const c_void, N) }.then_some(bytes)
}

fn includer_table(image: usize, setup_rva: u32, read: GuardedCopy) -> Result<usize, &'static str> {
    // Standard/AVX share the LEA form and vtable entry prefixes, with distinct profiled RVAs.
    let instruction = || {
        let lea = image.checked_add(setup_rva as usize)?.checked_add(6)?;
        let [0x48, 0x8d, 0x05, a, b, c, d] = read_bytes::<7>(read, lea)? else {
            return None;
        };
        let displacement = i32::from_le_bytes([a, b, c, d]) as isize;
        lea.checked_add(7)?.checked_add_signed(displacement)
    };
    let table = instruction().ok_or("include table instruction")?;
    let entries = || {
        let open = usize::from_le_bytes(read_bytes(read, table)?);
        let close = usize::from_le_bytes(read_bytes(read, table.checked_add(size_of::<usize>())?)?);
        Some(
            read_bytes::<9>(read, open)? == [0x40, 0x53, 0x48, 0x81, 0xec, 0x40, 0x02, 0x00, 0x00]
                && read_bytes::<8>(read, close)?
                    == [0x48, 0x83, 0xec, 0x28, 0x48, 0x85, 0xd2, 0x74],
        )
    };
    if entries() == Some(true) {
        Ok(table)
    } else {
        Err("engine includer entries")
    }
}

impl RuntimePack {
    /// The profile borrows the engine's module name and supplies its confirmed setup RVA.
    pub(crate) unsafe fn open_engine(
        pack: &[u16],
        cache: &[u16],
        image: usize,
        profile: EngineShaderProfile,
        ops: &EngineIncludeOps,
    ) -> Result<Self, String> {
        let (Some(open), Some(copy), Some(close)) = (ops.open, ops.copy, ops.close) else {
            return Err(String::from("engine include operations unavailable"));
        };
        let table = includer_table(image, profile.setup_rva, copy)?;
        unsafe {
            Self::open(
                pack.as_ptr(),
                cache.as_ptr(),
                profile.compiler_name,
                Includer {
                    table: table as *mut c_void,
                    open,
                    copy,
                    close,
                },
            )
        }
    }
}

pub(crate) struct RuntimeVariant {
    pub(crate) stock: Snapshot,
    pub(crate) reflection: Snapshot,
    pub(crate) bytecode: Vec<u8>,
    pub(crate) skin: i32,
    pub(crate) outcome: u8,
}

impl RuntimePack {
    pub(crate) fn constant_ops(&self) -> ConstantOps {
        self.constant_ops
    }

    pub(crate) fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    // The stage guarantees this is a live borrowed D3D11 device.
    pub(crate) unsafe fn attach(&self, device: *mut c_void) -> i32 {
        unsafe { (self.session.api.attach_d3d11.unwrap())(device) }
    }

    pub(crate) fn dump(&self, path: &[u16]) -> i32 {
        assert_eq!(path.last(), Some(&0));
        unsafe { (self.session.api.dump.unwrap())(path.as_ptr()) }
    }

    pub(crate) fn counters(&self) -> rsf_patch_counters {
        let mut counters = rsf_patch_counters {
            struct_size: size_of::<rsf_patch_counters>() as u32,
            abi_version: RSF_SHADER_PATCH_VERSION,
            ..Default::default()
        };
        unsafe { (self.session.api.counters.unwrap())(&mut counters) };
        counters
    }

    pub(crate) unsafe fn prepare(
        &mut self,
        input: &Request,
        stage: u8,
    ) -> Result<RuntimeVariant, (u8, String)> {
        let mut output = rsf_patch_output::default();
        let status = unsafe { prepare_bytecode(self, input, &mut output) };
        // The v1 Runtime copies diagnostics into the fixed output array.
        let detail = output.reason.map(|c| c as u8);
        let refusal = |outcome: Outcome| {
            let detail = CStr::from_bytes_until_nul(&detail).map_or(&detail[..], CStr::to_bytes);
            (outcome as u8, String::from_utf8_lossy(detail).into_owned())
        };
        if let Some(outcome) = runtime_refusal(status) {
            return Err(refusal(outcome));
        }
        let incompatible = || refusal(Outcome::ReflectionMismatch);
        let reflector = self.modules.reflect();
        let stock =
            reflect(reflector, unsafe { input.bytecode.as_slice() }).ok_or_else(incompatible)?;
        let code = if output.bytecode.data.is_null() || output.bytecode.size == 0 {
            return Err(incompatible());
        } else {
            // The session owns the bytes until its next prepare or close. Copy the
            // result here before any other session operation can reuse that storage.
            unsafe { std::slice::from_raw_parts(output.bytecode.data.cast(), output.bytecode.size) }
        };
        let reflection = reflect(reflector, code).ok_or_else(incompatible)?;
        if rsf_anomaly_shader_compatible(stage, stock.view(), reflection.view())
            != Compatibility::Ok
        {
            return Err(incompatible());
        }
        Ok(RuntimeVariant {
            stock,
            reflection,
            bytecode: code.to_vec(),
            skin: unsafe { rsf_anomaly_shader_skin_mode(input.defines) },
            outcome: if output.cache_hit != 0 {
                Outcome::ReplacedFromCache as u8
            } else {
                Outcome::Replaced as u8
            },
        })
    }

    pub(crate) unsafe fn create(
        &self,
        device: *mut c_void,
        target: &CStr,
        code: &[u8],
        shader: &mut *mut c_void,
    ) -> i32 {
        unsafe {
            (self.session.api.create_d3d11.unwrap())(
                device,
                target.as_ptr(),
                rsf_patch_bytes {
                    data: code.as_ptr().cast(),
                    size: code.len(),
                },
                shader,
            )
        }
    }
}

unsafe fn prepare_bytecode(
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
