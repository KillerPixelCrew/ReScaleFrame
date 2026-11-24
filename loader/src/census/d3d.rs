// SPDX-License-Identifier: MIT
use super::Ids;
use super::decode::rsf_census_dxgi_format_name;
use super::hooks::{rsf_census_hook_bind_inline, rsf_census_hook_original, rsf_census_hooks_apply};
use super::json::{Json, rsf_census_json_raw};
use super::memory::read_as;
use super::platform;
use super::session::{Call, rsf_census_active, rsf_census_flags};
use super::stats::{
    rsf_census_present_enter, rsf_census_present_leave, rsf_census_stats_queue_describe,
    rsf_census_stats_queue_submit,
};
use crate::sys::RSF_CENSUS_FLAG_D3D12_DEBUG_ONLY;
use crate::sys::*;
use std::ffi::{CStr, c_void};
use std::mem::{MaybeUninit, transmute};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering::*};
use windows::{
    Win32::{
        Foundation::E_NOINTERFACE,
        Graphics::{
            Direct3D12::{
                D3D12_COMMAND_QUEUE_DESC, D3D12_DRED_ENABLEMENT_FORCED_ON,
                D3D12_MESSAGE_CALLBACK_IGNORE_FILTERS, D3D12_MESSAGE_CATEGORY, D3D12_MESSAGE_ID,
                D3D12_MESSAGE_SEVERITY, D3D12_MESSAGE_SEVERITY_CORRUPTION,
                D3D12_MESSAGE_SEVERITY_ERROR, D3D12_MESSAGE_SEVERITY_WARNING, ID3D12Debug,
                ID3D12DeviceRemovedExtendedDataSettings, ID3D12InfoQueue1,
            },
            Dxgi::{
                DXGI_PRESENT_PARAMETERS, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_CHAIN_FULLSCREEN_DESC, IDXGIFactory2, IDXGISwapChain1, IDXGISwapChain2,
                IDXGISwapChain3,
            },
        },
    },
    core::{GUID, Interface, PCSTR},
};
use windows_sys::{
    Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    core::w,
};

type Ptr = *mut c_void;
type Entry = rsf_census_d3d_entry;
const COUNT: usize = RSF_D3D_COUNT as usize;
const QUEUE: u8 = 1;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const DEBUG_LAYER: u32 = 8;

static IDS: Ids<COUNT> = Ids::new();
static LAST_COLOR: AtomicI32 = AtomicI32::new(-1);
static DEBUG: AtomicBool = AtomicBool::new(false);
static CREATED_DEVICE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static BEFORE_SUBMIT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static AFTER_SUBMIT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static BEFORE_PRESENT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static SWAPCHAIN: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static COLOR_SPACE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

type ObjectObserver = unsafe extern "C" fn(Ptr);
type SubmitObserver = unsafe extern "C" fn(Ptr, u32, *const Ptr);
type ColorObserver = unsafe extern "C" fn(Ptr, u32);

#[unsafe(no_mangle)]
extern "C" fn rsf_census_observe_d3d12(
    device: Option<ObjectObserver>,
    before_submit: Option<SubmitObserver>,
    after_submit: Option<SubmitObserver>,
    present: Option<ObjectObserver>,
    swapchain: Option<ObjectObserver>,
    color: Option<ColorObserver>,
) {
    CREATED_DEVICE.store(device.map_or(null_mut(), |value| value as Ptr), Release);
    BEFORE_SUBMIT.store(
        before_submit.map_or(null_mut(), |value| value as Ptr),
        Release,
    );
    AFTER_SUBMIT.store(
        after_submit.map_or(null_mut(), |value| value as Ptr),
        Release,
    );
    BEFORE_PRESENT.store(present.map_or(null_mut(), |value| value as Ptr), Release);
    SWAPCHAIN.store(swapchain.map_or(null_mut(), |value| value as Ptr), Release);
    COLOR_SPACE.store(color.map_or(null_mut(), |value| value as Ptr), Release);
}

fn notify_object(observer: &AtomicPtr<c_void>, object: Ptr) {
    let callback = observer.load(Acquire);
    if !callback.is_null() {
        unsafe { transmute::<Ptr, ObjectObserver>(callback)(object) };
    }
}

