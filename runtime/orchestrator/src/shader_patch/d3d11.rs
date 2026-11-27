// SPDX-License-Identifier: MIT
//! D3D11 creation interception, the bytecode inventory and generic constant buffers.

use super::{ARGUMENT, COMPILE, COUNTERS, INTERNAL, OK, guarded, lock, sha256, wide_path};
use crate::sys::rsf_patch_bytes;
use rsf_minhook_sys::{
    MH_CreateHook, MH_DisableHook, MH_ERROR_ALREADY_INITIALIZED, MH_ERROR_ENABLED, MH_EnableHook,
    MH_Initialize, MH_OK, MH_RemoveHook,
};
use rsf_shader_compiler::blob_bytes;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_void};
use std::fs;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3D_DISASM_ENABLE_INSTRUCTION_NUMBERING, D3DDisassemble,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::{Interface, PCSTR};

type Create = unsafe extern "system" fn(
    *mut c_void,
    *const c_void,
    usize,
    *mut c_void,
    *mut *mut c_void,
) -> i32;

// ID3D11Device vtable slots of CreateVertex, Pixel, Geometry, Hull, Domain and ComputeShader.
const SLOTS: [usize; 6] = [12, 15, 13, 16, 17, 18];
const STAGES: &[u8] = b"vpghdc";

static ORIGINALS: [AtomicPtr<c_void>; 6] = [const { AtomicPtr::new(ptr::null_mut()) }; 6];
// Hooked creation entry points, recorded as addresses. Zero while detached.
struct Hooks {
    targets: [usize; 6],
    active: bool,
}
static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    targets: [0; 6],
    active: false,
});
// First bytecode seen for each SHA-256, for authoring dumps.
static INVENTORY: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());

thread_local! {
    // Set while the service creates its own replacement, which is not an engine creation.
    static INTERNAL_CREATE: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn create<const I: usize>(
    device: *mut c_void,
    code: *const c_void,
    size: usize,
    linkage: *mut c_void,
    out: *mut *mut c_void,
) -> i32 {
    if !INTERNAL_CREATE.get() {
        COUNTERS.intercepted.fetch_add(1, Ordering::Relaxed);
        if !code.is_null() && size != 0 {
            let _ = std::panic::catch_unwind(|| {
                let bytes = unsafe { std::slice::from_raw_parts(code.cast::<u8>(), size) };
                if let Ok(hash) = sha256(bytes) {
                    lock(&INVENTORY)
                        .entry(hash)
                        .or_insert_with(|| bytes.to_vec());
                }
            });
        }
    }
    let original: Create = unsafe { std::mem::transmute(ORIGINALS[I].load(Ordering::Acquire)) };
    unsafe { original(device, code, size, linkage, out) }
}

const DETOURS: [Create; 6] = [
    create::<0>,
    create::<1>,
    create::<2>,
    create::<3>,
    create::<4>,
    create::<5>,
];

/// Hooks the six creation entry points once. Later devices must share the same entries.
pub(super) unsafe extern "C" fn attach(device: *mut c_void) -> i32 {
    if device.is_null() {
        return ARGUMENT;
    }
    let mut hooks = lock(&HOOKS);
    let vtable = unsafe { *device.cast::<*const usize>() };
    let entries = SLOTS.map(|slot| unsafe { *vtable.add(slot) });
    if hooks.targets[0] != 0 {
        if hooks.targets != entries {
            return ARGUMENT;
        }
        if hooks.active {
            return OK;
        }
    } else {
        if !matches!(
            unsafe { MH_Initialize() },
            MH_OK | MH_ERROR_ALREADY_INITIALIZED
        ) {
            return INTERNAL;
        }
        for (i, (&entry, detour)) in entries.iter().zip(DETOURS).enumerate() {
            let mut original = ptr::null_mut();
            if unsafe { MH_CreateHook(entry as *mut c_void, detour as *mut c_void, &mut original) }
                != MH_OK
            {
                for &created in &entries[..i] {
                    // These hooks have never been enabled.
                    unsafe { MH_RemoveHook(created as *mut c_void) };
                }
                return INTERNAL;
            }
            ORIGINALS[i].store(original, Ordering::Release);
        }
        hooks.targets = entries;
    }
    for (i, &entry) in entries.iter().enumerate() {
        if !matches!(
            unsafe { MH_EnableHook(entry as *mut c_void) },
            MH_OK | MH_ERROR_ENABLED
        ) {
            for &enabled in &entries[..i] {
                unsafe { MH_DisableHook(enabled as *mut c_void) };
            }
            // A detour already on another thread can still call its trampoline. Keep
            // every trampoline and retry these same targets on the next attach.
            return INTERNAL;
        }
    }
    hooks.active = true;
    OK
}

/// Creates a replacement shader without counting it as an engine creation.
pub(super) unsafe extern "C" fn create_shader(
    device: *mut c_void,
    target: *const c_char,
    code: rsf_patch_bytes,
    shader: *mut *mut c_void,
) -> i32 {
    if !shader.is_null() {
        unsafe { *shader = ptr::null_mut() };
    }
    if device.is_null()
        || target.is_null()
        || code.data.is_null()
        || code.size == 0
        || shader.is_null()
    {
        return ARGUMENT;
    }
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    let code = unsafe { std::slice::from_raw_parts(code.data.cast::<u8>(), code.size) };
    INTERNAL_CREATE.set(true);
    let result = unsafe {
        match *target.cast::<u8>() {
            b'v' => device.CreateVertexShader(code, None, Some(shader.cast())),
            b'p' => device.CreatePixelShader(code, None, Some(shader.cast())),
            b'g' => device.CreateGeometryShader(code, None, Some(shader.cast())),
            b'h' => device.CreateHullShader(code, None, Some(shader.cast())),
            b'd' => device.CreateDomainShader(code, None, Some(shader.cast())),
            b'c' => device.CreateComputeShader(code, None, Some(shader.cast())),
            _ => Err(windows::core::Error::empty()),
        }
    };
    INTERNAL_CREATE.set(false);
    if result.is_ok() { OK } else { COMPILE }
}

pub(super) unsafe extern "C" fn constant_buffer(
    device: *mut c_void,
    size: u32,
    out: *mut *mut c_void,
) -> i32 {
    if !out.is_null() {
        unsafe { *out = ptr::null_mut() };
    }
    if device.is_null() || out.is_null() || size == 0 || !size.is_multiple_of(16) || size > 65536 {
        return ARGUMENT;
    }
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    match unsafe { device.CreateBuffer(&desc, None, Some(out.cast())) } {
        Ok(()) => OK,
        Err(_) => INTERNAL,
    }
}

/// Replaces a whole dynamic, CPU-writable buffer with WRITE_DISCARD.
pub(super) unsafe extern "C" fn upload_constant(
    context: *mut c_void,
    buffer: *mut c_void,
    data: rsf_patch_bytes,
) -> i32 {
    if context.is_null() || buffer.is_null() || data.data.is_null() {
        return ARGUMENT;
    }
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.unwrap();
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }.unwrap();
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) };
    let writable = desc.Usage == D3D11_USAGE_DYNAMIC
        && desc.CPUAccessFlags & D3D11_CPU_ACCESS_WRITE.0 as u32 != 0;
    if data.size != desc.ByteWidth as usize || !writable {
        return ARGUMENT;
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }.is_err() {
        return INTERNAL;
    }
    if !mapped.pData.is_null() {
        unsafe { ptr::copy_nonoverlapping(data.data.cast::<u8>(), mapped.pData.cast(), data.size) };
    }
    unsafe { context.Unmap(buffer, 0) };
    if mapped.pData.is_null() { INTERNAL } else { OK }
}

