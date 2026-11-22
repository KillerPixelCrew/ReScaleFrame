// SPDX-License-Identifier: MIT
//! State and resource ownership behind the typed DXGI facade.
use super::{
    DXGI_ERROR_UNSUPPORTED, E_FAIL, S_OK, device::*, facade::*, factory::*, gpu::*, log, settings,
};
use crate::allocate_handle;
use crate::com_ref;
use crate::d3d11_present_abi::*;
use crate::shared_surface::{
    Fence, rsf_shared_fence_create, rsf_shared_fence_d3d11, rsf_shared_fence_d3d12,
    rsf_shared_fence_destroy,
};
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::RwLock;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_DESC;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, IsWindow};
use windows::core::{GUID, IUnknown, Interface};

use crate::sys::{
    RSF_SL_PROFILE_DLSS_FG, RSF_STREAMLINE_HOST_ABI_VERSION, rsf_streamline_host_adopt,
    rsf_streamline_host_create, rsf_streamline_host_destroy, rsf_streamline_host_setup,
};

use crate::d3d12_generation::rsf_generation_fsr4_supported;

// The SEH-guarded native calls in d3d11_present_bridge.c, and the Streamline host's graphics.
unsafe extern "C" {
    fn rsf_d3d11p_original_create() -> Option<GuardedCreate>;
    fn rsf_d3d11p_unwrap_guarded(host: Raw, proxy: Raw) -> Raw;
    fn rsf_streamline_host_graphics(host: Raw, graphics: *mut rsf_streamline_graphics) -> i32;
}

#[derive(Default)]
struct BridgeState {
    direct_queue: Option<IUnknown>,
    direct_device: Option<IUnknown>,
    parent: Option<IUnknown>,
    adapter: Option<IUnknown>,
    context: Option<IUnknown>,
    device: Option<IUnknown>,
    tearing: u32,
    host: Raw,
    graphics: rsf_streamline_graphics,
    native12: u32,
    shared_fence: *mut Fence,
}

#[derive(Default)]
struct FacadeState {
    // Resize publishes a descriptor after it has finished GPU and provider work.
    // Concurrent GetDesc calls take a copy without holding a lock across that work.
    description: RwLock<DXGI_SWAP_CHAIN_DESC>,
    // The owner initializes these before publication and retires them under the final lease.
    resources: UnsafeCell<BridgeState>,
}

fn raw(object: &Option<IUnknown>) -> Raw {
    object.as_ref().map_or(null_mut(), Interface::as_raw)
}

unsafe fn state<'a>(facade: Raw) -> &'a BridgeState {
    unsafe { &*capsule(rsf_d3d11p_facade_state(facade)).resources.get() }
}

unsafe fn state_mut<'a>(facade: Raw) -> &'a mut BridgeState {
    unsafe { &mut *capsule(rsf_d3d11p_facade_state(facade)).resources.get() }
}

unsafe fn capsule<'a>(state: Raw) -> &'a FacadeState {
    unsafe { &*state.cast::<FacadeState>() }
}

unsafe fn description(facade: Raw) -> DXGI_SWAP_CHAIN_DESC {
    let capsule = unsafe { capsule(rsf_d3d11p_facade_state(facade)) };
    *capsule.description.read().unwrap()
}

fn host_setup(adapter: Raw) -> rsf_streamline_host_setup {
    let s = settings();
    rsf_streamline_host_setup {
        struct_size: size_of::<rsf_streamline_host_setup>() as u32,
        abi_version: RSF_STREAMLINE_HOST_ABI_VERSION,
        runtime_directory_utf8: s.streamline_directory_utf8,
        dxgi_adapter: adapter,
        engine_type: s.engine_type,
        engine_version_utf8: s.engine_version_utf8,
        project_id_utf8: s.project_id_utf8,
        require_signature: 1,
        development_runtime: s.development_runtime,
        log: s.log,
        log_user: s.user,
        profile: RSF_SL_PROFILE_DLSS_FG,
        load_dlss_sr: 1,
    }
}

unsafe extern "C" fn describe(state: Raw, out: *mut DXGI_SWAP_CHAIN_DESC) {
    let desc = *unsafe { capsule(state) }.description.read().unwrap();
    unsafe { out.write(desc) };
}

unsafe extern "C" fn parent(state: Raw) -> Raw {
    raw(&unsafe { &*capsule(state).resources.get() }.parent)
}

