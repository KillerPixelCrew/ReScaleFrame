// SPDX-License-Identifier: MIT
//! The Godot Vulkan bridge. The graphics guard serializes borrowed Runtime and
//! engine objects. Runtime capture callbacks from nested SDK WSI bypass the
//! bridge while that guard is held.

use super::creation::abi::{self, *};
use super::{BACKEND_ERROR_NOT_READY, BACKEND_ERROR_NOT_SUPPORTED, BACKEND_OK};
use crate::platform::*;
use crate::{CONTROL, rsf_godot_vk_creation_resolver};
use rsf_game_sys::ffi_util::ascii_wide;
use rsf_game_sys::win32::typed_export;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_FRAME_ABI_VERSION, RSF_GAME_RENDER_FINAL_SCENE,
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_frame_record, rsf_game_log_fn,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{size_of, size_of_val};
use std::ptr;
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};

mod capture;
#[path = "../../../../runtime/orchestrator/src/vulkan_sr_session/linked.rs"]
mod linked;

type Handle = *mut c_void;
const INVALID: i32 = -1;
const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;
const VK_INIT_FAILED: i32 = -3;
const CAPABILITY_SR: u32 = 1;
const CAPABILITY_FSR4_SR: u32 = 2;
const CAPABILITY_FSR4_FG: u32 = 4;
const CAPABILITY_XESS_FG: u32 = 8;
const SHADER_READ: u32 = 5;
const TRANSFER_DST: u32 = 7;
const PRESENT_LAYOUT: u32 = 1_000_001_002;

#[repr(C)]
struct NativeFeatures {
    flags: u32,
    reserved: u32,
    fsr4_probe: Option<unsafe extern "C" fn(*const rsf_srvk_setup, *mut rsf_fsr4fg_info) -> i32>,
    capture: capture::Ops,
}

unsafe extern "C" {
    fn rsf_godot_platform_leaves() -> *const Platform;
    fn rsf_godot_vk_native_features() -> *const NativeFeatures;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: Handle, flags: u32) -> Handle;
}

const _: () = {
    assert!(size_of::<NativeFeatures>() == 48);
    assert!(std::mem::offset_of!(NativeFeatures, capture) == 16);
};

fn features() -> &'static NativeFeatures {
    // The CMake-configured native table is immutable and part of this module.
    unsafe { &*rsf_godot_vk_native_features() }
}

fn platform() -> &'static Platform {
    // Native entry points and their checked Godot table ship in the same DLL.
    unsafe { &*rsf_godot_platform_leaves() }
}

static GUARD: Mutex<()> = Mutex::new(());
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
static RESOLVER: RwLock<Option<VkProc>> = RwLock::new(None);

#[derive(Default)]
struct State {
    directory: CString,
    streamline: CString,
    fsr4: CString,
    intel: Vec<u16>,
    frame: u64,
    explicit: bool,
    log: rsf_game_log_fn,
    log_user: Handle,
    presenter: *mut rsf_fgvk,
    graphics: rsf_srvk_setup,
    shutdown_pending: bool,
    graph_handoff: bool,
    streamline_ready: bool,
    capture: capture::Capture,
}

// Device, presenter and logger borrows have engine lifetimes. The graphics guard
// serializes their consumers, and the state mutex protects publication.
unsafe impl Send for State {}

fn guard() -> MutexGuard<'static, ()> {
    GUARD.lock().unwrap_or_else(PoisonError::into_inner)
}

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn log(message: &CStr) {
    let (logger, user) = {
        let state = state();
        (state.log, state.log_user)
    };
    if let Some(logger) = logger {
        unsafe { logger(user, message.as_ptr()) };
    }
}

fn log_text(message: String) {
    if let Ok(message) = CString::new(message) {
        log(&message);
    }
}

unsafe fn checked<'a, T>(value: *const T) -> Option<&'a T> {
    if value.is_null() || (unsafe { value.cast::<u32>().read() } as usize) < size_of::<T>() {
        return None;
    }
    // Callers lend these records through the full synchronous native entry point.
    Some(unsafe { &*value })
}

