// SPDX-License-Identifier: MIT
//! Windows x64 leaves for the renderer. COM layouts follow Windows SDK 10.0.26100.0.
use super::ResourceFacts;
use crate::{renderer, scene};
use rsf_minhook_sys::*;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::{
    arch::naked_asm,
    ffi::{c_char, c_void},
    ptr::{self, null_mut},
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    LibraryLoader::*,
    Memory::{PAGE_READWRITE, VirtualProtect},
    Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    Threading::GetCurrentProcess,
};
use windows_sys::core::GUID;

#[unsafe(naked)]
pub(crate) unsafe extern "C" fn pop_detour(_: *mut c_void, _: *mut c_void) -> *mut c_void {
    naked_asm!("mov r8, [rsp]", "jmp {}", sym renderer::rsf_rdr2_pop_detour);
}
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn output_detour(_: *mut c_void, _: *mut c_void, _: u32) -> usize {
    naked_asm!("mov r9, [rsp]", "jmp {}", sym scene::rsf_rdr2_output_detour);
}
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn fsr_dispatch_detour(_: *mut c_void, _: *const c_void) -> i32 {
    naked_asm!("mov r8, [rsp]", "jmp {}", sym renderer::rsf_rdr2_fsr_dispatch_detour);
}

// Pack the two 32-bit arguments into rdx to free r8 for the actual caller address.
// The four register arguments and both existing stack arguments fit without changing rsp.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn blit_detour(
    _: *mut c_void,
    _: u32,
    _: u32,
    _: *const c_void,
    _: *const c_void,
    _: *const c_void,
) -> usize {
    naked_asm!("mov edx, edx", "shl r8, 32", "or rdx, r8", "mov r8, [rsp]", "jmp {}", sym blit_bridge);
}
unsafe extern "C" fn blit_bridge(
    shader: *mut c_void,
    packed: u64,
    caller: usize,
    source: *const c_void,
    destination: *const c_void,
    clip: *const c_void,
) -> usize {
    unsafe {
        scene::rsf_rdr2_blit_detour(
            shader,
            packed as u32,
            (packed >> 32) as u32,
            source,
            destination,
            clip,
            caller,
        )
    }
}

pub(crate) unsafe extern "C" fn read(address: usize, out: *mut c_void, bytes: usize) -> bool {
    let mut count = 0;
    address != 0
        && !out.is_null()
        && unsafe { ReadProcessMemory(GetCurrentProcess(), address as _, out, bytes, &mut count) }
            != 0
        && count == bytes
}

pub(crate) unsafe extern "C" fn module_path(
    address: usize,
    base: *mut usize,
    path: *mut c_char,
    capacity: usize,
) -> bool {
    unsafe { base.write(0) };
    if capacity == 0 {
        return false;
    }
    unsafe { path.write(0) };
    let mut owner = null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { GetModuleHandleExW(flags, address as _, &mut owner) } == 0 || owner.is_null() {
        return false;
    }
    unsafe { base.write(owner as usize) };
    let mut wide = [0u16; 260];
    let len = unsafe { GetModuleFileNameW(owner, wide.as_mut_ptr(), wide.len() as u32) };
    let name = String::from_utf16_lossy(&wide[..len as usize]);
    if name.is_empty() || name.len() >= capacity {
        return false;
    }
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), path.cast(), name.len());
        path.add(name.len()).write(0);
    }
    true
}

// D3D12 GetDesc uses a hidden output pointer after this on Windows x64.
const EXECUTE: usize = 10;
const QUEUE_DESC: usize = 18;
const RESOURCE_DESC: usize = 10;
const BARRIER: usize = 26;
const PRESENT: usize = 8;
const GET_BUFFER: usize = 9;
const SWAPCHAIN_DESC: usize = 12;
const BACK_BUFFER: usize = 36;
const SWAPCHAIN4: GUID = GUID::from_u128(0x3d585d5a_bd4a_489e_b1f4_3dbcb6452ffb);
const RESOURCE: GUID = GUID::from_u128(0x696442be_a72e_4059_bc79_5b5c98040fad);