fn notify_submit(observer: &AtomicPtr<c_void>, queue: Ptr, count: u32, lists: *const Ptr) {
    let callback = observer.load(Acquire);
    if !callback.is_null() {
        unsafe { transmute::<Ptr, SubmitObserver>(callback)(queue, count, lists) };
    }
}

fn id(entry: Entry) -> u16 {
    IDS.get(entry as usize)
}

impl Json<'_> {
    fn fmt_code(&mut self, format: u32) {
        self.number(c"format", format);
        unsafe { self.text_ptr(c"format_name", rsf_census_dxgi_format_name(format)) };
    }
    fn written(&mut self, key: &CStr, out: *mut Ptr) {
        if let Some(value) = read_ptr(out) {
            self.hex(Some(key), value as u64);
        } else {
            unsafe { rsf_census_json_raw(self.0, key.as_ptr(), c"null".as_ptr()) };
        }
    }
}

fn read_ptr(source: *mut Ptr) -> Option<Ptr> {
    unsafe { read_as::<usize>(source as u64) }.map(|value| value as Ptr)
}

fn bind(entry: Entry, object: Ptr, slot: usize, interface: &CStr) {
    if !rsf_census_hook_original(id(entry)).is_null() || object.is_null() {
        return;
    }
    let target = unsafe { platform().slot.unwrap()(object, slot) };
    if !target.is_null() && rsf_census_hook_bind_inline(id(entry), target, interface.as_ptr()) {
        rsf_census_hooks_apply();
    }
}

fn with_interface<T: Interface>(object: Ptr, visit: impl FnOnce(Ptr)) {
    let iid = T::IID;
    let mut iface = null_mut();
    let status = unsafe { platform().query.unwrap()(object, (&raw const iid).cast(), &mut iface) };
    if status >= 0 && !iface.is_null() {
        visit(iface);
        unsafe { platform().release.unwrap()(iface) };
    }
}

fn hook_factory(object: Ptr) {
    if object.is_null() {
        return;
    }
    bind(
        RSF_D3D_DxgiFactoryCreateSwapChain,
        object,
        10,
        c"IDXGIFactory",
    );
    with_interface::<IDXGIFactory2>(object, |factory| {
        bind(
            RSF_D3D_DxgiFactoryCreateSwapChainForHwnd,
            factory,
            15,
            c"IDXGIFactory2",
        );
        bind(
            RSF_D3D_DxgiFactoryCreateSwapChainForCoreWindow,
            factory,
            16,
            c"IDXGIFactory2",
        );
    });
}

fn hook_swapchain(object: Ptr) {
    if object.is_null() {
        return;
    }
    notify_object(&SWAPCHAIN, object);
    bind(RSF_D3D_DxgiSwapChainPresent, object, 8, c"IDXGISwapChain");
    bind(
        RSF_D3D_DxgiSwapChainSetFullscreenState,
        object,
        10,
        c"IDXGISwapChain",
    );
    bind(
        RSF_D3D_DxgiSwapChainResizeBuffers,
        object,
        13,
        c"IDXGISwapChain",
    );
    with_interface::<IDXGISwapChain1>(object, |chain| {
        bind(RSF_D3D_DxgiSwapChainPresent1, chain, 22, c"IDXGISwapChain1")
    });
    with_interface::<IDXGISwapChain2>(object, |chain| {
        bind(
            RSF_D3D_DxgiSwapChainGetFrameLatencyWaitableObject,
            chain,
            33,
            c"IDXGISwapChain2",
        )
    });
    with_interface::<IDXGISwapChain3>(object, |chain| {
        bind(
            RSF_D3D_DxgiSwapChainSetColorSpace1,
            chain,
            38,
            c"IDXGISwapChain3",
        );
        bind(
            RSF_D3D_DxgiSwapChainResizeBuffers1,
            chain,
            39,
            c"IDXGISwapChain3",
        );
    });
}

fn result(value: i32) -> u64 {
    u64::from(value as u32)
}