unsafe extern "C" fn device(state: Raw) -> Raw {
    let s = unsafe { &*capsule(state).resources.get() };
    if s.native12 != 0 {
        raw(&s.direct_device)
    } else {
        raw(&s.device)
    }
}

unsafe extern "C" fn destroy(capsule: Raw) {
    drop(unsafe { Box::from_raw(capsule.cast::<FacadeState>()) });
}

static STATE_OPS: NativeState = NativeState {
    describe,
    parent,
    device,
    destroy,
};

pub(super) unsafe extern "C" fn window_valid(window: Raw) -> u32 {
    u32::from(unsafe { IsWindow(Some(HWND(window))) }.as_bool())
}

pub(super) unsafe extern "C" fn client_size(window: Raw, width: *mut u32, height: *mut u32) -> u32 {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(HWND(window), &mut rect) }.is_err() {
        return 0;
    }
    unsafe {
        width.write(rect.right as u32);
        height.write(rect.bottom as u32);
    }
    1
}

pub(super) unsafe extern "C" fn canonical_identity(receiver: Raw) -> Raw {
    unsafe { com_ref::identity(receiver) }.map_or(null_mut(), |identity| identity.as_raw())
}

pub(super) unsafe extern "C" fn add_ref(object: Raw) {
    unsafe { com_ref::retain(object) }.into_raw();
}

pub(super) unsafe extern "C" fn release(object: Raw) {
    drop(unsafe { com_ref::owned(object) });
}

pub(super) unsafe extern "C" fn bridge_new(chain: *mut Chain) -> Raw {
    let capsule: Raw = allocate_handle(FacadeState::default()).cast();
    if capsule.is_null() {
        return null_mut();
    }
    let facade = unsafe { rsf_d3d11p_facade_create(chain, capsule, &STATE_OPS) };
    if facade.is_null() {
        unsafe { destroy(capsule) };
    }
    facade
}

pub(super) unsafe extern "C" fn bridge_release_owned(facade: Raw) {
    unsafe { rsf_d3d11p_facade_release(facade) };
}

pub(super) unsafe extern "C" fn facts(facade: Raw, out: *mut rsf_d3d11p_facts) {
    let s = unsafe { state(facade) };
    let d = unsafe { description(facade) };
    unsafe {
        out.write(rsf_d3d11p_facts {
            window: d.OutputWindow.0,
            width: d.BufferDesc.Width,
            height: d.BufferDesc.Height,
            format: d.BufferDesc.Format.0 as u32,
            buffer_count: d.BufferCount,
            flags: d.Flags,
            windowed: u32::from(d.Windowed.as_bool()),
            native12: s.native12,
            tearing: s.tearing,
            graphics: s.graphics,
            direct_queue: raw(&s.direct_queue),
            device11: raw(&s.device),
            buffer_usage: d.BufferUsage.0,
        });
    }
}

pub(super) unsafe extern "C" fn init_source(
    facade: Raw,
    factory: Raw,
    source: Raw,
    desc: *const c_void,
) -> u32 {
    let s = unsafe { state_mut(facade) };
    let capsule = unsafe { capsule(rsf_d3d11p_facade_state(facade)) };
    *capsule.description.write().unwrap() = unsafe { *desc.cast() };
    s.parent = Some(unsafe { com_ref::retain(factory) });
    s.graphics.struct_size = size_of::<rsf_streamline_graphics>() as u32;
    let queue = unsafe { IUnknown::from_raw_borrowed(&source) }
        .and_then(|source| source.cast::<ID3D12CommandQueue>().ok());
    s.direct_queue = queue.and_then(|queue| unsafe { com_ref::owned(queue.into_raw()) });
    s.native12 = u32::from(s.direct_queue.is_some());
    s.native12
}

pub(super) unsafe extern "C" fn native_device(facade: Raw) -> i32 {
    let s = unsafe { state_mut(facade) };
    s.direct_device = None;
    let mut device = null_mut();
    let result = unsafe { rsf_d3d11p_device_queue_device(raw(&s.direct_queue), &mut device) };
    s.direct_device = unsafe { com_ref::owned(device) };
    result
}