#[repr(C)]
#[derive(Default)]
struct ResourceDesc {
    dimension: u32,
    alignment: u64,
    width: u64,
    height: u32,
    depth_or_array_size: u16,
    mip_levels: u16,
    format: u32,
    samples: u32,
    sample_quality: u32,
    layout: u32,
    flags: u32,
}
#[repr(C)]
#[derive(Default)]
struct SwapchainDesc {
    mode: [u32; 7],
    sample_desc: [u32; 2],
    usage: u32,
    buffers: u32,
    window: usize,
    windowed: i32,
    effect: u32,
    flags: u32,
}
#[repr(C)]
struct TransitionBarrier {
    kind: u32,
    flags: u32,
    resource: *mut c_void,
    subresource: u32,
    before: u32,
    after: u32,
    padding: u32,
}
const _: () = {
    assert!(size_of::<ResourceDesc>() == 56);
    assert!(std::mem::offset_of!(ResourceDesc, format) == 32);
    assert!(size_of::<SwapchainDesc>() == 72);
    assert!(std::mem::offset_of!(SwapchainDesc, buffers) == 40);
    assert!(std::mem::offset_of!(SwapchainDesc, window) == 48);
    assert!(size_of::<TransitionBarrier>() == 32);
};
unsafe fn method<F: Copy>(object: *mut c_void, slot: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let table = unsafe { object.cast::<*const usize>().read() };
    unsafe { std::mem::transmute_copy(&table.add(slot).read()) }
}
unsafe fn clone_table<const N: usize>(
    object: *mut c_void,
    slot: usize,
    detour: *const c_void,
    original: *mut *mut c_void,
) -> bool {
    let location = unsafe { AtomicPtr::<usize>::from_ptr(object.cast()) };
    let table = location.load(Ordering::Acquire);
    let Some(mut copy) = rsf_game_sys::ffi_util::try_box([0usize; N]) else {
        return false;
    };
    if !unsafe {
        read(
            table as usize,
            copy.as_mut_ptr().cast(),
            size_of::<[usize; N]>(),
        )
    } || copy[slot] == 0
    {
        return false;
    }
    let saved = copy[slot];
    copy[slot] = detour as usize;
    let mut protection = 0;
    if unsafe { VirtualProtect(object, size_of::<usize>(), PAGE_READWRITE, &mut protection) } == 0 {
        return false;
    }
    let replacement = Box::into_raw(copy);
    let installed = location
        .compare_exchange(
            table,
            replacement.cast(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok();
    let mut ignored = 0;
    unsafe { VirtualProtect(object, size_of::<usize>(), protection, &mut ignored) };
    if installed {
        unsafe { original.write(saved as *mut c_void) };
        // The owner remains referenced and callable until process exit.
    } else {
        drop(unsafe { Box::from_raw(replacement) });
    }
    installed
}
pub(crate) unsafe extern "C" fn attach_queue(
    queue: *mut c_void,
    detour: *const c_void,
    original: *mut *mut c_void,
) -> bool {
    if queue.is_null() {
        return false;
    }
    type GetDesc = unsafe extern "system" fn(*mut c_void, *mut [u32; 4]) -> *mut [u32; 4];
    let get: GetDesc = unsafe { method(queue, QUEUE_DESC) };
    let mut desc = [0; 4];
    unsafe { get(queue, &mut desc) };
    if desc[0] != 0 || !unsafe { clone_table::<19>(queue, EXECUTE, detour, original) } {
        return false;
    }
    let add_ref: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { method(queue, 1) };
    unsafe { add_ref(queue) };
    true
}
pub(crate) unsafe extern "C" fn attach_present(
    chain: *mut c_void,
    detour: *const c_void,
    original: *mut *mut c_void,
) -> bool {
    if chain.is_null() {
        return false;
    }
    type Query = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
    let query: Query = unsafe { method(chain, 0) };
    let mut latest = null_mut();
    if unsafe { query(chain, &SWAPCHAIN4, &mut latest) } < 0 {
        return false;
    }
    if latest != chain || !unsafe { clone_table::<41>(chain, PRESENT, detour, original) } {
        unsafe { release(latest) };
        return false;
    }
    true
}
pub(crate) unsafe extern "C" fn resource_facts(
    resource: *mut c_void,
    out: *mut ResourceFacts,
) -> bool {
    if resource.is_null() || out.is_null() {
        return false;
    }
    type GetDesc = unsafe extern "system" fn(*mut c_void, *mut ResourceDesc) -> *mut ResourceDesc;
    let get: GetDesc = unsafe { method(resource, RESOURCE_DESC) };
    let mut desc = ResourceDesc::default();
    unsafe {
        get(resource, &mut desc);
        out.write(ResourceFacts {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            samples: desc.samples,
            _reserved: 0,
        });
    }
    true
}
pub(crate) unsafe extern "C" fn barrier(
    list: *mut c_void,
    resource: *mut c_void,
    before: u32,
    after: u32,
) {
    let barrier = TransitionBarrier {
        kind: 0,
        flags: 0,
        resource,
        subresource: u32::MAX,
        before,
        after,
        padding: 0,
    };
    let record: unsafe extern "system" fn(*mut c_void, u32, *const TransitionBarrier) =
        unsafe { method(list, BARRIER) };
    unsafe { record(list, 1, &barrier) };
}
pub(crate) unsafe extern "C" fn swapchain_facts(
    chain: *mut c_void,
    buffers: *mut u32,
    window: *mut u64,
) -> bool {
    if chain.is_null() {
        return false;
    }
    let get: unsafe extern "system" fn(*mut c_void, *mut SwapchainDesc) -> i32 =
        unsafe { method(chain, SWAPCHAIN_DESC) };
    let mut desc = SwapchainDesc::default();
    if unsafe { get(chain, &mut desc) } < 0 {
        return false;
    }
    unsafe {
        buffers.write(desc.buffers);
        window.write(desc.window as u64);
    }
    true
}
pub(crate) unsafe extern "C" fn swapchain_buffer(chain: *mut c_void, index: u32) -> *mut c_void {
    if chain.is_null() {
        return null_mut();
    }
    type GetBuffer =
        unsafe extern "system" fn(*mut c_void, u32, *const GUID, *mut *mut c_void) -> i32;
    let get: GetBuffer = unsafe { method(chain, GET_BUFFER) };
    let mut buffer = null_mut();
    if unsafe { get(chain, index, &RESOURCE, &mut buffer) } < 0 {
        return null_mut();
    }
    buffer
}
pub(crate) unsafe extern "C" fn back_buffer_index(chain: *mut c_void) -> u32 {
    let get: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { method(chain, BACK_BUFFER) };
    unsafe { get(chain) }
}
pub(crate) unsafe extern "C" fn release(object: *mut c_void) {
    if !object.is_null() {
        let release: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { method(object, 2) };
        unsafe { release(object) };
    }
}

pub(crate) unsafe extern "C" fn main_module() -> usize {
    unsafe { GetModuleHandleW(ptr::null()) as usize }
}
pub(crate) unsafe extern "C" fn module_handle(name: *const c_char) -> usize {
    unsafe { GetModuleHandleA(name.cast()) as usize }
}
pub(crate) unsafe extern "C" fn export_address(module: usize, name: *const c_char) -> usize {
    if module == 0 {
        return 0;
    }
    unsafe { GetProcAddress(module as _, name.cast()).map_or(0, |entry| entry as usize) }
}
pub(crate) unsafe extern "C" fn qpc(now: *mut u64, frequency: *mut u64) {
    let (mut counter, mut rate) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut counter);
        QueryPerformanceFrequency(&mut rate);
        now.write(counter as u64);
        frequency.write(rate as u64);
    }
}
pub(crate) unsafe extern "C" fn initialize() -> bool {
    matches!(unsafe { MH_Initialize() }, 0 | 1)
}
pub(crate) unsafe extern "C" fn create(
    target: usize,
    detour: *const c_void,
    original: *mut *mut c_void,
) -> bool {
    unsafe { MH_CreateHook(target as _, detour.cast_mut(), original) == 0 }
}
pub(crate) unsafe extern "C" fn enable(target: usize) -> bool {
    unsafe { MH_EnableHook(target as _) == 0 }
}
pub(crate) unsafe extern "C" fn disable(target: usize) -> bool {
    matches!(unsafe { MH_DisableHook(target as _) }, 0 | 4 | 6)
}
pub(crate) unsafe extern "C" fn remove(target: usize) -> bool {
    matches!(unsafe { MH_RemoveHook(target as _) }, 0 | 4)
}