fn create_factory(
    entry: Entry,
    pc: Ptr,
    iid: *const GUID,
    out: *mut Ptr,
    flags: Option<u32>,
) -> i32 {
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        if let Some(flags) = flags {
            json.hex(Some(c"flags"), flags.into());
        }
        json.guid(iid as u64);
    }
    let status = if let Some(flags) = flags {
        original!(
            entry,
            unsafe extern "system" fn(u32, *const GUID, *mut Ptr) -> i32
        )
        .map_or(0, |function| unsafe { function(flags, iid, out) })
    } else {
        original!(
            entry,
            unsafe extern "system" fn(*const GUID, *mut Ptr) -> i32
        )
        .map_or(0, |function| unsafe { function(iid, out) })
    };
    if let Some(mut json) = call.json() {
        json.written(c"factory", out);
    }
    if status >= 0
        && let Some(factory) = read_ptr(out)
    {
        hook_factory(factory);
    }
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn factory(iid: *const GUID, out: *mut Ptr) -> i32 {
    create_factory(RSF_D3D_CreateDXGIFactory, caller!(), iid, out, None)
}
#[inline(never)]
extern "system" fn factory1(iid: *const GUID, out: *mut Ptr) -> i32 {
    create_factory(RSF_D3D_CreateDXGIFactory1, caller!(), iid, out, None)
}
#[inline(never)]
extern "system" fn factory2(flags: u32, iid: *const GUID, out: *mut Ptr) -> i32 {
    create_factory(RSF_D3D_CreateDXGIFactory2, caller!(), iid, out, Some(flags))
}

unsafe extern "system" fn debug_message(
    category: D3D12_MESSAGE_CATEGORY,
    level: D3D12_MESSAGE_SEVERITY,
    message: D3D12_MESSAGE_ID,
    text: PCSTR,
    device: Ptr,
) {
    if level.0 > D3D12_MESSAGE_SEVERITY_WARNING.0 {
        return;
    }
    const LEVELS: [&CStr; 5] = [c"corruption", c"error", c"warning", c"info", c"message"];
    let pc = (0x1_0000u32.wrapping_add(message.0 as u32)) as Ptr;
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(RSF_D3D_D3d12DebugMessage), pc);
    if let Some(mut json) = call.json() {
        json.number(c"id", message.0 as u32);
        json.hex(c"device", device as u64);
        json.number(c"category", category.0 as u32);
        json.text(
            Some(c"severity"),
            LEVELS.get(level.0 as usize).copied().unwrap_or(c"unknown"),
        );
        let mut message = [0u8; 1024];
        if !text.0.is_null()
            && unsafe {
                platform().read_name.unwrap()(
                    text.0.cast(),
                    message.as_mut_ptr().cast(),
                    message.len(),
                )
            }
            && let Ok(text) = CStr::from_bytes_until_nul(&message)
        {
            json.text(Some(c"text"), text);
        }
    }
    call.finish(result(level.0));
}

type DebugInterface = unsafe extern "system" fn(*const GUID, *mut Ptr) -> i32;

fn debug_interface<T: Interface>(get: DebugInterface) -> (i32, Option<T>) {
    let iid = T::IID;
    let mut raw = null_mut();
    let status = unsafe { get(&iid, &mut raw) };
    let interface = (status >= 0 && !raw.is_null()).then(|| unsafe { T::from_raw(raw) });
    (status, interface)
}

fn enable_debug() -> (i32, i32) {
    let module = unsafe { GetModuleHandleW(w!("d3d12.dll")) };
    if module.is_null() {
        return (E_NOINTERFACE.0, E_NOINTERFACE.0);
    }
    let Some(get) = (unsafe { GetProcAddress(module, c"D3D12GetDebugInterface".as_ptr().cast()) })
    else {
        return (E_NOINTERFACE.0, E_NOINTERFACE.0);
    };
    let get = unsafe { transmute::<unsafe extern "system" fn() -> isize, DebugInterface>(get) };
    let (layer, debug) = debug_interface::<ID3D12Debug>(get);
    if let Some(debug) = debug {
        unsafe { debug.EnableDebugLayer() };
    }
    let (dred, settings) = debug_interface::<ID3D12DeviceRemovedExtendedDataSettings>(get);
    if let Some(settings) = settings {
        unsafe {
            settings.SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            settings.SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
        }
    }
    (layer, dred)
}