pub(super) unsafe extern "C" fn owning_adapter(
    facade: Raw,
    source: Raw,
    out: *mut rsf_d3d11p_adapter,
) -> i32 {
    if unsafe { state(facade) }.native12 != 0 && unsafe { native_device(facade) } < 0 {
        return E_FAIL;
    }
    let mut info = AdapterInfo::default();
    if unsafe {
        rsf_d3d11p_device_read_adapter(source, raw(&state(facade).direct_device), &mut info)
    } < 0
    {
        return E_FAIL;
    }
    unsafe {
        out.write(rsf_d3d11p_adapter {
            vendor: info.vendor,
            device: info.device,
            default_backend: crate::fg_choice::rsf_fg_default_backend(info.vendor, info.software),
            reserved: 0,
        });
    }
    S_OK
}

pub(super) unsafe extern "C" fn adopt_host(facade: Raw, identity: u32, result: *mut i32) -> u32 {
    let s = unsafe { state_mut(facade) };
    let mut adapter = null_mut();
    if unsafe { rsf_d3d11p_device_native_adapter(raw(&s.direct_device), &mut adapter) } < 0 {
        return 0;
    }
    let adapter = unsafe { com_ref::owned(adapter) };
    let setup = host_setup(raw(&adapter));
    let status = if identity != 0 {
        unsafe {
            rsf_streamline_host_adopt(
                &setup,
                raw(&s.direct_device),
                raw(&s.direct_queue),
                (&raw mut s.host).cast(),
            )
        }
    } else {
        -7
    };
    unsafe { result.write(status) };
    1
}

pub(super) unsafe extern "C" fn d3d11_device(facade: Raw, source: Raw) -> i32 {
    let s = unsafe { state_mut(facade) };
    s.device = None;
    s.context = None;
    s.adapter = None;
    let [mut device, mut context, mut adapter] = [null_mut(); 3];
    let result =
        unsafe { rsf_d3d11p_device_immediate(source, &mut device, &mut context, &mut adapter) };
    s.device = unsafe { com_ref::owned(device) };
    s.context = unsafe { com_ref::owned(context) };
    s.adapter = unsafe { com_ref::owned(adapter) };
    result
}

pub(super) unsafe extern "C" fn create_host(facade: Raw) -> i32 {
    let s = unsafe { state_mut(facade) };
    let setup = host_setup(raw(&s.adapter));
    unsafe { rsf_streamline_host_create(&setup, (&raw mut s.host).cast()) }
}

pub(super) unsafe extern "C" fn host(facade: Raw) -> Raw {
    unsafe { state(facade) }.host
}

pub(super) unsafe extern "C" fn resolve_graphics(facade: Raw) -> i32 {
    let s = unsafe { state_mut(facade) };
    let adapter = s.adapter.take();
    if !s.host.is_null() {
        return if unsafe { rsf_streamline_host_graphics(s.host, &mut s.graphics) } == 0 {
            S_OK
        } else {
            E_FAIL
        };
    }
    if s.native12 == 0 {
        s.direct_device = None;
        let mut device = null_mut();
        if unsafe { rsf_d3d11p_device_create12(raw(&adapter), &mut device) } < 0 {
            return DXGI_ERROR_UNSUPPORTED;
        }
        s.direct_device = unsafe { com_ref::owned(device) };
    }
    if s.direct_queue.is_none() {
        let mut queue = null_mut();
        if unsafe { create_interop_queue(raw(&s.direct_device), &mut queue) } < 0 {
            return E_FAIL;
        }
        s.direct_queue = unsafe { com_ref::owned(queue) };
    }
    s.graphics.device = raw(&s.direct_device);
    s.graphics.native_device = s.graphics.device;
    s.graphics.queue = raw(&s.direct_queue);
    s.graphics.native_queue = s.graphics.queue;
    S_OK
}

pub(super) unsafe extern "C" fn tearing_supported(facade: Raw) -> u32 {
    let s = unsafe { state_mut(facade) };
    s.tearing = unsafe { rsf_d3d11p_device_tearing(raw(&s.parent)) };
    s.tearing
}

pub(super) unsafe extern "C" fn fsr4_supported(facade: Raw, directory: *const c_char) -> u32 {
    unsafe { rsf_generation_fsr4_supported(state(facade).graphics.native_device, directory) }
}

pub(super) unsafe extern "C" fn create_upload_fence(facade: Raw) -> i32 {
    let s = unsafe { state_mut(facade) };
    let config = settings();
    let ok = unsafe {
        rsf_shared_fence_create(
            raw(&s.device),
            s.graphics.native_device,
            config.log,
            config.user,
            &mut s.shared_fence,
        )
    } == 0;
    if ok { S_OK } else { E_FAIL }
}

