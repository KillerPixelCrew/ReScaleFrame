// SPDX-License-Identifier: MIT
//! Typed SDK calls protected by the observer's native exception frame.
use super::Handle;
use std::ptr::{from_mut, null, null_mut};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_QUEUE_DESC, D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT, D3D12_DRED_ENABLEMENT,
    D3D12_DRED_ENABLEMENT_FORCED_ON, D3D12_DRED_PAGE_FAULT_OUTPUT,
    D3D12_MESSAGE_CALLBACK_FLAG_NONE, D3D12_MESSAGE_CALLBACK_FLAGS, D3D12MessageFunc,
    ID3D12CommandQueue, ID3D12CommandQueue_Vtbl, ID3D12Debug, ID3D12Debug_Vtbl, ID3D12Device,
    ID3D12Device_Vtbl, ID3D12DeviceChild, ID3D12DeviceChild_Vtbl, ID3D12DeviceRemovedExtendedData,
    ID3D12DeviceRemovedExtendedData_Vtbl, ID3D12DeviceRemovedExtendedDataSettings,
    ID3D12DeviceRemovedExtendedDataSettings_Vtbl, ID3D12InfoQueue1, ID3D12InfoQueue1_Vtbl,
};
use windows::Win32::Graphics::Dxgi::{DXGI_SWAP_CHAIN_DESC, IDXGISwapChain, IDXGISwapChain_Vtbl};
use windows::core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface};

const FAIL: i32 = 0x8000_4005u32 as i32;

unsafe extern "C" {
    fn rsf_observer12_try(call: unsafe extern "C-unwind" fn(Handle), context: Handle) -> i32;
}

#[link(name = "d3d12")]
unsafe extern "system-unwind" {
    fn D3D12GetDebugInterface(iid: *const GUID, out: *mut Handle) -> HRESULT;
}

fn guarded<F: FnOnce() -> T, T>(call: F) -> Option<T> {
    struct Call<F, T> {
        run: Option<F>,
        result: Option<T>,
    }
    unsafe extern "C-unwind" fn invoke<F: FnOnce() -> T, T>(context: Handle) {
        let call = unsafe { &mut *context.cast::<Call<F, T>>() };
        if let Some(run) = call.run.take() {
            call.result = Some(run());
        }
    }
    // The native frame calls the thunk synchronously once. On a foreign exception
    // the thunk unwinds to that frame without publishing an incomplete result.
    let mut call = Call {
        run: Some(call),
        result: None,
    };
    let caught = unsafe { rsf_observer12_try(invoke::<F, T>, from_mut(&mut call).cast()) };
    if caught == 0 { None } else { call.result }
}

macro_rules! method {
    ($iface:ty, $table:ty, $method:ident => $name:ident, ($($arg:ty),*) -> $result:ty) => {
        fn $name(interface: &$iface) -> unsafe extern "system-unwind" fn(Handle, $($arg),*) -> $result {
            let call: unsafe extern "system" fn(Handle, $($arg),*) -> $result =
                <$iface as Interface>::vtable(interface).$method;
            // The system calling convention stays the same. The private call site
            // permits a foreign exception to reach the native catch frame.
            unsafe { std::mem::transmute(call) }
        }
        const _: fn(&$table) -> unsafe extern "system" fn(Handle, $($arg),*) -> $result =
            |table| table.$method;
    };
}

method!(IUnknown, IUnknown_Vtbl, QueryInterface => query_interface, (*const GUID, *mut Handle) -> HRESULT);
method!(ID3D12DeviceChild, ID3D12DeviceChild_Vtbl, GetDevice => get_device, (*const GUID, *mut Handle) -> HRESULT);
method!(ID3D12Device, ID3D12Device_Vtbl, GetDeviceRemovedReason => get_removed_reason, () -> HRESULT);
method!(ID3D12DeviceRemovedExtendedData, ID3D12DeviceRemovedExtendedData_Vtbl,
    GetAutoBreadcrumbsOutput => get_breadcrumbs, (*mut D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT) -> HRESULT);
method!(ID3D12DeviceRemovedExtendedData, ID3D12DeviceRemovedExtendedData_Vtbl,
    GetPageFaultAllocationOutput => get_page_fault, (*mut D3D12_DRED_PAGE_FAULT_OUTPUT) -> HRESULT);
method!(ID3D12DeviceRemovedExtendedDataSettings, ID3D12DeviceRemovedExtendedDataSettings_Vtbl,
    SetAutoBreadcrumbsEnablement => set_breadcrumbs, (D3D12_DRED_ENABLEMENT) -> ());
method!(ID3D12DeviceRemovedExtendedDataSettings, ID3D12DeviceRemovedExtendedDataSettings_Vtbl,
    SetPageFaultEnablement => set_page_faults, (D3D12_DRED_ENABLEMENT) -> ());