fn start_debug() {
    if rsf_census_flags() & (DEBUG_LAYER | RSF_CENSUS_FLAG_D3D12_DEBUG_ONLY) == 0
        || DEBUG
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
    {
        return;
    }
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(RSF_D3D_CensusEvent), start_debug as Ptr);
    if let Some(mut json) = call.json() {
        json.text(Some(c"event"), c"d3d12_debug_layer");
    }
    let (layer, dred) = enable_debug();
    if let Some(mut json) = call.json() {
        json.hex(Some(c"debug_layer_hr"), result(layer));
        json.hex(Some(c"dred_hr"), result(dred));
    }
    call.finish(result(layer));
}

fn attach_info(device: Ptr) {
    if device.is_null() || !DEBUG.load(Acquire) {
        return;
    }
    let mut cookie = 0;
    let status = register_info_queue(device, &mut cookie);
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(RSF_D3D_CensusEvent), attach_info as Ptr);
    if let Some(mut json) = call.json() {
        json.text(Some(c"event"), c"d3d12_info_queue");
        json.hex(c"device", device as u64);
        json.hex(Some(c"hr"), result(status));
        json.number(c"cookie", cookie);
    }
    call.finish(result(status));
}

fn register_info_queue(device: Ptr, cookie: &mut u32) -> i32 {
    let mut raw = null_mut();
    let iid = ID3D12InfoQueue1::IID;
    let query = unsafe { platform().query.unwrap()(device, (&raw const iid).cast(), &mut raw) };
    if query < 0 || raw.is_null() {
        return query;
    }
    let queue = unsafe { ID3D12InfoQueue1::from_raw(raw) };
    let status = unsafe {
        for severity in [
            D3D12_MESSAGE_SEVERITY_CORRUPTION,
            D3D12_MESSAGE_SEVERITY_ERROR,
            D3D12_MESSAGE_SEVERITY_WARNING,
        ] {
            let _ = queue.SetBreakOnSeverity(severity, false);
        }
        (queue.vtable().RegisterMessageCallback)(
            queue.as_raw(),
            Some(debug_message),
            D3D12_MESSAGE_CALLBACK_IGNORE_FILTERS,
            device,
            cookie,
        )
        .0
    };
    if status >= 0 {
        // The registered callback keeps this reference for the process lifetime.
        let _ = queue.into_raw();
    }
    status
}

#[inline(never)]
extern "system" fn create_device(adapter: Ptr, level: i32, iid: *const GUID, out: *mut Ptr) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_D3D12CreateDevice;
    let original = original!(
        entry,
        unsafe extern "system" fn(Ptr, i32, *const GUID, *mut Ptr) -> i32
    );
    start_debug();
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"adapter", adapter as u64);
        json.hex(Some(c"minimum_feature_level"), result(level));
        json.guid(iid as u64);
        json.flag(c"capability_test", out.is_null());
    }
    let status = original.map_or(E_FAIL, |function| unsafe {
        function(adapter, level, iid, out)
    });
    if let Some(mut json) = call.json() {
        json.written(c"device", out);
    }
    if status >= 0
        && let Some(device) = read_ptr(out)
    {
        bind(
            RSF_D3D_D3d12DeviceCreateCommandQueue,
            device,
            8,
            c"ID3D12Device",
        );
        attach_info(device);
        notify_object(&CREATED_DEVICE, device);
    }
    call.finish(result(status));
    status
}

fn describe_queue(queue: Ptr, desc: &D3D12_COMMAND_QUEUE_DESC) {
    if rsf_census_active() {
        rsf_census_stats_queue_describe(
            QUEUE,
            queue,
            desc.Type.0 as u32,
            desc.Priority as u32,
            desc.Flags.0 as u32,
        );
    }
}