pub(super) unsafe extern "C" fn create_slot(
    facade: Raw,
    allocator: *mut Raw,
    list: *mut Raw,
) -> i32 {
    unsafe { rsf_d3d11p_gpu_create_slot(state(facade).graphics.device, allocator, list) }
}

pub(super) unsafe extern "C" fn create_render_buffer(
    facade: Raw,
    width: u32,
    height: u32,
    format: u32,
    out: *mut Raw,
) -> i32 {
    unsafe {
        rsf_d3d11p_gpu_render_buffer(
            raw(&state(facade).direct_device),
            width,
            height,
            format,
            out,
        )
    }
}

pub(super) unsafe extern "C" fn create_plain(
    facade: Raw,
    plan: &rsf_d3d11p_plain_plan,
    physical: *mut Raw,
    queue: *mut Raw,
) -> i32 {
    let s = unsafe { state(facade) };
    let (parent, desc, native12) = (raw(&s.parent), unsafe { description(facade) }, s.native12);
    unsafe {
        rsf_d3d11p_factory_create_plain(
            facade,
            parent,
            &desc,
            plan,
            native12,
            rsf_d3d11p_original_create(),
            physical,
            queue,
        )
    }
}

pub(super) unsafe extern "C" fn resize_physical(
    facade: Raw,
    physical: Raw,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
) -> i32 {
    unsafe {
        rsf_d3d11p_gpu_resize(
            physical,
            count,
            width,
            height,
            format,
            state(facade).tearing,
        )
    }
}

pub(super) unsafe extern "C" fn upload_wait(facade: Raw, queue: Raw, value: u64) -> i32 {
    let s = unsafe { state(facade) };
    unsafe {
        rsf_d3d11p_gpu_upload_wait(
            raw(&s.context),
            queue,
            rsf_shared_fence_d3d11(s.shared_fence),
            rsf_shared_fence_d3d12(s.shared_fence),
            value,
        )
    }
}

pub(super) unsafe extern "C" fn record_copy(
    facade: Raw,
    commands: Raw,
    backbuffer: Raw,
    source: Raw,
) {
    unsafe { rsf_d3d11p_gpu_record_copy(commands, backbuffer, source, state(facade).native12) };
}

pub(super) unsafe extern "C" fn context11(facade: Raw) -> Raw {
    raw(&unsafe { state(facade) }.context)
}

pub(super) unsafe extern "C" fn upload_return(facade: Raw, queue: Raw, value: u64) -> i32 {
    let s = unsafe { state(facade) };
    unsafe {
        rsf_d3d11p_gpu_upload_return(
            raw(&s.context),
            queue,
            rsf_shared_fence_d3d11(s.shared_fence),
            rsf_shared_fence_d3d12(s.shared_fence),
            value,
        )
    }
}

pub(super) unsafe extern "C" fn report_device_failure(facade: Raw, result: i32, backend: u32) {
    let device = raw(&unsafe { state(facade) }.direct_device);
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return;
    };
    let reason = unsafe { (device.vtable().GetDeviceRemovedReason)(device.as_raw()) };
    log(&format!(
        "Native D3D12 presentation failed backend={backend} HRESULT={:08x} device_reason={:08x}",
        result as u32, reason.0 as u32
    ));
    report_debug_messages(device);
}

fn report_debug_messages(device: &ID3D12Device) {
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_MESSAGE, D3D12_MESSAGE_SEVERITY_ERROR, ID3D12InfoQueue,
    };
    let Ok(queue) = device.cast::<ID3D12InfoQueue>() else {
        return;
    };
    let count = unsafe { queue.GetNumStoredMessagesAllowedByRetrievalFilter() };
    let mut reported = 0;
    for index in (0..count).rev() {
        let mut bytes = 0;
        if unsafe { queue.GetMessage(index, None, &mut bytes) }.is_err() {
            continue;
        }
        // D3D12 writes a message header followed by its description into caller storage.
        let mut storage = Vec::<usize>::new();
        let words = bytes.div_ceil(size_of::<usize>());
        if bytes < size_of::<D3D12_MESSAGE>() || storage.try_reserve_exact(words).is_err() {
            continue;
        }
        let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
        if unsafe { queue.GetMessage(index, Some(message), &mut bytes) }.is_ok() {
            let message = unsafe { &*message };
            if message.Severity.0 <= D3D12_MESSAGE_SEVERITY_ERROR.0
                && !message.pDescription.is_null()
            {
                log(&unsafe { CStr::from_ptr(message.pDescription.cast()) }.to_string_lossy());
                reported += 1;
                if reported == 8 {
                    break;
                }
            }
        }
    }
}

