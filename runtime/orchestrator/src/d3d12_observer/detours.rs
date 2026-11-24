// SPDX-License-Identifier: MIT
use super::{
    CHAIN, DEVICE, FACTORY, Handle, QUEUE, attachment, bind, diagnostics, enter, leave, original,
    submit_begin, submit_end,
};
use std::ptr;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_QUEUE_DESC, ID3D12CommandQueue_Vtbl, ID3D12Device_Vtbl,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FULLSCREEN_DESC, IDXGIFactory2_Vtbl,
    IDXGISwapChain_Vtbl,
};
use windows::core::{GUID, HRESULT};

macro_rules! original_method {
    ($name:ident, $table:ty, $method:ident, ($($arg:ty),*) -> $result:ty) => {
        // Preserve the native detours' exception propagation to the SDK caller.
        type $name = unsafe extern "system-unwind" fn($($arg),*) -> $result;
        const _: fn(&$table) -> unsafe extern "system" fn($($arg),*) -> $result =
            |table| table.$method;
    };
}

original_method!(Factory, IDXGIFactory2_Vtbl, CreateSwapChainForHwnd,
    (Handle, Handle, HWND, *const DXGI_SWAP_CHAIN_DESC1, *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC, Handle, *mut Handle) -> HRESULT);
original_method!(Device, ID3D12Device_Vtbl, CreateCommandQueue,
    (Handle, *const D3D12_COMMAND_QUEUE_DESC, *const GUID, *mut Handle) -> HRESULT);
original_method!(Queue, ID3D12CommandQueue_Vtbl, ExecuteCommandLists, (Handle, u32, *const Handle) -> ());
original_method!(Present, IDXGISwapChain_Vtbl, Present, (Handle, u32, DXGI_PRESENT) -> HRESULT);

const FAIL: HRESULT = HRESULT(0x8000_4005u32 as i32);

struct Scope {
    kind: u32,
    outer: i32,
}

impl Scope {
    fn enter(kind: u32) -> Self {
        Self {
            kind,
            outer: enter(kind),
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        leave(self.kind);
    }
}

unsafe extern "system-unwind" fn factory<const I: usize>(
    this: Handle,
    queue: Handle,
    window: HWND,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    output: Handle,
    chain: *mut Handle,
) -> HRESULT {
    let scope = Scope::enter(FACTORY);
    let target = original(FACTORY, I);
    if target.is_null() {
        return FAIL;
    }
    let call: Factory = unsafe { std::mem::transmute(target) };
    let result = unsafe { call(this, queue, window, desc, fullscreen, output, chain) };
    unsafe { attachment::factory_result(result.0, scope.outer as u32, queue, window.0, chain) };
    result
}

unsafe extern "system-unwind" fn device<const I: usize>(
    this: Handle,
    desc: *const D3D12_COMMAND_QUEUE_DESC,
    iid: *const GUID,
    out: *mut Handle,
) -> HRESULT {
    let target = original(DEVICE, I);
    if target.is_null() {
        return FAIL;
    }
    let call: Device = unsafe { std::mem::transmute(target) };
    let result = unsafe { call(this, desc, iid, out) };
    unsafe { attachment::device_result(result.0, out) };
    result
}

unsafe extern "system-unwind" fn queue<const I: usize>(
    this: Handle,
    count: u32,
    lists: *const Handle,
) {
    let scope = Scope::enter(QUEUE);
    let target = original(QUEUE, I);
    if target.is_null() {
        return;
    }
    let call: Queue = unsafe { std::mem::transmute(target) };
    if scope.outer == 0 {
        unsafe { call(this, count, lists) };
        return;
    }
    let id = submit_begin(this, count, lists);
    unsafe { call(this, count, lists) };
    submit_end(this, id, count, lists);
    diagnostics::report_removal(this);
}

unsafe extern "system-unwind" fn present<const I: usize>(
    this: Handle,
    interval: u32,
    flags: DXGI_PRESENT,
) -> HRESULT {
    let scope = Scope::enter(CHAIN);
    let target = original(CHAIN, I);
    if target.is_null() {
        return FAIL;
    }
    let call: Present = unsafe { std::mem::transmute(target) };
    if scope.outer == 0 {
        return unsafe { call(this, interval, flags) };
    }
    let selected = attachment::present_begin(this, interval, flags.0) != 0;
    let result = unsafe { call(this, interval, flags) };
    if selected {
        super::notify_present(this, interval, flags.0, 0);
    }
    result
}

macro_rules! wrappers {
    ($hook:ident) => {
        [
            $hook::<0> as Handle,
            $hook::<1> as Handle,
            $hook::<2> as Handle,
            $hook::<3> as Handle,
            $hook::<4> as Handle,
            $hook::<5> as Handle,
            $hook::<6> as Handle,
            $hook::<7> as Handle,
        ]
    };
}

// Unit tests bind through a fake, so this leaf and the detours it installs are unused there.
#[cfg_attr(test, allow(dead_code))]
pub(super) unsafe extern "C" fn bind_interface(kind: u32, object: Handle, slot: usize) -> i32 {
    if object.is_null() {
        return 0;
    }
    let wrappers = match kind {
        FACTORY => wrappers!(factory),
        DEVICE => wrappers!(device),
        QUEUE => wrappers!(queue),
        CHAIN => wrappers!(present),
        _ => return 0,
    };
    let table = unsafe { object.cast::<*const Handle>().read() };
    if table.is_null() {
        return 0;
    }
    let target = unsafe { ptr::read(table.add(slot)) };
    unsafe { bind(kind, target, &wrappers) }
}
