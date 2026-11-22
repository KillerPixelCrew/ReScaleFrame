// SPDX-License-Identifier: MIT
use super::*;
use crate::overlay_host_abi::{
    RSF_OVERLAY_INPUT_ABI_VERSION, rsf_overlay_input_options as Options,
};
use crate::overlay_input::{
    rsf_overlay_input_collect, rsf_overlay_input_install, rsf_overlay_input_set_visible,
    rsf_overlay_input_uninstall, rsf_overlay_input_visible,
};
use std::mem::{offset_of, transmute};
use std::ptr::null;
use windows_sys::Win32::System::{
    Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    SystemInformation::GetTickCount64,
    Threading::GetCurrentThreadId,
};
use windows_sys::core::GUID;

const UNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const DEVICE: GUID = GUID::from_u128(0xdb6f6ddb_ac77_4e88_8253_819df9bbf140);
const TEXTURE_2D: GUID = GUID::from_u128(0x6f15aaf2_d208_4e89_9ab4_489535d34f9c);

// Slots and structures follow the Windows SDK's C interface definitions in
// dxgi.h and d3d11.h, including the inherited interfaces.
const QUERY_INTERFACE: usize = 0;
const CHAIN_GET_DEVICE: usize = 7;
const CHAIN_GET_BUFFER: usize = 9;
const CHAIN_GET_DESC: usize = 12;
const DEVICE_GET_IMMEDIATE_CONTEXT: usize = 40;
const CHILD_GET_DEVICE: usize = 3;
const TEXTURE_GET_DESC: usize = 10;
const DEVICE_CREATE_RENDER_TARGET_VIEW: usize = 9;
const CONTEXT_OM_GET_RENDER_TARGETS: usize = 89;
const CONTEXT_OM_SET_RENDER_TARGETS: usize = 33;

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
    buffer_desc: ModeDesc,
    sample_desc: [u32; 2],
    buffer_usage: u32,
    buffer_count: u32,
    output_window: Handle,
    windowed: i32,
    swap_effect: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Default)]
struct Texture2dDesc {
    width: u32,
    height: u32,
    mip_levels: u32,
    array_size: u32,
    format: u32,
    sample_desc: [u32; 2],
    usage: u32,
    bind_flags: u32,
    cpu_access_flags: u32,
    misc_flags: u32,
}

const _: () = {
    assert!(size_of::<SwapChainDesc>() == 72);
    assert!(offset_of!(SwapChainDesc, output_window) == 48);
    assert!(size_of::<Texture2dDesc>() == 44);
    assert!(offset_of!(Texture2dDesc, bind_flags) == 32);
};

unsafe fn slot(object: Handle, index: usize) -> *const c_void {
    let vtable = unsafe { *object.cast::<*const *const c_void>() };
    unsafe { *vtable.add(index) }
}

macro_rules! com_call {
    ($object:ident, $slot:ident, $signature:ty, $($arg:expr),+) => {{
        unsafe {
            let method = transmute::<*const c_void, $signature>(slot($object, $slot));
            method($object, $($arg),+)
        }
    }};
}

unsafe extern "C" fn chain_device(chain: Handle, out: *mut Handle) -> i32 {
    com_call!(
        chain,
        CHAIN_GET_DEVICE,
        unsafe extern "system" fn(Handle, *const GUID, *mut Handle) -> i32,
        &DEVICE,
        out
    )
}

unsafe extern "C" fn chain_buffer(chain: Handle, out: *mut Handle) -> i32 {
    com_call!(
        chain,
        CHAIN_GET_BUFFER,
        unsafe extern "system" fn(Handle, u32, *const GUID, *mut Handle) -> i32,
        0,
        &TEXTURE_2D,
        out
    )
}

unsafe extern "C" fn chain_window(chain: Handle) -> Handle {
    let mut desc = SwapChainDesc::default();
    let status = com_call!(
        chain,
        CHAIN_GET_DESC,
        unsafe extern "system" fn(Handle, *mut SwapChainDesc) -> i32,
        &mut desc
    );
    if status < 0 {
        null_mut()
    } else {
        desc.output_window
    }
}