pub(super) unsafe extern "C" fn log_present_failure(
    facade: Raw,
    step: *const c_char,
    result: i32,
    render_index: u32,
) {
    let d = unsafe { description(facade) };
    let step = unsafe { CStr::from_ptr(step) }.to_string_lossy();
    log(&format!(
        "Native D3D12 presentation step '{step}' failed HRESULT={:08x} render_index={render_index} buffers={} size={}x{}",
        result as u32, d.BufferCount, d.BufferDesc.Width, d.BufferDesc.Height
    ));
}

pub(super) unsafe extern "C" fn log_buffer_refusal(
    facade: Raw,
    index: u32,
    requested: *const c_void,
    result: i32,
    pending: u32,
) {
    let iid = unsafe { *requested.cast::<GUID>() };
    let native12 = unsafe { state(facade) }.native12;
    log(&format!(
        "GetBuffer refused HRESULT=0x{:08x} facade={facade:p} index={index} IID={iid:?} output=0x0 native12={native12} physical_pending={pending}",
        result as u32
    ));
}

pub(super) unsafe extern "C" fn set_description(
    facade: Raw,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
) {
    let capsule = unsafe { capsule(rsf_d3d11p_facade_state(facade)) };
    let mut d = capsule.description.write().unwrap();
    d.BufferCount = count;
    d.BufferDesc.Width = width;
    d.BufferDesc.Height = height;
    d.BufferDesc.Format = DXGI_FORMAT(format as i32);
    d.Flags = flags;
}

pub(super) unsafe extern "C" fn create_retirement_fence(facade: Raw, out: *mut Raw) -> i32 {
    unsafe { create_completion_fence(state(facade).graphics.native_device, out) }
}

pub(super) unsafe extern "C" fn retire_submit(
    facade: Raw,
    queue: Raw,
    retirement_fence: Raw,
    vendor_fence: Raw,
    vendor_value: u64,
    list: Raw,
    value: u64,
) -> i32 {
    unsafe {
        rsf_d3d11p_gpu_retire_submit(
            facade,
            queue,
            retirement_fence,
            vendor_fence,
            vendor_value,
            list,
            value,
            command_list,
        )
    }
}

pub(super) unsafe extern "C" fn retain_all(facade: Raw) {
    let s = unsafe { state_mut(facade) };
    // Unknown completion: leave every reference reachable by GPU work alive.
    for reference in [
        &mut s.context,
        &mut s.device,
        &mut s.parent,
        &mut s.direct_device,
        &mut s.direct_queue,
    ] {
        if let Some(reference) = reference.take() {
            reference.into_raw();
        }
    }
}

pub(super) unsafe extern "C" fn release_presentation(facade: Raw) {
    let fence = std::mem::replace(&mut unsafe { state_mut(facade) }.shared_fence, null_mut());
    unsafe { rsf_shared_fence_destroy(fence) };
}

pub(super) unsafe extern "C" fn release_direct_graphics(facade: Raw) {
    let s = unsafe { state_mut(facade) };
    s.direct_queue = None;
    s.direct_device = None;
}

pub(super) unsafe extern "C" fn release_host(facade: Raw) {
    let s = unsafe { state_mut(facade) };
    s.context = None;
    s.device = None;
    s.parent = None;
    let host = std::mem::replace(&mut s.host, null_mut());
    unsafe { rsf_streamline_host_destroy(host.cast()) };
}

pub(super) unsafe extern "C" fn source_queue(facade: Raw, mut queue: Raw) -> Raw {
    let s = unsafe { state(facade) };
    if s.native12 == 0 && !s.host.is_null() && !queue.is_null() {
        queue = unsafe { rsf_d3d11p_unwrap_guarded(s.host, queue) };
    }
    if !queue.is_null() {
        unsafe { com_ref::retain(queue) }.into_raw();
    }
    queue
}

pub(super) unsafe extern "C" fn command_list(facade: Raw, queue: Raw, list: Raw) -> Raw {
    let s = unsafe { state(facade) };
    if !s.host.is_null() && queue != s.graphics.queue {
        unsafe { rsf_d3d11p_unwrap_guarded(s.host, list) }
    } else {
        list
    }
}