method!(ID3D12Debug, ID3D12Debug_Vtbl, EnableDebugLayer => enable_debug_layer, () -> ());
method!(ID3D12CommandQueue, ID3D12CommandQueue_Vtbl, GetDesc => get_queue_desc, (*mut D3D12_COMMAND_QUEUE_DESC) -> ());
method!(IDXGISwapChain, IDXGISwapChain_Vtbl, GetDesc => get_chain_desc, (*mut DXGI_SWAP_CHAIN_DESC) -> HRESULT);
method!(ID3D12InfoQueue1, ID3D12InfoQueue1_Vtbl, RegisterMessageCallback => register_message_callback,
    (D3D12MessageFunc, D3D12_MESSAGE_CALLBACK_FLAGS, Handle, *mut u32) -> HRESULT);

pub(super) unsafe fn query(raw: Handle, iid: &GUID, out: &mut Handle) -> i32 {
    let Some(object) = (unsafe { IUnknown::from_raw_borrowed(&raw) }) else {
        return FAIL;
    };
    guarded(|| unsafe { query_interface(object)(raw, iid, out) }.0).unwrap_or(FAIL)
}

pub(super) unsafe fn debug(iid: &GUID, out: &mut Handle) -> i32 {
    guarded(|| unsafe { D3D12GetDebugInterface(iid, out) }.0).unwrap_or(FAIL)
}

pub(super) unsafe extern "C" fn queue_device(raw: Handle, out: &mut Handle) -> i32 {
    let Some(queue) = (unsafe { ID3D12DeviceChild::from_raw_borrowed(&raw) }) else {
        return FAIL;
    };
    guarded(|| unsafe { get_device(queue)(raw, &ID3D12Device::IID, out) }.0).unwrap_or(FAIL)
}

pub(super) unsafe extern "C" fn removed_reason(raw: Handle) -> i32 {
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    guarded(|| unsafe { get_removed_reason(device)(raw) }.0).unwrap_or(0)
}

pub(super) unsafe extern "C" fn breadcrumb_head(raw: Handle) -> *const std::ffi::c_void {
    let Some(dred) = (unsafe { ID3D12DeviceRemovedExtendedData::from_raw_borrowed(&raw) }) else {
        return null();
    };
    let mut out = D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT::default();
    let result = guarded(|| unsafe { get_breadcrumbs(dred)(raw, &mut out) }.0);
    if result.is_some_and(|hr| hr >= 0) {
        out.pHeadAutoBreadcrumbNode.cast()
    } else {
        null()
    }
}

pub(super) unsafe extern "C" fn page_fault(raw: Handle, address: &mut u64) -> i32 {
    let Some(dred) = (unsafe { ID3D12DeviceRemovedExtendedData::from_raw_borrowed(&raw) }) else {
        return FAIL;
    };
    let mut out = D3D12_DRED_PAGE_FAULT_OUTPUT::default();
    let result = guarded(|| unsafe { get_page_fault(dred)(raw, &mut out) }.0).unwrap_or(FAIL);
    if result >= 0 {
        *address = out.PageFaultVA;
    }
    result
}

pub(super) unsafe extern "C" fn breadcrumbs(raw: Handle) {
    if let Some(settings) =
        unsafe { ID3D12DeviceRemovedExtendedDataSettings::from_raw_borrowed(&raw) }
    {
        guarded(|| unsafe { set_breadcrumbs(settings)(raw, D3D12_DRED_ENABLEMENT_FORCED_ON) });
    }
}

pub(super) unsafe extern "C" fn page_faults(raw: Handle) {
    if let Some(settings) =
        unsafe { ID3D12DeviceRemovedExtendedDataSettings::from_raw_borrowed(&raw) }
    {
        guarded(|| unsafe { set_page_faults(settings)(raw, D3D12_DRED_ENABLEMENT_FORCED_ON) });
    }
}

pub(super) unsafe extern "C" fn debug_layer(raw: Handle) -> i32 {
    let Some(debug) = (unsafe { ID3D12Debug::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    i32::from(guarded(|| unsafe { enable_debug_layer(debug)(raw) }).is_some())
}

pub(super) unsafe extern "C" fn queue_type(raw: Handle) -> u32 {
    let Some(queue) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&raw) }) else {
        return u32::MAX;
    };
    let mut out = D3D12_COMMAND_QUEUE_DESC::default();
    guarded(|| unsafe { get_queue_desc(queue)(raw, &mut out) })
        .map_or(u32::MAX, |_| out.Type.0 as u32)
}

pub(super) unsafe fn register_debug(raw: Handle, callback: D3D12MessageFunc) {
    if let Some(queue) = unsafe { ID3D12InfoQueue1::from_raw_borrowed(&raw) } {
        let mut cookie = 0;
        guarded(|| unsafe {
            register_message_callback(queue)(
                raw,
                callback,
                D3D12_MESSAGE_CALLBACK_FLAG_NONE,
                null_mut(),
                &mut cookie,
            )
        });
    }
}

pub(super) unsafe extern "C" fn describe_window(raw: Handle, window: &mut Handle) -> i32 {
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    let mut out = DXGI_SWAP_CHAIN_DESC::default();
    let result = guarded(|| unsafe { get_chain_desc(chain)(raw, &mut out) }.0);
    if result.is_none_or(|hr| hr < 0) {
        return 0;
    }
    *window = out.OutputWindow.0;
    1
}