unsafe extern "C" fn same_object(first: Handle, second: Handle) -> u32 {
    let identity = |object: Handle| {
        if object.is_null() {
            return None;
        }
        let mut raw = null_mut();
        let status = com_call!(
            object,
            QUERY_INTERFACE,
            unsafe extern "system" fn(Handle, *const GUID, *mut Handle) -> i32,
            &UNKNOWN,
            &mut raw
        );
        unsafe { ComRef::from_owned(raw) }.filter(|_| status >= 0)
    };
    u32::from(
        identity(first).is_some_and(|a| identity(second).is_some_and(|b| a.as_ptr() == b.as_ptr())),
    )
}

unsafe extern "C" fn immediate_context(device: Handle) -> Handle {
    let mut context = null_mut();
    com_call!(
        device,
        DEVICE_GET_IMMEDIATE_CONTEXT,
        unsafe extern "system" fn(Handle, *mut Handle),
        &mut context
    );
    context
}

unsafe extern "C" fn texture_device(texture: Handle) -> Handle {
    let mut device = null_mut();
    com_call!(
        texture,
        CHILD_GET_DEVICE,
        unsafe extern "system" fn(Handle, *mut Handle),
        &mut device
    );
    device
}

unsafe extern "C" fn texture_desc(texture: Handle, out: *mut TextureDesc) {
    let mut desc = Texture2dDesc::default();
    com_call!(
        texture,
        TEXTURE_GET_DESC,
        unsafe extern "system" fn(Handle, *mut Texture2dDesc),
        &mut desc
    );
    unsafe {
        out.write(TextureDesc {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            bind_flags: desc.bind_flags,
        })
    };
}

unsafe extern "C" fn create_view(device: Handle, texture: Handle, out: *mut Handle) -> i32 {
    com_call!(
        device,
        DEVICE_CREATE_RENDER_TARGET_VIEW,
        unsafe extern "system" fn(Handle, Handle, *const c_void, *mut Handle) -> i32,
        texture,
        null(),
        out
    )
}

unsafe extern "C" fn save_targets(context: Handle, out: *mut Targets) {
    let out = unsafe { &mut *out };
    com_call!(
        context,
        CONTEXT_OM_GET_RENDER_TARGETS,
        unsafe extern "system" fn(Handle, u32, *mut Handle, *mut Handle),
        out.views.len() as u32,
        out.views.as_mut_ptr(),
        &mut out.depth
    );
}

unsafe extern "C" fn bind_target(context: Handle, view: Handle) {
    com_call!(
        context,
        CONTEXT_OM_SET_RENDER_TARGETS,
        unsafe extern "system" fn(Handle, u32, *const Handle, Handle),
        1,
        &view,
        null_mut()
    );
}

unsafe extern "C" fn restore_targets(context: Handle, saved: *const Targets) {
    let saved = unsafe { &*saved };
    com_call!(
        context,
        CONTEXT_OM_SET_RENDER_TARGETS,
        unsafe extern "system" fn(Handle, u32, *const Handle, Handle),
        saved.views.len() as u32,
        saved.views.as_ptr(),
        saved.depth
    );
}

#[repr(C)]
struct RendererSetup {
    struct_size: u32,
    abi_version: u32,
    log: Log,
    log_user: Handle,
}

unsafe extern "C" {
    fn rsf_overlay_renderer_create(
        device: Handle,
        setup: *const RendererSetup,
        out: *mut Handle,
    ) -> i32;
    fn rsf_overlay_renderer_upload_texture(
        renderer: Handle,
        context: Handle,
        update: *const rsf_overlay_texture_update,
    ) -> i32;
    fn rsf_overlay_renderer_free_texture(renderer: Handle, id: u64);
    fn rsf_overlay_renderer_draw(
        renderer: Handle,
        context: Handle,
        data: *const rsf_overlay_draw_data,
        width: u32,
        height: u32,
    ) -> i32;
    fn rsf_overlay_renderer_destroy(renderer: Handle);
}

unsafe extern "C" fn renderer_create(
    device: Handle,
    log: Log,
    user: Handle,
    out: *mut Handle,
) -> i32 {
    let setup = RendererSetup {
        struct_size: size_of::<RendererSetup>() as u32,
        abi_version: 1,
        log,
        log_user: user,
    };
    unsafe { rsf_overlay_renderer_create(device, &setup, out) }
}