#[inline(never)]
extern "system" fn create_queue(
    device: Ptr,
    desc: *const D3D12_COMMAND_QUEUE_DESC,
    iid: *const GUID,
    out: *mut Ptr,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_D3d12DeviceCreateCommandQueue;
    let original = original!(
        entry,
        unsafe extern "system" fn(
            Ptr,
            *const D3D12_COMMAND_QUEUE_DESC,
            *const GUID,
            *mut Ptr,
        ) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        const TYPES: [&CStr; 7] = [
            c"DIRECT",
            c"BUNDLE",
            c"COMPUTE",
            c"COPY",
            c"VIDEO_DECODE",
            c"VIDEO_PROCESS",
            c"VIDEO_ENCODE",
        ];
        json.hex(c"device", device as u64);
        if let Some(value) = unsafe { read_as::<D3D12_COMMAND_QUEUE_DESC>(desc as u64) } {
            json.number(c"type", value.Type.0 as u32);
            json.text(
                Some(c"type_name"),
                TYPES
                    .get(value.Type.0 as usize)
                    .copied()
                    .unwrap_or(c"other"),
            );
            json.signed(c"priority", value.Priority);
            json.hex(Some(c"flags"), result(value.Flags.0));
            json.number(c"node_mask", value.NodeMask);
        }
        json.guid(iid as u64);
    }
    let status = original.map_or(0, |function| unsafe { function(device, desc, iid, out) });
    if let Some(mut json) = call.json() {
        json.written(c"queue", out);
    }
    if status >= 0
        && let Some(queue) = read_ptr(out).filter(|ptr| !ptr.is_null())
    {
        if let Some(value) = unsafe { read_as::<D3D12_COMMAND_QUEUE_DESC>(desc as u64) } {
            describe_queue(queue, &value);
        }
        bind(
            RSF_D3D_D3d12QueueExecuteCommandLists,
            queue,
            10,
            c"ID3D12CommandQueue",
        );
    }
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn execute(queue: Ptr, count: u32, lists: *const Ptr) {
    let pc = caller!();
    let entry = RSF_D3D_D3d12QueueExecuteCommandLists;
    let original = original!(entry, unsafe extern "system" fn(Ptr, u32, *const Ptr));
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"queue", queue as u64);
        json.number(c"num_lists", count);
        json.open(c"lists", b'[');
        for index in 0..count.min(8) as usize {
            let address = (lists as u64).wrapping_add((index * size_of::<Ptr>()) as u64);
            let Some(list) = (unsafe { read_as::<usize>(address) }) else {
                break;
            };
            json.hex(None, list as u64);
        }
        json.close(b']');
    }
    notify_submit(&BEFORE_SUBMIT, queue, count, lists);
    if let Some(function) = original {
        unsafe { function(queue, count, lists) };
    }
    notify_submit(&AFTER_SUBMIT, queue, count, lists);
    if rsf_census_active() && rsf_census_stats_queue_submit(QUEUE, queue, count) {
        let mut desc = D3D12_COMMAND_QUEUE_DESC::default();
        if unsafe { platform().queue_desc.unwrap()(queue, (&raw mut desc).cast()) } {
            describe_queue(queue, &desc);
        }
    }
    call.finish(0);
}

fn legacy_desc(json: &mut Json<'_>, desc: &DXGI_SWAP_CHAIN_DESC, creation: bool) {
    let key = if creation { c"desc" } else { c"swapchain_desc" };
    json.open(key, b'{');
    json.number(c"width", desc.BufferDesc.Width);
    json.number(c"height", desc.BufferDesc.Height);
    json.fmt_code(desc.BufferDesc.Format.0 as u32);
    if creation {
        json.number(c"refresh_numerator", desc.BufferDesc.RefreshRate.Numerator);
        json.number(
            c"refresh_denominator",
            desc.BufferDesc.RefreshRate.Denominator,
        );
        json.number(c"samples", desc.SampleDesc.Count);
        json.hex(Some(c"buffer_usage"), desc.BufferUsage.0.into());
    }
    json.number(c"buffer_count", desc.BufferCount);
    if creation {
        json.hex(c"output_window", desc.OutputWindow.0 as u64);
        json.flag(c"windowed", desc.Windowed.as_bool());
    }
    json.number(c"swap_effect", desc.SwapEffect.0 as u32);
    json.hex(Some(c"flags"), desc.Flags.into());
    if !creation {
        json.flag(c"windowed", desc.Windowed.as_bool());
    }
    json.close(b'}');
}