fn runtime_path(root: &CStr, suffix: &[u8]) -> CString {
    let mut path = root.to_bytes().to_vec();
    path.extend_from_slice(suffix);
    CString::new(path).expect("runtime suffix has no NUL")
}

fn system_resolver() -> Option<VkProc> {
    static SYSTEM: OnceLock<Option<VkProc>> = OnceLock::new();
    *SYSTEM.get_or_init(|| {
        const DLL: [u16; 13] = ascii_wide(b"vulkan-1.dll\0");
        // Keep this loader mapped through Godot's late Vulkan destruction.
        let module =
            unsafe { LoadLibraryExW(DLL.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        unsafe { typed_export::<VkProc>(module, c"vkGetInstanceProcAddr") }
    })
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsfg_vulkan_supported() -> i32 {
    i32::from(features().flags & CAPABILITY_SR != 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_leaf_supported() -> u32 {
    rsfg_vulkan_supported() as u32
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_locked(
    body: unsafe extern "C" fn(Handle) -> i32,
    user: Handle,
) -> i32 {
    let _guard = guard();
    unsafe { body(user) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_start(setup: *const Setup) -> i32 {
    let setup = unsafe { &*setup };
    let root = unsafe { CStr::from_ptr(setup.runtime_directory) };
    let (streamline, xess) = {
        let mut state = state();
        state.frame = 0;
        state.explicit = false;
        state.streamline_ready = false;
        state.directory = runtime_path(root, b"/fidelityfx-vulkan");
        state.fsr4 = runtime_path(root, b"/fidelityfx");
        state.streamline = runtime_path(root, b"/streamline-vulkan");
        state.log = setup.log;
        state.log_user = setup.log_user;
        (state.streamline.as_ptr(), runtime_path(root, b"/xess"))
    };
    let latency = rsf_slvk_setup {
        struct_size: size_of::<rsf_slvk_setup>() as u32,
        abi_version: RSF_SLVK_ABI_VERSION,
        runtime_directory_utf8: streamline,
        project_id: setup.project_id,
        features: RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG,
    };
    let latency_result = unsafe { rsf_slvk_initialize(&latency) };
    let Ok(intel) = xess.to_str() else { return 0 };
    state().intel = intel.encode_utf16().chain(Some(0)).collect();
    let xess = rsf_xessvk_setup {
        struct_size: size_of::<rsf_xessvk_setup>() as u32,
        abi_version: RSF_XESSVK_REQUIREMENTS_ABI_VERSION,
        runtime_directory_utf8: xess.as_ptr(),
    };
    let xess_result = unsafe { rsf_xessvk_initialize(&xess) };
    let base =
        if unsafe { rsf_latencyvk_instance_proc(ptr::null_mut(), c"vkCreateInstance".as_ptr()) }
            .is_some()
        {
            Some(rsf_latencyvk_instance_proc as VkProc)
        } else {
            system_resolver()
        };
    let resolver = if rsfg_vulkan_supported() != 0 {
        let requirements = super::creation::typed::Requirements::linked();
        unsafe {
            rsf_godot_vk_creation_resolver(
                base,
                u32::from(xess_result == BACKEND_OK),
                setup.log,
                setup.log_user,
                &requirements,
            )
        }
    } else {
        base
    };
    *RESOLVER.write().unwrap_or_else(PoisonError::into_inner) = resolver;
    state().streamline_ready = latency_result == BACKEND_OK;
    log(if latency_result == BACKEND_OK {
        c"Godot Vulkan host ready with Streamline Vulkan dispatch and PCL startup."
    } else {
        c"Godot Vulkan host ready. Streamline Vulkan initialization failed; platform latency capabilities are queried at device creation."
    });
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_leaf_stop() -> i32 {
    {
        let mut state = state();
        let user = state.capture.user();
        if state.capture.release(user) != BACKEND_OK {
            return 0;
        }
    }
    if unsafe { rsf_xessvk_shutdown() } != BACKEND_OK {
        return 0;
    }
    CONTROL.vulkan.creation().stop();
    // A surviving presenter needs the SDK through swapchain retirement.
    let pending = {
        let mut state = state();
        state.shutdown_pending = !state.presenter.is_null();
        state.shutdown_pending
    };
    if !pending && unsafe { rsf_latencyvk_shutdown() } != BACKEND_OK {
        return 0;
    }
    let mut state = state();
    state.streamline_ready = false;
    state.capture.stop();
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_leaf_release(session: u64) {
    unsafe { rsf_input_dump_cancel(session) };
    state().directory = CString::default();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_latency_facts(out: *mut VkLatencyFacts) -> u32 {
    let mut latency = rsf_latencyvk_status {
        struct_size: size_of::<rsf_latencyvk_status>() as u32,
        abi_version: RSF_LATENCYVK_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { rsf_latencyvk_get_status(&mut latency) } != BACKEND_OK {
        return 0;
    }
    unsafe {
        out.write(VkLatencyFacts {
            low_latency_available: latency.low_latency_available,
            report_available: latency.report_available,
            pcl_message: latency.pcl_message,
            mode: latency.mode,
            owner: latency.owner,
            sleep_calls: latency.sleep_calls,
            marker_calls: latency.marker_calls,
            ..Default::default()
        })
    };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_facts(out: *mut VkFacts) {
    let state = state();
    let mut backend = 0;
    let present = !state.presenter.is_null();
    if present {
        let _ = unsafe { rsf_fgvk_backend(state.presenter, &mut backend) };
    }
    let flags = features().flags;
    unsafe {
        out.write(VkFacts {
            streamline_ready: u32::from(state.streamline_ready),
            graph_handoff: u32::from(state.graph_handoff),
            fsr4_sr_compiled: u32::from(flags & CAPABILITY_FSR4_SR != 0),
            fsr4_fg_compiled: u32::from(flags & CAPABILITY_FSR4_FG != 0),
            xess_fg_compiled: u32::from(flags & CAPABILITY_XESS_FG != 0),
            presenter: u32::from(present),
            presenter_backend: backend,
            sdk_shutdown_pending: u32::from(state.shutdown_pending),
            ..Default::default()
        })
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_presenter_max(count: *mut u32) -> i32 {
    let owner = state().presenter;
    if owner.is_null() {
        return BACKEND_ERROR_NOT_READY;
    }
    unsafe { rsf_fgvk_max_generated(owner, count) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_leaf_fsr4_fg_probe() -> u32 {
    let Some(probe) = features().fsr4_probe else {
        return 0;
    };
    let setup = {
        let state = state();
        rsf_srvk_setup {
            runtime_directory_utf8: state.fsr4.as_ptr(),
            ..state.graphics
        }
    };
    let mut info = rsf_fsr4fg_info {
        struct_size: size_of::<rsf_fsr4fg_info>() as u32,
        abi_version: RSF_FSR4FG_BRIDGE_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { probe(&setup, &mut info) } == BACKEND_OK {
        info.max_generated
    } else {
        0
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_latency_options(mode: u32, frame_limit: u32) -> i32 {
    unsafe { rsf_latencyvk_options(mode, frame_limit) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_godot_vk_leaf_presenter_drain(disable: u32) -> i32 {
    let owner = state().presenter;
    if owner.is_null() {
        return BACKEND_OK;
    }
    if disable != 0 {
        let _ = unsafe { rsf_fgvk_enable(owner, 0) };
    }
    unsafe { rsf_fgvk_drain(owner) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_device_idle(device: Handle, proc: Option<VkProc>) -> u32 {
    let wait = proc.and_then(|get| {
        // Win64 Vulkan dispatch uses PFN_vkVoidFunction as its transport.
        unsafe {
            std::mem::transmute::<_, PFN_vkDeviceWaitIdle>(get(
                device,
                c"vkDeviceWaitIdle".as_ptr(),
            ))
        }
    });
    u32::from(wait.is_some_and(|wait| unsafe { wait(device.cast()) } == VK_SUCCESS))
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_resolver() -> Option<VkProc> {
    *RESOLVER.read().unwrap_or_else(PoisonError::into_inner)
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_host_capabilities(flags: u32) -> i32 {
    let _guard = guard();
    if CONTROL.vulkan.booted() || flags & !RSF_GODOT_VK_GRAPH_HANDOFF != 0 {
        return 0;
    }
    state().graph_handoff = flags & RSF_GODOT_VK_GRAPH_HANDOFF != 0;
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_boot_vulkan(setup: *const Setup) -> i32 {
    unsafe { crate::rsf_godot_boot_vulkan(platform(), setup) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_godot_vk_render_begin(frame: u64, explicit: u32) {
    if explicit == 0 {
        unsafe { rsf_latencyvk_render(frame) };
    }
    let _guard = guard();
    let mut state = state();
    state.frame = frame;
    state.explicit = explicit != 0;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_create(
    setup: *const rsf_fgvk_setup,
    out: *mut Handle,
    chain: *mut u64,
) -> i32 {
    if !CONTROL.vulkan.booted() || out.is_null() || chain.is_null() {
        return INVALID;
    }
    let Some(setup) = (unsafe { checked(setup) }) else {
        return INVALID;
    };
    let _guard = guard();
    if !state().presenter.is_null() {
        return BACKEND_ERROR_NOT_SUPPORTED;
    }
    CONTROL
        .vulkan
        .creation()
        .report_render_device(setup.graphics.device);
    let (view, _) = CONTROL.vulkan.view(platform());
    let requested = view.settings;
    let selected = view.fg_backend;
    let mut value = *setup;
    let readable = capture::readable_create(setup);
    if let Some(readable) = readable.as_ref() {
        value.create_info = ptr::from_ref(readable).cast();
        state().capture.setup(&value);
    }
    let (directory, intel) = {
        let state = state();
        let directory = match selected {
            super::FG_DLSS => &state.streamline,
            super::FG_FSR4 => &state.fsr4,
            _ => &state.directory,
        };
        (directory.as_ptr(), state.intel.as_ptr())
    };
    value.graphics.runtime_directory_utf8 = directory;
    let host = rsf_fgvk_host_options {
        struct_size: size_of::<rsf_fgvk_host_options>() as u32,
        abi_version: RSF_FGVK_HOST_ABI_VERSION,
        window: CONTROL.vulkan.primary_window(),
        timeline_enabled: u32::from(CONTROL.vulkan.creation().interop_ready()),
        intel_runtime_directory: intel,
    };
    let mut owner = ptr::null_mut();
    let result = unsafe {
        rsf_fgvk_create_with_host(
            &value,
            selected,
            requested.generated_frames,
            &host,
            &mut owner,
            chain,
        )
    };
    state().presenter = owner;
    unsafe { out.write(owner.cast()) };
    let ready = result == BACKEND_OK && !owner.is_null();
    let mut maximum = 0;
    if ready {
        let _ = unsafe { rsf_fgvk_max_generated(owner, &mut maximum) };
    }
    CONTROL.vulkan.presenter_created(ready, selected, maximum);
    if ready {
        state().graphics = rsf_srvk_setup {
            runtime_directory_utf8: ptr::null(),
            ..setup.graphics
        };
        let _ = unsafe { rsf_latencyvk_presentation(owner.cast()) };
        let _ = unsafe { rsf_latencyvk_options(requested.latency_mode, requested.frame_limit_us) };
    }
    let queues = if owner.is_null() { 0 } else { 4 };
    log_text(format!(
        "Godot Vulkan swapchain owner: create={result} reserved queues={queues}"
    ));
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_capture_setup(setup: *const rsf_fgvk_setup) {
    if !CONTROL.vulkan.booted() {
        return;
    }
    let Some(setup) = (unsafe { checked(setup) }) else {
        return;
    };
    if setup.graphics.instance_proc.is_none() || setup.graphics.device_proc.is_none() {
        return;
    }
    let _guard = guard();
    CONTROL
        .vulkan
        .creation()
        .report_render_device(setup.graphics.device);
    state().capture.setup(setup);
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_capture_release(user: Handle) -> i32 {
    let _guard = guard();
    state().capture.release(user)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_images(owner: Handle, count: *mut u32, images: *mut u64) -> i32 {
    unsafe { rsf_fgvk_images(owner.cast(), count, images) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_acquire(
    owner: Handle,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let _guard = guard();
    let mut backend = 0;
    if unsafe { rsf_fgvk_backend(owner.cast(), &mut backend) } != BACKEND_OK {
        return VK_INIT_FAILED;
    }
    if !CONTROL.vulkan.presenter_current(platform(), backend) {
        return VK_ERROR_OUT_OF_DATE_KHR;
    }
    unsafe { rsf_fgvk_acquire(owner.cast(), timeout, semaphore, fence, index) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_image_layout(owner: Handle) -> u32 {
    let mut layout = PRESENT_LAYOUT;
    if !owner.is_null() {
        let _ = unsafe { rsf_fgvk_image_layout(owner.cast(), &mut layout) };
    }
    layout
}

fn capture_present(owner: *mut rsf_fgvk, info: *const c_void, result: i32, begin: bool) {
    if features().capture.snapshot.is_none() {
        return;
    }
    let armed = unsafe { rsf_input_dump_requested() } != 0;
    unsafe { rsf_input_dump_poll() };
    if !armed || info.is_null() {
        return;
    }
    let frame = {
        let state = state();
        if !state.capture.can_present() {
            return;
        }
        state.frame
    };
    if frame == 0 {
        return;
    }
    let (view, _) = CONTROL.vulkan.view(platform());
    let refused = state()
        .capture
        .present(owner, info, result, begin, &view, frame);
    if refused {
        log(c"Core frame dump: final Vulkan image could not be read; see the bundle's missing stage.");
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_godot_vk_capture_present(
    info: *const c_void,
    result: i32,
    begin: u32,
) {
    // A nested SDK WSI call is already inside the provider-owned Present.
    if let Ok(_guard) = GUARD.try_lock() {
        capture_present(ptr::null_mut(), info, result, begin != 0);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_present(owner: Handle, info: *const c_void) -> i32 {
    let _guard = guard();
    let (explicit, frame) = {
        let state = state();
        (state.explicit, state.frame)
    };
    if !explicit {
        unsafe { rsf_latencyvk_submit_end(frame) };
    }
    capture_present(owner.cast(), info, 0, true);
    let result = unsafe { rsf_fgvk_present(owner.cast(), info) };
    capture_present(owner.cast(), info, result, false);
    let mut status = rsf_fgvk_status {
        struct_size: size_of::<rsf_fgvk_status>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { rsf_fgvk_get_status(owner.cast(), &mut status) } == BACKEND_OK {
        CONTROL.vulkan.presented(&VkPresentFacts {
            enabled: status.enabled,
            result: status.result,
            generated_callbacks: status.generated_callbacks,
            presented: status.last_present_count,
            tagged: status.tagged,
        });
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_recording_begin(commands: Handle) {
    let _guard = guard();
    state().capture.retire(commands);
    // Godot reports the recycled recording only after its previous use retired.
    unsafe { linked::rsf_core_srvk_session_retire(CONTROL.vulkan.sr_context(), commands) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_submitted(commands: *const Handle, count: u32) {
    let _guard = guard();
    let owner = state().presenter;
    if !owner.is_null() {
        unsafe { rsf_fgvk_submitted(owner, commands, count) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_destroy(owner: Handle) -> i32 {
    let _guard = guard();
    {
        let mut state = state();
        if owner == state.presenter.cast() {
            let user = state.capture.user();
            if state.capture.release(user) != BACKEND_OK {
                return BACKEND_ERROR_NOT_READY;
            }
        }
    }
    let result = unsafe { rsf_fgvk_destroy(owner.cast()) };
    let (retired, shutdown_pending) = {
        let mut state = state();
        let retired = result == BACKEND_OK && owner == state.presenter.cast();
        if retired {
            state.presenter = ptr::null_mut();
            state.graphics = rsf_srvk_setup::default();
        }
        (retired, state.shutdown_pending)
    };
    if retired {
        CONTROL.vulkan.presenter_retired();
        if shutdown_pending {
            if unsafe { rsf_latencyvk_shutdown() } == BACKEND_OK {
                state().shutdown_pending = false;
            } else {
                log(c"Godot Vulkan SDK shutdown refused after owner retirement; retaining the mapped session.");
            }
        }
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_drain(owner: Handle) -> i32 {
    let _guard = guard();
    unsafe { rsf_fgvk_drain(owner.cast()) }
}

fn image(packet: &rsf_godot_vk_packet, value: &rsf_godot_vk_image, layout: u32) -> rsf_srvk_image {
    rsf_srvk_image {
        device: packet.device,
        image: value.image,
        format: value.format,
        width: value.width,
        height: value.height,
        layout,
        copy_source: value.copy_source,
    }
}

struct Recording<'a> {
    commands: Handle,
    continuation: *mut Handle,
    packet: &'a rsf_godot_vk_packet,
    handoff: Option<&'a rsf_godot_vk_handoff>,
    record: &'a rsf_frame_record,
    plan: [u32; 2],
}

fn empty_frame_record() -> rsf_frame_record {
    // Generated from game_frame.h: numeric values and fixed numeric arrays.
    unsafe { std::mem::zeroed() }
}

unsafe fn render(
    commands: Handle,
    packet: *const rsf_godot_vk_packet,
    handoff: Option<&rsf_godot_vk_handoff>,
    continuation: *mut Handle,
) -> i32 {
    if !continuation.is_null() {
        unsafe { continuation.write(commands) };
    }
    if !CONTROL.vulkan.booted() || commands.is_null() {
        return 0;
    }
    let Some(packet) = (unsafe { checked(packet) }) else {
        return 0;
    };
    let pass = &packet.pass;
    if packet.abi_version != RSF_GODOT_ABI_VERSION
        || packet.device.is_null()
        || packet.instance_proc.is_none()
        || packet.device_proc.is_none()
        || pass.struct_size < size_of_val(pass) as u32
        || !matches!(pass.role, RSF_GAME_RENDER_SR | RSF_GAME_RENDER_FINAL_SCENE)
        || pass.camera_valid == 0
    {
        return 0;
    }

    let _guard = guard();
    if crate::depth_domains_requested(&pass.depth_domains) {
        return 0;
    }

    let camera = pass.camera;
    let record = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        session_id: pass.session_id,
        frame_id: pass.source_frame_id,
        view_id: pass.view_key as u32,
        resource_generation: pass.resource_generation,
        camera,
        frame_time_ms: camera.frame_time_seconds * 1000.0,
        render_width: camera.render_width,
        render_height: camera.render_height,
        output_width: camera.output_width,
        output_height: camera.output_height,
        flags: if pass.flags & RSF_GAME_RENDER_RESET != 0 {
            RSF_FRAME_FLAG_RESET
        } else {
            0
        },
        ..empty_frame_record()
    };
    if features().capture.snapshot.is_some() {
        let backend = CONTROL.vulkan.settings().0.sr_backend;
        state().capture.render(commands, packet, record, backend);
    }
    let fg = rsf_fgvk_frame {
        struct_size: size_of::<rsf_fgvk_frame>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        record: &record,
        depth: image(packet, &packet.depth, SHADER_READ),
        motion: image(packet, &packet.motion, SHADER_READ),
        hudless: image(packet, &packet.color, SHADER_READ),
        motion_scale_x: -(camera.render_width as f32) * pass.motion_to_uv[0],
        motion_scale_y: -(camera.render_height as f32) * pass.motion_to_uv[1],
    };
    let (owner, matches) = {
        let state = state();
        (
            state.presenter,
            !state.presenter.is_null() && state.graphics.device == packet.device,
        )
    };
    let fg_result = CONTROL.vulkan.fg_pass(
        pass.screen,
        matches,
        |enabled, count| unsafe { rsf_fgvk_enable_options(owner, enabled, count) },
        || unsafe {
            if pass.role == RSF_GAME_RENDER_SR {
                rsf_fgvk_prepare(owner, commands, &fg)
            } else {
                rsf_fgvk_hudless(owner, commands, &fg)
            }
        },
    );
    if pass.role == RSF_GAME_RENDER_FINAL_SCENE {
        return i32::from(fg_result == BACKEND_OK);
    }

    let mut recording = Recording {
        commands,
        continuation,
        packet,
        handoff,
        record: &record,
        plan: [0; 2],
    };
    let sr = VkSrPass {
        call: (&raw mut recording).cast(),
        commands,
        device: packet.device,
        device_proc: packet.device_proc,
        output_width: camera.output_width,
        output_height: camera.output_height,
        source_frame_id: pass.source_frame_id,
        render_width: camera.render_width,
        render_height: camera.render_height,
    };
    CONTROL.vulkan.sr_render(platform(), &sr)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_render_vulkan(
    commands: Handle,
    packet: *const rsf_godot_vk_packet,
) -> i32 {
    unsafe { render(commands, packet, None, ptr::null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_render_vulkan_handoff(
    commands: Handle,
    packet: *const rsf_godot_vk_packet,
    handoff: *const rsf_godot_vk_handoff,
    continuation: *mut Handle,
) -> i32 {
    if continuation.is_null() {
        return 0;
    }
    unsafe { continuation.write(commands) };
    let Some(handoff) = (unsafe { checked(handoff) }) else {
        return 0;
    };
    if handoff.abi_version != RSF_GODOT_VK_HANDOFF_ABI_VERSION {
        return 0;
    }
    unsafe { render(commands, packet, Some(handoff), continuation) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_sr_destroy(owner: Handle) -> i32 {
    unsafe { linked::rsf_core_srvk_session_close(owner) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_sr_create(
    call: Handle,
    request: *const VkSrCreate,
    out: *mut VkSrContext,
) -> u32 {
    let call = unsafe { &*call.cast::<Recording<'_>>() };
    let request = unsafe { &*request };
    let packet = call.packet;
    let camera = packet.pass.camera;
    let (directory, fsr4) = {
        let state = state();
        (state.directory.as_ptr(), state.fsr4.as_ptr())
    };
    let setup = rsf_srvk_setup {
        struct_size: size_of::<rsf_srvk_setup>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: packet.instance,
        physical_device: packet.physical_device,
        device: packet.device,
        instance_proc: packet.instance_proc,
        device_proc: packet.device_proc,
        backend: request.backend,
        quality: request.quality,
        output_width: camera.output_width,
        output_height: camera.output_height,
        depth_inverted: camera.depth_inverted,
        depth_infinite: 0,
        motion_jittered: camera.motion_jittered,
        runtime_directory_utf8: directory,
    };
    let handoff = call
        .handoff
        .filter(|handoff| !call.continuation.is_null() && handoff.split_record.is_some())
        .map(|handoff| rsf_fsr4vk_handoff {
            struct_size: size_of::<rsf_fsr4vk_handoff>() as u32,
            abi_version: RSF_SRVK_SPLIT_ABI_VERSION,
            timeline_enabled: handoff.timeline_enabled,
            queue_family: handoff.queue_family,
            user: handoff.user,
            split_record: handoff.split_record,
        });
    let mut owner = ptr::null_mut();
    let result = unsafe {
        linked::open(
            &setup,
            handoff.as_ref().map_or(ptr::null(), ptr::from_ref),
            fsr4,
            request.preset,
            &mut owner,
        )
    };
    unsafe {
        out.write(VkSrContext {
            handle: owner,
            result,
            reserved: 0,
        })
    };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_sr_plan(
    call: Handle,
    owner: Handle,
    width: *mut u32,
    height: *mut u32,
) -> u32 {
    let call = unsafe { &mut *call.cast::<Recording<'_>>() };
    let (mut w, mut h) = (0, 0);
    let _ = unsafe { linked::rsf_core_srvk_session_plan(owner, &mut w, &mut h) };
    call.plan = [w, h];
    unsafe {
        width.write(w);
        height.write(h);
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_sr_record(
    call: Handle,
    owner: Handle,
    sharpening: *const Sharpening,
    provider: *mut i32,
    output: *mut i32,
    next: *mut Handle,
) -> u32 {
    let call = unsafe { &mut *call.cast::<Recording<'_>>() };
    let packet = call.packet;
    let fallback = rsf_srvk_fallback {
        struct_size: size_of::<rsf_srvk_fallback>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: packet.instance,
        physical_device: packet.physical_device,
        device: packet.device,
        instance_proc: packet.instance_proc,
        device_proc: packet.device_proc,
        command_buffer: call.commands,
        color: image(packet, &packet.color, SHADER_READ),
        output: image(packet, &packet.output, TRANSFER_DST),
    };
    let frame = if owner.is_null() {
        None
    } else {
        let (log, log_user) = {
            let state = state();
            (state.log, state.log_user)
        };
        Some(rsf_srvk_frame {
            struct_size: size_of::<rsf_srvk_frame>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            record: call.record,
            color: fallback.color,
            depth: image(packet, &packet.depth, SHADER_READ),
            motion: image(packet, &packet.motion, SHADER_READ),
            output: fallback.output,
            motion_scale_x: -(call.record.render_width as f32) * packet.pass.motion_to_uv[0],
            motion_scale_y: -(call.record.render_height as f32) * packet.pass.motion_to_uv[1],
            pre_exposure: 1.0,
            sharpening: unsafe { *sharpening },
            log,
            log_user,
            reactive: Default::default(),
            composition: Default::default(),
            exposure: Default::default(),
            bias_current_color: Default::default(),
        })
    };
    let result = unsafe {
        linked::record(
            owner,
            frame.as_ref().map_or(ptr::null(), ptr::from_ref),
            &fallback,
            call.handoff.is_some() && !call.continuation.is_null(),
            provider,
            next,
        )
    };
    call.commands = unsafe { next.read() };
    unsafe {
        output.write(result);
        if !call.continuation.is_null() {
            call.continuation.write(call.commands);
        }
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_sr_log(
    call: Handle,
    kind: u32,
    provider: i32,
    fallback: i32,
) -> u32 {
    let call = unsafe { &*call.cast::<Recording<'_>>() };
    let message = match kind {
        VK_LOG_REJECTED => {
            format!("Vulkan SR rejected: provider={provider} spatial fallback={fallback}")
        }
        VK_LOG_FSR4_DISPATCH => {
            let continuing = u32::from(!call.commands.is_null());
            format!("Godot Vulkan FSR4 dispatch: result={provider} continuation={continuing}")
        }
        VK_LOG_INPUT_REFUSAL => {
            let record = call.record;
            let packet = call.packet;
            let [width, height] = call.plan;
            format!(
                "Vulkan SR input refusal: frame={} render={}x{} plan={width}x{height} output={}x{} formats={}/{}/{}/{} sizes={}x{}/{}x{}/{}x{}/{}x{} ms={:.3} near={:.3} far={:.3} fov={:.3}",
                record.frame_id,
                record.render_width,
                record.render_height,
                record.output_width,
                record.output_height,
                packet.color.format,
                packet.depth.format,
                packet.motion.format,
                packet.output.format,
                packet.color.width,
                packet.color.height,
                packet.depth.width,
                packet.depth.height,
                packet.motion.width,
                packet.motion.height,
                packet.output.width,
                packet.output.height,
                record.frame_time_ms,
                record.camera.near_plane,
                record.camera.far_plane,
                record.camera.vertical_fov_radians,
            )
        }
        _ => return 1,
    };
    log_text(message);
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_godot_vk_leaf_sr_dump_result(
    call: Handle,
    backend: u32,
    result: i32,
) -> u32 {
    let call = unsafe { &*call.cast::<Recording<'_>>() };
    unsafe { rsf_input_dump_result(call.record, c"sr".as_ptr(), backend, result) };
    1
}