unsafe extern "C" fn input_install(window: Handle, toggle: u32, log: Log, user: Handle) -> i32 {
    let options = Options {
        struct_size: size_of::<Options>() as u32,
        abi_version: RSF_OVERLAY_INPUT_ABI_VERSION,
        toggle_virtual_key: toggle,
        log,
        log_user: user,
    };
    unsafe { rsf_overlay_input_install(window, &options) }
}

unsafe extern "C" fn input_uninstall() {
    rsf_overlay_input_uninstall();
}

unsafe extern "C" fn input_collect(
    out: *mut rsf_overlay_input,
    width: u32,
    height: u32,
    delta: f32,
) {
    unsafe { rsf_overlay_input_collect(out, width, height, delta) };
}

unsafe extern "C" fn tick() -> u64 {
    unsafe { GetTickCount64() }
}

unsafe extern "C" fn counter(out: *mut i64) -> u32 {
    u32::from(unsafe { QueryPerformanceCounter(out) } != 0)
}

unsafe extern "C" fn frequency(out: *mut i64) -> u32 {
    u32::from(unsafe { QueryPerformanceFrequency(out) } != 0)
}

unsafe extern "C" fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

static PLATFORM: Platform = Platform {
    chain_device,
    chain_buffer,
    chain_window,
    same_object,
    immediate_context,
    texture_device,
    texture_desc,
    create_view,
    save_targets,
    bind_target,
    restore_targets,
    renderer_create,
    renderer_upload: rsf_overlay_renderer_upload_texture,
    renderer_free: rsf_overlay_renderer_free_texture,
    renderer_draw: rsf_overlay_renderer_draw,
    renderer_destroy: rsf_overlay_renderer_destroy,
    input_install,
    input_uninstall,
    input_visible: rsf_overlay_input_visible,
    input_set_visible: rsf_overlay_input_set_visible,
    input_collect,
    tick,
    counter,
    frequency,
    thread_id,
};

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_start(
    chain: Handle,
    log: Log,
    user: Handle,
) -> i32 {
    unsafe { rsf_core_overlay_host_start(&PLATFORM, chain, log, user) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_host_start_device(
    device: Handle,
    window: Handle,
    log: Log,
    user: Handle,
) -> i32 {
    unsafe { rsf_core_overlay_host_start_device(&PLATFORM, device, window, log, user) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_host_draw_target(
    context: Handle,
    target: Handle,
    width: u32,
    height: u32,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    unsafe {
        rsf_core_overlay_host_draw_target(
            &PLATFORM, context, target, width, height, stats, intent, None,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_host_admit_target(
    show_hud: u32,
    visible: *mut u32,
    hint: *mut f32,
) -> i32 {
    if visible.is_null() || hint.is_null() || !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    let Some(admission) = try_hold().and_then(|host| host.admit_target(&PLATFORM, show_hud != 0))
    else {
        return 0;
    };
    unsafe {
        visible.write(u32::from(admission.visible));
        hint.write(admission.hint);
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_host_draw_admitted_target(
    context: Handle,
    target: Handle,
    width: u32,
    height: u32,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    visible: u32,
    hint: f32,
) -> i32 {
    let admission = DrawAdmission {
        visible: visible != 0,
        hint,
    };
    unsafe {
        rsf_core_overlay_host_draw_target(
            &PLATFORM,
            context,
            target,
            width,
            height,
            stats,
            intent,
            Some(admission),
        )
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_host_visible() -> u32 {
    rsf_core_overlay_host_visible(&PLATFORM)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_host_toggle() {
    rsf_core_overlay_host_toggle(&PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_present(
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    if !unsafe { crate::sr_session::has_capacity(stats) } {
        return 0;
    }
    let mut stats = unsafe { stats.read() };
    if !stats.saved_rows.is_null() {
        return unsafe { rsf_core_overlay_host_present(&PLATFORM, chain, &stats, intent) };
    }
    // Linked carriers that present directly still have the session-wide rows.
    let saved = crate::overlay_saved::Frame::new(null_mut());
    saved.attach(&mut stats);
    let result = unsafe { rsf_core_overlay_host_present(&PLATFORM, chain, &stats, intent) };
    if result != 0
        && let Some(intent) = unsafe { intent.as_ref() }
    {
        saved.apply(intent);
    }
    result
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_overlay_host_stop() {
    rsf_core_overlay_host_stop(&PLATFORM);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_host_stop_device(device: Handle) {
    rsf_core_overlay_host_stop_device(&PLATFORM, device);
}