fn desc1(json: &mut Json<'_>, desc: *const DXGI_SWAP_CHAIN_DESC1) {
    let Some(desc) = (unsafe { read_as::<DXGI_SWAP_CHAIN_DESC1>(desc as u64) }) else {
        return;
    };
    json.open(c"desc", b'{');
    json.number(c"width", desc.Width);
    json.number(c"height", desc.Height);
    json.fmt_code(desc.Format.0 as u32);
    json.flag(c"stereo", desc.Stereo.as_bool());
    json.number(c"samples", desc.SampleDesc.Count);
    json.hex(Some(c"buffer_usage"), desc.BufferUsage.0.into());
    json.number(c"buffer_count", desc.BufferCount);
    json.number(c"scaling", desc.Scaling.0 as u32);
    json.number(c"swap_effect", desc.SwapEffect.0 as u32);
    json.number(c"alpha_mode", desc.AlphaMode.0 as u32);
    json.hex(Some(c"flags"), desc.Flags.into());
    json.close(b'}');
}

fn created_chain(mut call: Call<'_>, status: i32, out: *mut Ptr) -> i32 {
    if let Some(mut json) = call.json() {
        json.written(c"swapchain", out);
    }
    if status >= 0
        && let Some(chain) = read_ptr(out)
    {
        hook_swapchain(chain);
    }
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn create_swapchain(
    factory: Ptr,
    device: Ptr,
    desc: *const DXGI_SWAP_CHAIN_DESC,
    out: *mut Ptr,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiFactoryCreateSwapChain;
    let original = original!(
        entry,
        unsafe extern "system" fn(Ptr, Ptr, *const DXGI_SWAP_CHAIN_DESC, *mut Ptr) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"factory", factory as u64);
        json.hex(c"device_or_queue", device as u64);
        if let Some(desc) = unsafe { read_as::<DXGI_SWAP_CHAIN_DESC>(desc as u64) } {
            legacy_desc(&mut json, &desc, true);
        }
    }
    let status = original.map_or(0, |function| unsafe {
        function(factory, device, desc, out)
    });
    created_chain(call, status, out)
}

#[inline(never)]
extern "system" fn create_hwnd(
    factory: Ptr,
    device: Ptr,
    window: Ptr,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    restriction: Ptr,
    out: *mut Ptr,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiFactoryCreateSwapChainForHwnd;
    let original = original!(
        entry,
        unsafe extern "system" fn(
            Ptr,
            Ptr,
            Ptr,
            *const DXGI_SWAP_CHAIN_DESC1,
            *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
            Ptr,
            *mut Ptr,
        ) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"factory", factory as u64);
        json.hex(c"device_or_queue", device as u64);
        json.hex(c"hwnd", window as u64);
        desc1(&mut json, desc);
        if let Some(value) =
            unsafe { read_as::<DXGI_SWAP_CHAIN_FULLSCREEN_DESC>(fullscreen as u64) }
        {
            json.flag(c"fullscreen_windowed", value.Windowed.as_bool());
            json.number(c"fullscreen_scaling", value.Scaling.0 as u32);
        }
    }
    let status = original.map_or(0, |function| unsafe {
        function(factory, device, window, desc, fullscreen, restriction, out)
    });
    created_chain(call, status, out)
}

#[inline(never)]
extern "system" fn create_core_window(
    factory: Ptr,
    device: Ptr,
    window: Ptr,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    restriction: Ptr,
    out: *mut Ptr,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiFactoryCreateSwapChainForCoreWindow;
    let original = original!(
        entry,
        unsafe extern "system" fn(
            Ptr,
            Ptr,
            Ptr,
            *const DXGI_SWAP_CHAIN_DESC1,
            Ptr,
            *mut Ptr,
        ) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"factory", factory as u64);
        json.hex(c"device_or_queue", device as u64);
        json.hex(c"core_window", window as u64);
        desc1(&mut json, desc);
    }
    let status = original.map_or(0, |function| unsafe {
        function(factory, device, window, desc, restriction, out)
    });
    created_chain(call, status, out)
}

