// SPDX-License-Identifier: MIT
//! D3D11 creation interception, the bytecode inventory and generic constant buffers.

use super::{
    ARGUMENT, COMPILE, COUNTERS, INTERNAL, OK, guarded, lock, platform, raw, sha256, wide_path,
};
use crate::shader_patch_abi::{
    RSF_PATCH_CONSTANT_SLOTS, RSF_PATCH_CPU_ACCESS_WRITE, RSF_PATCH_USAGE_DYNAMIC, rsf_patch_bytes,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::{c_char, c_void};
use std::fs;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

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
const E_INVALIDARG: i32 = 0x8007_0057_u32 as i32;

static ORIGINALS: [AtomicPtr<c_void>; 6] = [const { AtomicPtr::new(ptr::null_mut()) }; 6];
// Hooked creation entry points, recorded as addresses. Zero while detached.
static TARGETS: Mutex<[usize; 6]> = Mutex::new([0; 6]);
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

pub(super) const DETOURS: [Create; 6] = [
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
    let p = platform();
    let mut targets = lock(&TARGETS);
    let vtable = unsafe { *device.cast::<*const usize>() };
    let entry = |i: usize| unsafe { *vtable.add(SLOTS[i]) };
    if targets[0] != 0 {
        let same = (0..6).all(|i| targets[i] == entry(i));
        return if same { OK } else { ARGUMENT };
    }
    if unsafe { (p.hooks_ready.unwrap())() } == 0 {
        return INTERNAL;
    }
    let mut count = 0;
    while count < 6 {
        let detour = DETOURS[count] as *mut c_void;
        let original = ORIGINALS[count].as_ptr();
        if unsafe { (p.hook_create.unwrap())(entry(count) as *mut c_void, detour, original) } == 0 {
            break;
        }
        targets[count] = entry(count);
        count += 1;
    }
    if count == 6 {
        let failed =
            (0..6).find(|&i| unsafe { (p.hook_enable.unwrap())(targets[i] as *mut c_void) } == 0);
        count = failed.unwrap_or(6);
    }
    if count != 6 {
        for target in targets.iter_mut().filter(|t| **t != 0) {
            unsafe { (p.hook_remove.unwrap())(*target as *mut c_void) };
            *target = 0;
        }
        return INTERNAL;
    }
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
    let stage = unsafe { *target.cast::<u8>() };
    INTERNAL_CREATE.set(true);
    let result = if STAGES.contains(&stage) {
        unsafe { (platform().create_shader.unwrap())(device, u32::from(stage), code, shader) }
    } else {
        E_INVALIDARG
    };
    INTERNAL_CREATE.set(false);
    if result >= 0 { OK } else { COMPILE }
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
    let created = unsafe { (platform().create_constant_buffer.unwrap())(device, size, out) };
    if created >= 0 { OK } else { INTERNAL }
}

/// Replaces a whole dynamic, CPU-writable buffer with WRITE_DISCARD.
pub(super) unsafe extern "C" fn upload_constant(
    context: *mut c_void,
    buffer: *mut c_void,
    bytes: rsf_patch_bytes,
) -> i32 {
    if context.is_null() || buffer.is_null() || bytes.data.is_null() {
        return ARGUMENT;
    }
    let p = platform();
    let (mut width, mut usage, mut access) = (0, 0, 0);
    unsafe { (p.buffer_desc.unwrap())(buffer, &mut width, &mut usage, &mut access) };
    let writable =
        usage == RSF_PATCH_USAGE_DYNAMIC as u32 && access & RSF_PATCH_CPU_ACCESS_WRITE as u32 != 0;
    if bytes.size != width as usize || !writable {
        return ARGUMENT;
    }
    if unsafe { (p.upload.unwrap())(context, buffer, bytes) } != 0 {
        return INTERNAL;
    }
    OK
}

pub(super) unsafe extern "C" fn bind_constant(
    context: *mut c_void,
    stage: u32,
    slot: u32,
    buffer: *mut c_void,
) -> i32 {
    let known = u8::try_from(stage).is_ok_and(|s| STAGES.contains(&s));
    if context.is_null() || slot >= RSF_PATCH_CONSTANT_SLOTS as u32 || !known {
        return ARGUMENT;
    }
    unsafe { (platform().bind_constant.unwrap())(context, stage, slot, buffer) };
    OK
}

unsafe extern "C" fn append(user: *mut c_void, data: *const c_void, size: usize) {
    let out = unsafe { &mut *user.cast::<Vec<u8>>() };
    if size != 0 {
        out.extend_from_slice(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) });
    }
}

/// Writes every inventoried bytecode and its disassembly. Individual file failures are ignored.
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
        let disassemble = platform().disassemble.unwrap();
        for (hash, code) in snapshot {
            let _ = fs::write(root.join(format!("{hash}.dxbc")), &code);
            let mut text = Vec::new();
            let written = unsafe { disassemble(raw(&code), (&raw mut text).cast(), Some(append)) };
            if written == 0 {
                let _ = fs::write(root.join(format!("{hash}.asm")), &text);
            }
        }
        OK
    })
}

#[cfg(test)]
pub(super) fn inventory_contains(hash: &str) -> bool {
    lock(&INVENTORY).contains_key(hash)
}
