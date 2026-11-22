// SPDX-License-Identifier: MIT
//! D3D11 creation interception, the bytecode inventory and generic constant buffers.

#[cfg(not(test))]
use super::platform::{CONSTANT_SLOTS, CPU_WRITE, DYNAMIC_USAGE};
#[cfg(test)]
use super::tests::{CONSTANT_SLOTS, CPU_WRITE, DYNAMIC_USAGE};
use super::{
    ARGUMENT, COMPILE, COUNTERS, INTERNAL, OK, guarded, lock, platform, raw, sha256, wide_path,
};
use crate::sys::rsf_patch_bytes;
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
        if unsafe { (p.hooks_ready)() } == 0 {
            return INTERNAL;
        }
        for (i, (&entry, detour)) in entries.iter().zip(DETOURS).enumerate() {
            let mut original = ptr::null_mut();
            if unsafe {
                (p.hook_create)(entry as *mut c_void, detour as *mut c_void, &mut original)
            } == 0
            {
                for &created in &entries[..i] {
                    // These hooks have never been enabled.
                    unsafe { (p.hook_remove)(created as *mut c_void) };
                }
                return INTERNAL;
            }
            ORIGINALS[i].store(original, Ordering::Release);
        }
        hooks.targets = entries;
    }
    for (i, &entry) in entries.iter().enumerate() {
        if unsafe { (p.hook_enable)(entry as *mut c_void) } == 0 {
            for &enabled in &entries[..i] {
                unsafe { (p.hook_disable)(enabled as *mut c_void) };
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
    let stage = unsafe { *target.cast::<u8>() };
    INTERNAL_CREATE.set(true);
    let result = if STAGES.contains(&stage) {
        unsafe { (platform().create_shader)(device, u32::from(stage), code, shader) }
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
    let created = unsafe { (platform().create_constant_buffer)(device, size, out) };
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
    unsafe { (p.buffer_desc)(buffer, &mut width, &mut usage, &mut access) };
    let writable = usage == DYNAMIC_USAGE && access & CPU_WRITE != 0;
    if bytes.size != width as usize || !writable {
        return ARGUMENT;
    }
    if unsafe { (p.upload)(context, buffer, bytes) } != 0 {
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
    if context.is_null() || slot >= CONSTANT_SLOTS || !known {
        return ARGUMENT;
    }
    unsafe { (platform().bind_constant)(context, stage, slot, buffer) };
    OK
}

unsafe fn append(user: *mut c_void, data: *const c_void, size: usize) {
    let out = unsafe { &mut *user.cast::<Vec<u8>>() };
    if size != 0 {
        out.extend_from_slice(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) });
    }
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
        let disassemble = platform().disassemble;
        let mut report = String::new();
        use std::fmt::Write as _;
        for (hash, code) in snapshot {
            if let Err(error) = fs::write(root.join(format!("{hash}.dxbc")), &code) {
                let _ = writeln!(report, "{hash}.dxbc: {error}");
            }
            let mut text = Vec::new();
            let status = unsafe { disassemble(raw(&code), (&raw mut text).cast(), Some(append)) };
            if status != 0 || text.is_empty() {
                let _ = writeln!(report, "{hash}.asm: disassembly failed ({status})");
            } else if let Err(error) = fs::write(root.join(format!("{hash}.asm")), &text) {
                let _ = writeln!(report, "{hash}.asm: {error}");
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

#[cfg(test)]
pub(super) fn inventory_contains(hash: &str) -> bool {
    lock(&INVENTORY).contains_key(hash)
}