struct PresentLease;
impl Drop for PresentLease {
    fn drop(&mut self) {
        rsf_census_present_leave();
    }
}

fn put_present(
    json: &mut Json<'_>,
    chain: Ptr,
    sync: u32,
    flags: u32,
    params: *const DXGI_PRESENT_PARAMETERS,
) {
    json.hex(c"swapchain", chain as u64);
    json.number(c"sync_interval", sync);
    json.hex(Some(c"present_flags"), flags.into());
    json.open(c"present_flag_names", b'[');
    for (bit, name) in [
        (0x1, c"TEST"),
        (0x2, c"DO_NOT_SEQUENCE"),
        (0x4, c"RESTART"),
        (0x8, c"DO_NOT_WAIT"),
        (0x10, c"STEREO_PREFER_RIGHT"),
        (0x20, c"STEREO_TEMPORARY_MONO"),
        (0x40, c"RESTRICT_TO_OUTPUT"),
        (0x100, c"USE_DURATION"),
        (0x200, c"ALLOW_TEARING"),
    ] {
        if flags & bit != 0 {
            json.text(None, name);
        }
    }
    json.close(b']');
    if let Some(params) = unsafe { read_as::<DXGI_PRESENT_PARAMETERS>(params as u64) } {
        json.number(c"dirty_rects", params.DirtyRectsCount);
        json.flag(c"scroll_rect", !params.pScrollRect.is_null());
    }
    let mut desc = DXGI_SWAP_CHAIN_DESC::default();
    if unsafe { platform().swapchain_desc.unwrap()(chain, (&raw mut desc).cast()) } {
        legacy_desc(json, &desc, false);
    }
    let color = LAST_COLOR.load(Relaxed);
    if color >= 0 {
        json.number(c"last_color_space_set", color as u32);
    }
}

#[inline(never)]
extern "system" fn present(chain: Ptr, sync: u32, flags: u32) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainPresent;
    let original = original!(entry, unsafe extern "system" fn(Ptr, u32, u32) -> i32);
    let test = flags & 1 != 0;
    let outer = rsf_census_present_enter(test);
    let _lease = PresentLease;
    if outer && !test {
        notify_object(&BEFORE_PRESENT, chain);
    }
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        put_present(&mut json, chain, sync, flags, null());
    }
    let status = original.map_or(0, |function| unsafe { function(chain, sync, flags) });
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn present1(
    chain: Ptr,
    sync: u32,
    flags: u32,
    params: *const DXGI_PRESENT_PARAMETERS,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainPresent1;
    let original = original!(
        entry,
        unsafe extern "system" fn(Ptr, u32, u32, *const DXGI_PRESENT_PARAMETERS) -> i32
    );
    rsf_census_present_enter(flags & 1 != 0);
    let _lease = PresentLease;
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        put_present(&mut json, chain, sync, flags, params);
    }
    let status = original.map_or(0, |function| unsafe {
        function(chain, sync, flags, params)
    });
    call.finish(result(status));
    status
}

fn resize_fields(
    json: &mut Json<'_>,
    chain: Ptr,
    count: u32,
    width: u32,
    height: u32,
    format: i32,
    flags: u32,
) {
    json.hex(c"swapchain", chain as u64);
    json.number(c"buffer_count", count);
    json.number(c"width", width);
    json.number(c"height", height);
    json.fmt_code(format as u32);
    json.hex(Some(c"flags"), flags.into());
}

