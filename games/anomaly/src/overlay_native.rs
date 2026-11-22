// SPDX-License-Identifier: MIT
//! Windows SDK calls used by the Anomaly overlay's D3D11 and DirectInput boundaries.
use crate::lease::Lease;
use crate::overlay_abi::{
    rsf_anomaly_overlay_chain, rsf_input_dump_named_texture_fn, rsf_named_frame_dump,
    rsf_named_frame_dump_report,
};
use std::ffi::c_void;
use std::mem::transmute;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{HMODULE, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::core::{GUID, HRESULT};

pub(super) type Object = *mut c_void;
type Symbol = unsafe extern "system" fn() -> isize;
type GetInterface = unsafe extern "system" fn(Object, *const GUID, *mut Object) -> HRESULT;
const SLOT_QUERY_INTERFACE: usize = 0;
const SLOT_RELEASE: usize = 2;
const SLOT_CREATE_DEVICE: usize = 3;
const SLOT_CHAIN_GET_DEVICE: usize = 7;
const SLOT_CHAIN_GET_BUFFER: usize = 9;
const SLOT_CHAIN_GET_DESC: usize = 12;
const SLOT_GET_IMMEDIATE_CONTEXT: usize = 40;

const CHAIN1: GUID = GUID::from_u128(0x790a45f7_0d42_4876_983a_0a55cfe6f4aa);
const DEVICE: GUID = GUID::from_u128(0xdb6f6ddb_ac77_4e88_8253_819df9bbf140);
const TEXTURE_2D: GUID = GUID::from_u128(0x6f15aaf2_d208_4e89_9ab4_489535d34f9c);

#[repr(C)]
#[derive(Default)]
struct ModeDesc {
    width: u32,
    height: u32,
    refresh_rate: [u32; 2],
    format: u32,
    scanline_ordering: u32,
    scaling: u32,
}

#[repr(C)]
#[derive(Default)]
struct SwapChainDesc {
    mode: ModeDesc,
    sample: [u32; 2],
    usage: u32,
    count: u32,
    window: HWND,
    windowed: i32,
    swap_effect: u32,
    flags: u32,
}

const _: () = {
    assert!(size_of::<SwapChainDesc>() == 72);
    assert!(std::mem::offset_of!(SwapChainDesc, window) == 48);
};

pub(super) unsafe fn slot(object: Object, index: usize) -> usize {
    let table = unsafe { *object.cast::<*const usize>() };
    unsafe { *table.add(index) }
}

pub(super) unsafe extern "C" fn release(object: Object) {
    let call: unsafe extern "system" fn(Object) -> u32 =
        unsafe { transmute(slot(object, SLOT_RELEASE)) };
    unsafe { call(object) };
}

pub(super) type CreateDevice =
    unsafe extern "system" fn(Object, *const GUID, *mut Object, Object) -> HRESULT;

pub(super) unsafe fn create_device(input: Object, guid: *const GUID) -> Object {
    let create: CreateDevice = unsafe { transmute(slot(input, SLOT_CREATE_DEVICE)) };
    let mut device = null_mut();
    if unsafe { create(input, guid, &mut device, null_mut()) } < 0 {
        return null_mut();
    }
    device
}

pub(super) unsafe fn create_input(address: Symbol, iid: *const GUID, version: u32) -> Object {
    type Create =
        unsafe extern "system" fn(HMODULE, u32, *const GUID, *mut Object, Object) -> HRESULT;
    let create: Create = unsafe { transmute(address) };
    let mut input = null_mut();
    if unsafe {
        create(
            GetModuleHandleW(std::ptr::null()),
            version,
            iid,
            &mut input,
            null_mut(),
        )
    } < 0
    {
        return null_mut();
    }
    input
}

pub(super) unsafe fn query_chain1(chain: Object) -> Object {
    let call: GetInterface = unsafe { transmute(slot(chain, SLOT_QUERY_INTERFACE)) };
    let mut result = null_mut();
    if unsafe { call(chain, &CHAIN1, &mut result) } < 0 {
        return null_mut();
    }
    result
}

#[derive(Default)]
#[repr(C)]
pub(super) struct ChainObjects {
    pub chain: Object,
    pub context: Object,
    pub device: Object,
}

pub(super) unsafe fn create_warp_chain(
    address: Symbol,
    window: HWND,
    extent: u32,
    objects: *mut ChainObjects,
) -> HRESULT {
    type Create = unsafe extern "system" fn(
        Object,
        u32,
        HMODULE,
        u32,
        *const u32,
        u32,
        u32,
        *const SwapChainDesc,
        *mut Object,
        *mut Object,
        *mut u32,
        *mut Object,
    ) -> HRESULT;
    const WARP: u32 = 5;
    const SDK_VERSION: u32 = 7;
    const RGBA8_UNORM: u32 = 28;
    const RENDER_TARGET_OUTPUT: u32 = 0x20;
    let create: Create = unsafe { transmute(address) };
    let description = SwapChainDesc {
        mode: ModeDesc {
            width: extent,
            height: extent,
            format: RGBA8_UNORM,
            ..ModeDesc::default()
        },
        sample: [1, 0],
        usage: RENDER_TARGET_OUTPUT,
        count: 1,
        window,
        windowed: 1,
        ..SwapChainDesc::default()
    };
    let objects = unsafe { &mut *objects };
    unsafe {
        create(
            null_mut(),
            WARP,
            null_mut(),
            0,
            std::ptr::null(),
            0,
            SDK_VERSION,
            &description,
            &mut objects.chain,
            &mut objects.device,
            null_mut(),
            &mut objects.context,
        )
    }
}

unsafe fn get_device(chain: Object) -> Option<Lease> {
    let call: GetInterface = unsafe { transmute(slot(chain, SLOT_CHAIN_GET_DEVICE)) };
    let mut raw = null_mut();
    let result = unsafe { call(chain, &DEVICE, &mut raw) };
    Lease::new(raw, release).filter(|_| result >= 0)
}

pub(super) unsafe extern "C" fn describe_chain(
    chain: Object,
    out: *mut rsf_anomaly_overlay_chain,
) -> u32 {
    let call: unsafe extern "system" fn(Object, *mut SwapChainDesc) -> HRESULT =
        unsafe { transmute(slot(chain, SLOT_CHAIN_GET_DESC)) };
    let mut desc = SwapChainDesc::default();
    if unsafe { call(chain, &mut desc) } < 0 {
        return 0;
    }
    let device = unsafe { get_device(chain) };
    unsafe {
        out.write(rsf_anomaly_overlay_chain {
            window: desc.window,
            device: device
                .as_ref()
                .map_or(std::ptr::null(), |device| device.as_ptr().cast_const()),
            width: desc.mode.width,
            height: desc.mode.height,
        });
    }
    1
}

pub(super) unsafe extern "C" fn capture_backbuffer(
    chain: Object,
    dump: rsf_input_dump_named_texture_fn,
    request: *const rsf_named_frame_dump,
    report: *mut rsf_named_frame_dump_report,
) -> i32 {
    let Some(dump) = dump else { return -1 };
    let Some(device) = (unsafe { get_device(chain) }) else {
        return -1;
    };
    let immediate_context: unsafe extern "system" fn(Object, *mut Object) =
        unsafe { transmute(slot(device.as_ptr(), SLOT_GET_IMMEDIATE_CONTEXT)) };
    let mut context = null_mut();
    unsafe { immediate_context(device.as_ptr(), &mut context) };
    let Some(context) = Lease::new(context, release) else {
        return -1;
    };
    let get_buffer: unsafe extern "system" fn(Object, u32, *const GUID, *mut Object) -> HRESULT =
        unsafe { transmute(slot(chain, SLOT_CHAIN_GET_BUFFER)) };
    let mut buffer = null_mut();
    let result = unsafe { get_buffer(chain, 0, &TEXTURE_2D, &mut buffer) };
    let buffer = Lease::new(buffer, release).filter(|_| result >= 0);
    buffer.map_or(-1, |buffer| unsafe {
        dump(context.as_ptr(), buffer.as_ptr(), request, report)
    })
}