pub(super) unsafe extern "C" fn bind_constant(
    context: *mut c_void,
    stage: u32,
    slot: u32,
    buffer: *mut c_void,
) -> i32 {
    let known = u8::try_from(stage).is_ok_and(|s| STAGES.contains(&s));
    if context.is_null() || slot >= D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT || !known {
        return ARGUMENT;
    }
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }.unwrap();
    // Option<ID3D11Buffer> is pointer-sized with null for None. Borrow the raw
    // pointer slot so the incoming COM reference is neither acquired nor released.
    let buffers = Some(unsafe { std::slice::from_raw_parts((&raw const buffer).cast(), 1) });
    unsafe {
        match stage as u8 {
            b'v' => context.VSSetConstantBuffers(slot, buffers),
            b'p' => context.PSSetConstantBuffers(slot, buffers),
            b'g' => context.GSSetConstantBuffers(slot, buffers),
            b'h' => context.HSSetConstantBuffers(slot, buffers),
            b'd' => context.DSSetConstantBuffers(slot, buffers),
            _ => context.CSSetConstantBuffers(slot, buffers),
        }
    }
    OK
}

/// Writes every inventoried bytecode and its disassembly, recording individual failures.
pub(super) unsafe extern "C" fn dump(directory: *const u16) -> i32 {
    if directory.is_null() {
        return ARGUMENT;
    }
    let root = unsafe { wide_path(directory) };
    guarded(|| {
        if fs::create_dir_all(&root).is_err() {
            return INTERNAL;
        }
        let snapshot = lock(&INVENTORY).clone();
        let mut report = String::new();
        use std::fmt::Write as _;
        for (hash, code) in snapshot {
            if let Err(error) = fs::write(root.join(format!("{hash}.dxbc")), &code) {
                let _ = writeln!(report, "{hash}.dxbc: {error}");
            }
            let assembly = unsafe {
                D3DDisassemble(
                    code.as_ptr().cast(),
                    code.len(),
                    D3D_DISASM_ENABLE_INSTRUCTION_NUMBERING,
                    PCSTR::null(),
                )
            };
            let assembly = assembly.ok();
            match assembly
                .as_ref()
                .map(blob_bytes)
                .filter(|text| !text.is_empty())
            {
                None => {
                    let _ = writeln!(report, "{hash}.asm: disassembly failed");
                }
                Some(text) => {
                    if let Err(error) = fs::write(root.join(format!("{hash}.asm")), text) {
                        let _ = writeln!(report, "{hash}.asm: {error}");
                    }
                }
            }
        }
        let complete = report.is_empty();
        if complete {
            report.push_str("Inventory complete.\n");
        }
        if fs::write(root.join("inventory-report.txt"), report).is_err() || !complete {
            INTERNAL
        } else {
            OK
        }
    })
}