#[inline(never)]
extern "system" fn resize(
    chain: Ptr,
    count: u32,
    width: u32,
    height: u32,
    format: i32,
    flags: u32,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainResizeBuffers;
    let original = original!(
        entry,
        unsafe extern "system" fn(Ptr, u32, u32, u32, i32, u32) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        resize_fields(&mut json, chain, count, width, height, format, flags);
    }
    let status = original.map_or(0, |function| unsafe {
        function(chain, count, width, height, format, flags)
    });
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn resize1(
    chain: Ptr,
    count: u32,
    width: u32,
    height: u32,
    format: i32,
    flags: u32,
    nodes: *const u32,
    queues: *const Ptr,
) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainResizeBuffers1;
    let original = original!(
        entry,
        unsafe extern "system" fn(Ptr, u32, u32, u32, i32, u32, *const u32, *const Ptr) -> i32
    );
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        resize_fields(&mut json, chain, count, width, height, format, flags);
    }
    let status = original.map_or(0, |function| unsafe {
        function(chain, count, width, height, format, flags, nodes, queues)
    });
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn set_color(chain: Ptr, color: i32) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainSetColorSpace1;
    let original = original!(entry, unsafe extern "system" fn(Ptr, i32) -> i32);
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"swapchain", chain as u64);
        json.number(c"color_space", color as u32);
    }
    let status = original.map_or(0, |function| unsafe { function(chain, color) });
    if status >= 0 {
        LAST_COLOR.store(color, Relaxed);
        let callback = COLOR_SPACE.load(Acquire);
        if !callback.is_null() {
            unsafe { transmute::<Ptr, ColorObserver>(callback)(chain, color as u32) };
        }
    }
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn fullscreen(chain: Ptr, enabled: i32, output: Ptr) -> i32 {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainSetFullscreenState;
    let original = original!(entry, unsafe extern "system" fn(Ptr, i32, Ptr) -> i32);
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"swapchain", chain as u64);
        json.flag(c"fullscreen", enabled != 0);
        json.hex(c"target_output", output as u64);
    }
    let status = original.map_or(0, |function| unsafe { function(chain, enabled, output) });
    call.finish(result(status));
    status
}

#[inline(never)]
extern "system" fn waitable(chain: Ptr) -> Ptr {
    let pc = caller!();
    let entry = RSF_D3D_DxgiSwapChainGetFrameLatencyWaitableObject;
    let original = original!(entry, unsafe extern "system" fn(Ptr) -> Ptr);
    let mut slot = MaybeUninit::uninit();
    let mut call = Call::begin(&mut slot, id(entry), pc);
    if let Some(mut json) = call.json() {
        json.hex(c"swapchain", chain as u64);
    }
    let handle = original.map_or(null_mut(), |function| unsafe { function(chain) });
    call.finish(handle as u64);
    handle
}

pub(super) unsafe fn rsf_census_d3d_bind(entries: &[u16; COUNT], output: &mut [Ptr; COUNT]) {
    IDS.bind(entries);
    for (entry, wrapper) in [
        (RSF_D3D_CreateDXGIFactory, factory as Ptr),
        (RSF_D3D_CreateDXGIFactory1, factory1 as Ptr),
        (RSF_D3D_CreateDXGIFactory2, factory2 as Ptr),
        (RSF_D3D_D3D12CreateDevice, create_device as Ptr),
        (RSF_D3D_DxgiFactoryCreateSwapChain, create_swapchain as Ptr),
        (
            RSF_D3D_DxgiFactoryCreateSwapChainForHwnd,
            create_hwnd as Ptr,
        ),
        (
            RSF_D3D_DxgiFactoryCreateSwapChainForCoreWindow,
            create_core_window as Ptr,
        ),
        (RSF_D3D_DxgiSwapChainPresent, present as Ptr),
        (RSF_D3D_DxgiSwapChainPresent1, present1 as Ptr),
        (RSF_D3D_DxgiSwapChainResizeBuffers, resize as Ptr),
        (RSF_D3D_DxgiSwapChainResizeBuffers1, resize1 as Ptr),
        (RSF_D3D_DxgiSwapChainSetColorSpace1, set_color as Ptr),
        (RSF_D3D_DxgiSwapChainSetFullscreenState, fullscreen as Ptr),
        (
            RSF_D3D_DxgiSwapChainGetFrameLatencyWaitableObject,
            waitable as Ptr,
        ),
        (RSF_D3D_D3d12DeviceCreateCommandQueue, create_queue as Ptr),
        (RSF_D3D_D3d12QueueExecuteCommandLists, execute as Ptr),
    ] {
        output[entry as usize] = wrapper;
    }
}
