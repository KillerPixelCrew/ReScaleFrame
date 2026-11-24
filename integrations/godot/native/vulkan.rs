// SPDX-License-Identifier: MIT
//! The Godot Vulkan session: settings and their reconstruction generation, provider admission
//! from the session's capability facts (Auto resolved by the Runtime's adapter policy), the FG
//! presenter the engine's WSI owner creates, the Runtime SR session with its retirement rules, the
//! frame capture and the status record. One lock is the session's guard and serializes every
//! borrowed engine and Runtime object, as the C++ guard did; Runtime capture calls from nested SDK
//! WSI skip it while it is held.

use crate::control::{
    Settings, Status, default_settings, reconstruction_key, sharpening, status_header,
};
use crate::sys::*;
use crate::vulkan_creation::{self as creation, DeviceReceipt, VkFunction, VkProc};
use crate::{checked, window};
use ash::vk;
use rsf_game_sys::backend_result;
use rsf_game_sys::ffi_util::{ascii_wide, lock_or_recover, wide};
use rsf_game_sys::win32::typed_export;
use rsf_game_sys::{
    RSF_FG_BACKEND_AUTO, RSF_FG_BACKEND_DLSS, RSF_FG_BACKEND_FSR3, RSF_FG_BACKEND_FSR4,
    RSF_FG_BACKEND_XESS, RSF_FRAME_FLAG_RESET, RSF_GAME_FRAME_ABI_VERSION,
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, RSF_SCREEN_FLIGHT,
    RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_NONE, RSF_SR_SPATIAL, RSF_SR_XESS,
    rsf_frame_record, rsf_game_log_fn,
};
use std::ffi::{CStr, CString, c_int, c_void};
use std::mem::size_of_val;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};

mod capture;
#[path = "../../../runtime/orchestrator/src/vulkan_sr_session/linked.rs"]
mod linked;

type Handle = *mut c_void;
const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;
const SHADER_READ: u32 = 5;
const TRANSFER_DST: u32 = 7;
const PRESENT_LAYOUT: u32 = 1_000_001_002;
/// The first registered window message; the PCL ping message is one when Streamline bound it.
const REGISTERED_MESSAGE: u32 = 0xc000;
/// Logged refusals per session: rejected passes, FSR4 dispatch failures and input refusals.
const DIAGNOSTIC_LIMIT: u32 = 4;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: Handle, flags: u32) -> Handle;
}

unsafe extern "C" {
    // The Runtime's private choice (runtime/orchestrator/src/fg_auto_choice.h).
    fn rsf_fg_select_auto_backend(preferred: u32, choices: u32) -> u32;
}

/// A Vulkan handle from the pointer-sized word the records carry.
pub(crate) fn handle<H: vk::Handle>(pointer: *mut c_void) -> H {
    H::from_raw(pointer.addr() as u64)
}

fn features() -> &'static rsf_godot_vk_features {
    // The CMake-configured native table is immutable and part of this module.
    unsafe { &*rsf_godot_vk_native_features() }
}

#[derive(Default)]
struct State {
    settings: Settings,
    generation: u64,
    status: Status,
    /// The engine's primary window as `rsfg_vk_window` last reported it, admitted or not: the FG
    /// presenter's host window and the FSR4 and XeSS FG precondition. Null from each start.
    primary_window: Handle,
    frame: u64,
    explicit: bool,
    directory: CString,
    streamline: CString,
    fsr4: CString,
    intel: Vec<u16>,
    log: rsf_game_log_fn,
    log_user: Handle,
    resolver: Option<VkProc>,
    presenter: *mut rsf_fgvk,
    /// The presenter's device borrows; never a runtime directory.
    graphics: rsf_srvk_setup,
    shutdown_pending: bool,
    graph_handoff: bool,
    streamline_ready: bool,
    capture: capture::Capture,
    /// The Runtime SR session and the request it was opened for.
    sr: Handle,
    /// The last open succeeded; a session kept after a failed open is retired next pass.
    sr_ready: bool,
    /// The engine device and its proc the session was opened on, kept for retirement.
    sr_device: Handle,
    sr_device_proc: Option<VkProc>,
    sr_generation: u64,
    sr_backend: u32,
    diagnostics: u32,
}

// Device, presenter and logger borrows have engine lifetimes; the lock serializes their users.
unsafe impl Send for State {}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        settings: default_settings(),
        generation: 1,
        status: status_header(),
        ..Default::default()
    })
});

fn state() -> MutexGuard<'static, State> {
    lock_or_recover(&STATE)
}

pub(crate) fn booted() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

impl State {
    fn log(&self, message: &CStr) {
        if let Some(logger) = self.log {
            unsafe { logger(self.log_user, message.as_ptr()) };
        }
    }

    fn log_text(&self, message: String) {
        if let Ok(message) = CString::new(message) {
            self.log(&message);
        }
    }

    fn presenter_backend(&self) -> u32 {
        let mut backend = 0;
        if !self.presenter.is_null() {
            let _ = unsafe { rsf_fgvk_backend(self.presenter, &mut backend) };
        }
        backend
    }

    /// The SR providers this session can serve on the device of `receipt`: DLSS needs the
    /// Streamline session started with DLSS SR and FG; XeSS the device created with the XeSS
    /// requirements; FSR4 its build, the engine's graph lease capability and the interop device.
    /// FSR3 is always served.
    fn sr_choices(&self, receipt: &DeviceReceipt) -> u32 {
        let fsr4 = features().fsr4_sr != 0 && self.graph_handoff && receipt.interop;
        choice(true, RSF_SR_FSR3)
            | choice(self.streamline_ready, RSF_SR_DLSS)
            | choice(receipt.xess, RSF_SR_XESS)
            | choice(fsr4, RSF_SR_FSR4)
    }

    /// The FG providers offered to Auto: FSR3, and XeSS with its build, the interop device and the
    /// primary window. DLSS-G stays an explicit opt-in (its presenter's retirement after native
    /// secondary window use still ends in an access violation, NATIVE.md) and FSR4 FG needs a
    /// provider probe that Auto does not trigger.
    fn fg_choices(&self, receipt: &DeviceReceipt) -> u32 {
        let xess = features().xess_fg != 0 && receipt.interop && !self.primary_window.is_null();
        choice(true, RSF_FG_BACKEND_FSR3) | choice(xess, RSF_FG_BACKEND_XESS)
    }

    /// `settings`' SR and FG providers with Auto resolved by the Runtime's adapter policy among
    /// the providers this session can serve on the device of `receipt`. Without adapter facts
    /// vendor and software are zero, the Runtime's unknown adapter.
    fn providers(&self, settings: &Settings, receipt: &DeviceReceipt) -> (u32, u32) {
        let adapter = receipt.adapter.unwrap_or_default();
        let sr = match settings.sr_backend {
            RSF_SR_AUTO => unsafe {
                rsf_gpu_select_sr_backend(
                    adapter.vendor,
                    adapter.software,
                    self.sr_choices(receipt),
                )
            },
            backend => backend,
        };
        let fg = match settings.fg_backend {
            RSF_FG_BACKEND_AUTO => unsafe {
                rsf_fg_select_auto_backend(
                    rsf_fg_default_backend(adapter.vendor, adapter.software),
                    self.fg_choices(receipt),
                )
            },
            backend => backend,
        };
        (sr, fg)
    }

    /// Device readiness and provider refusals, in the order they are checked. The FSR4 FG probe
    /// and the presenter capacity are asked only when reached.
    fn admit(&self, value: &Settings) -> bool {
        let receipt = creation::render_receipt();
        // DLSS admission is still verified by its live provider at evaluation.
        if matches!(value.sr_backend, RSF_SR_XESS | RSF_SR_FSR4)
            && self.sr_choices(&receipt) & choice(true, value.sr_backend) == 0
        {
            return false;
        }
        let selected = self.providers(value, &receipt).1;
        if !matches!(
            selected,
            RSF_FG_BACKEND_DLSS | RSF_FG_BACKEND_FSR3 | RSF_FG_BACKEND_FSR4 | RSF_FG_BACKEND_XESS
        ) {
            return false;
        }
        let presenter = !self.presenter.is_null();
        if selected == RSF_FG_BACKEND_FSR4 {
            let Some(probe) = features().fsr4_probe else {
                return false;
            };
            if !presenter
                || !receipt.interop
                || self.primary_window.is_null()
                || value.generated_frames != 1
            {
                return false;
            }
            // Refuse before asking the engine to retire its working WSI owner.
            let setup = rsf_srvk_setup {
                runtime_directory_utf8: self.fsr4.as_ptr(),
                ..self.graphics
            };
            let mut info = rsf_fsr4fg_info {
                struct_size: size_of::<rsf_fsr4fg_info>() as u32,
                abi_version: RSF_FSR4FG_BRIDGE_ABI_VERSION,
                ..Default::default()
            };
            if unsafe { probe(&setup, &mut info) } != backend_result::OK || info.max_generated < 1 {
                return false;
            }
        }
        if selected == RSF_FG_BACKEND_XESS
            && self.fg_choices(&receipt) & choice(true, selected) == 0
        {
            return false;
        }
        if value.fg_mode != 0
            && (!presenter || (selected == RSF_FG_BACKEND_FSR3 && value.generated_frames != 1))
        {
            return false;
        }
        if value.fg_mode != 0 && self.presenter_backend() == selected {
            let mut maximum = 0;
            if unsafe { rsf_fgvk_max_generated(self.presenter, &mut maximum) } != backend_result::OK
                || value.generated_frames > maximum
            {
                return false;
            }
        }
        true
    }

    /// After a presenter drain, wait for the device before asking the Runtime to close the SR
    /// session.
    fn retire(&mut self) -> bool {
        if self.sr.is_null() {
            return true;
        }
        let wait = self.sr_device_proc.and_then(|get| {
            // Win64 Vulkan dispatch uses PFN_vkVoidFunction as its transport.
            unsafe {
                std::mem::transmute::<Option<VkFunction>, Option<vk::PFN_vkDeviceWaitIdle>>(get(
                    self.sr_device,
                    c"vkDeviceWaitIdle".as_ptr(),
                ))
            }
        });
        if wait.is_none_or(|wait| unsafe { wait(handle(self.sr_device)) } != vk::Result::SUCCESS) {
            return false;
        }
        if unsafe { linked::rsf_core_srvk_session_close(self.sr) } != backend_result::OK {
            return false;
        }
        self.sr = ptr::null_mut();
        self.sr_ready = false;
        true
    }

    /// Counts one refusal against the session's budget; whether it may still be logged.
    fn spend(&mut self) -> bool {
        let within = self.diagnostics < DIAGNOSTIC_LIMIT;
        self.diagnostics = self.diagnostics.wrapping_add(1);
        within
    }

    /// The final image of a Present for an armed frame dump. `owner` is the presenter whose
    /// images are presented, or null for the engine's own swapchain.
    fn capture_present(
        &mut self,
        owner: *mut rsf_fgvk,
        info: *const c_void,
        result: i32,
        begin: bool,
    ) {
        if features().capture.snapshot.is_none() {
            return;
        }
        let armed = unsafe { rsf_input_dump_requested() } != 0;
        unsafe { rsf_input_dump_poll() };
        if !armed || info.is_null() || !self.capture.can_present() || self.frame == 0 {
            return;
        }
        let (session, backend, frame) =
            (self.status.session_id, self.settings.sr_backend, self.frame);
        if self
            .capture
            .present(owner, info, result, begin, session, backend, frame)
        {
            self.log(c"Core frame dump: final Vulkan image could not be read; see the bundle's missing stage.");
        }
    }
}

/// Bit `id` of a Runtime choices mask when `servable`.
fn choice(servable: bool, id: u32) -> u32 {
    if servable { 1 << id } else { 0 }
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

/// While booted, the engine's primary window for the presenter and FG admission.
pub(crate) fn report_window(window: Handle) {
    state().primary_window = window;
}

/// This build carries the Vulkan SR payload.
pub(crate) fn supported() -> bool {
    features().sr != 0
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vulkan_supported() -> i32 {
    supported().into()
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_resolver() -> Option<VkProc> {
    state().resolver
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_host_capabilities(flags: u32) -> i32 {
    let mut state = state();
    if booted() || flags & !RSF_GODOT_VK_GRAPH_HANDOFF != 0 {
        return 0;
    }
    state.graph_handoff = flags & RSF_GODOT_VK_GRAPH_HANDOFF != 0;
    1
}

/// # Safety
/// `setup` is null or a readable record of its stated size whose strings are NUL-terminated.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_boot_vulkan(setup: *const rsf_godot_setup) -> i32 {
    // SAFETY: the caller's guarantee.
    let Some(setup) = (unsafe { checked(setup, RSF_GODOT_ABI_VERSION) }) else {
        return 0;
    };
    let result = boot(setup);
    if result != 0 {
        window::bind_cpu_service(true);
    }
    result
}

/// The directories, Streamline and XeSS initialization and the creation resolver of a new
/// session. The generation keeps counting across sessions.
fn boot(setup: &rsf_godot_setup) -> i32 {
    if setup.runtime_directory.is_null() || !supported() {
        return 0;
    }
    let mut state = state();
    if booted() || state.shutdown_pending {
        return 0;
    }
    state.settings = default_settings();
    state.status = status_header();
    state.status.session_id =
        unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }.wrapping_add(1);
    state.status.fg_result = backend_result::NOT_SUPPORTED;
    state.diagnostics = 0;
    state.primary_window = ptr::null_mut();
    state.frame = 0;
    state.explicit = false;
    state.streamline_ready = false;
    let root = unsafe { CStr::from_ptr(setup.runtime_directory) };
    state.directory = runtime_path(root, b"/fidelityfx-vulkan");
    state.fsr4 = runtime_path(root, b"/fidelityfx");
    state.streamline = runtime_path(root, b"/streamline-vulkan");
    state.log = setup.log;
    state.log_user = setup.log_user;
    let latency = rsf_slvk_setup {
        struct_size: size_of::<rsf_slvk_setup>() as u32,
        abi_version: RSF_SLVK_ABI_VERSION,
        runtime_directory_utf8: state.streamline.as_ptr(),
        project_id: setup.project_id,
        features: RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG,
    };
    let latency_result = unsafe { rsf_slvk_initialize(&latency) };
    let xess = runtime_path(root, b"/xess");
    let Ok(intel) = xess.to_str() else { return 0 };
    state.intel = wide(intel);
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
    state.resolver = creation::resolver(
        base,
        xess_result == backend_result::OK,
        setup.log,
        setup.log_user,
    );
    state.streamline_ready = latency_result == backend_result::OK;
    state.log(if state.streamline_ready {
        c"Godot Vulkan host ready with Streamline Vulkan dispatch and PCL startup."
    } else {
        c"Godot Vulkan host ready. Streamline Vulkan initialization failed; platform latency capabilities are queried at device creation."
    });
    ACTIVE.store(true, Ordering::Release);
    1
}

/// The presenter is disabled and drained, then the SR session retires before the capture, XeSS
/// and SDK stop. A refused step keeps the session booted.
pub(crate) fn shutdown() -> c_int {
    let mut state = state();
    if !booted() {
        return 1;
    }
    if !state.presenter.is_null() {
        let _ = unsafe { rsf_fgvk_enable(state.presenter, 0) };
        if unsafe { rsf_fgvk_drain(state.presenter) } != backend_result::OK {
            return 0;
        }
    }
    if !state.retire() {
        return 0;
    }
    let user = state.capture.user();
    if state.capture.release(user) != backend_result::OK
        || unsafe { rsf_xessvk_shutdown() } != backend_result::OK
    {
        return 0;
    }
    creation::stop();
    // Scene teardown precedes the engine's swapchain destruction; a surviving presenter needs the
    // SDK through swapchain retirement.
    state.shutdown_pending = !state.presenter.is_null();
    if !state.shutdown_pending && unsafe { rsf_latencyvk_shutdown() } != backend_result::OK {
        return 0;
    }
    state.streamline_ready = false;
    state.capture.stop();
    ACTIVE.store(false, Ordering::Release);
    state.status.active_sr = 0;
    state.status.sr_result = backend_result::NOT_READY;
    // The device borrow ends with the session.
    state.sr_device = ptr::null_mut();
    state.sr_device_proc = None;
    unsafe { rsf_input_dump_cancel(state.status.session_id) };
    state.directory = CString::default();
    1
}

/// `settings` passed the shared `rsfg_configure` refusal rules.
pub(crate) fn configure(settings: &Settings) -> c_int {
    let mut state = state();
    if !booted() || !state.admit(settings) {
        return 0;
    }
    if unsafe { rsf_latencyvk_options(settings.latency_mode, settings.frame_limit_us) }
        != backend_result::OK
    {
        return 0;
    }
    if reconstruction_key(&state.settings) != reconstruction_key(settings) {
        state.generation = state.generation.wrapping_add(1);
    }
    state.settings = *settings;
    1
}

pub(crate) fn settings() -> (Settings, c_int) {
    (state().settings, c_int::from(booted()))
}

/// The record with the Streamline Vulkan latency overlay, also when the session is not booted.
pub(crate) fn status() -> (Status, c_int) {
    let mut value = state().status;
    let mut latency = rsf_latencyvk_status {
        struct_size: size_of::<rsf_latencyvk_status>() as u32,
        abi_version: RSF_LATENCYVK_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { rsf_latencyvk_get_status(&mut latency) } == backend_result::OK {
        value.latency_available = latency.low_latency_available;
        value.latency_report_available = latency.report_available;
        value.pcl_available = u32::from(latency.pcl_message >= REGISTERED_MESSAGE);
        value.latency_mode = latency.mode;
        value.sleep_calls = latency.sleep_calls;
        value.marker_calls = latency.marker_calls;
        value.latency_owner = latency.owner;
    }
    (value, c_int::from(booted()))
}

pub(crate) fn render_begin(frame: u64, explicit: bool) {
    if !explicit {
        unsafe { rsf_latencyvk_render(frame) };
    }
    let mut state = state();
    state.frame = frame;
    state.explicit = explicit;
}

/// A native Present the engine issues itself. A nested SDK WSI call is already inside the
/// provider-owned Present, which holds the guard.
pub(crate) fn capture_present(info: *const c_void, result: i32, begin: bool) {
    if let Ok(mut state) = STATE.try_lock() {
        state.capture_present(ptr::null_mut(), info, result, begin);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_create(
    setup: *const rsf_fgvk_setup,
    out: *mut Handle,
    chain: *mut u64,
) -> i32 {
    if !booted() || out.is_null() || chain.is_null() {
        return backend_result::INVALID_ARGUMENT;
    }
    let Some(setup) = (unsafe { rsf_game_sys::ffi_util::checked(setup) }) else {
        return backend_result::INVALID_ARGUMENT;
    };
    let mut state = state();
    if !state.presenter.is_null() {
        return backend_result::NOT_SUPPORTED;
    }
    creation::report_render_device(setup.graphics.device);
    let receipt = creation::render_receipt();
    let requested = state.settings;
    let selected = state.providers(&requested, &receipt).1;
    let mut value = *setup;
    let readable = capture::readable_create(setup);
    if let Some(readable) = readable.as_ref() {
        value.create_info = ptr::from_ref(readable).cast();
        state.capture.setup(&value);
    }
    value.graphics.runtime_directory_utf8 = match selected {
        RSF_FG_BACKEND_DLSS => state.streamline.as_ptr(),
        RSF_FG_BACKEND_FSR4 => state.fsr4.as_ptr(),
        _ => state.directory.as_ptr(),
    };
    let host = rsf_fgvk_host_options {
        struct_size: size_of::<rsf_fgvk_host_options>() as u32,
        abi_version: RSF_FGVK_HOST_ABI_VERSION,
        window: state.primary_window,
        timeline_enabled: u32::from(receipt.interop),
        intel_runtime_directory: state.intel.as_ptr(),
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
    state.presenter = owner;
    unsafe { out.write(owner.cast()) };
    let ready = result == backend_result::OK && !owner.is_null();
    let mut maximum = 0;
    if ready {
        let _ = unsafe { rsf_fgvk_max_generated(owner, &mut maximum) };
        state.graphics = rsf_srvk_setup {
            runtime_directory_utf8: ptr::null(),
            ..setup.graphics
        };
        let _ = unsafe { rsf_latencyvk_presentation(owner.cast()) };
        let _ = unsafe { rsf_latencyvk_options(requested.latency_mode, requested.frame_limit_us) };
    }
    let status = &mut state.status;
    status.fg_available = u32::from(ready);
    status.fg_backend = if ready { selected } else { 0 };
    status.max_generated_frames = maximum;
    let queues = if owner.is_null() { 0 } else { 4 };
    state.log_text(format!(
        "Godot Vulkan swapchain owner: create={result} reserved queues={queues}"
    ));
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_capture_setup(setup: *const rsf_fgvk_setup) {
    if !booted() {
        return;
    }
    let Some(setup) = (unsafe { rsf_game_sys::ffi_util::checked(setup) }) else {
        return;
    };
    if setup.graphics.instance_proc.is_none() || setup.graphics.device_proc.is_none() {
        return;
    }
    let mut state = state();
    creation::report_render_device(setup.graphics.device);
    state.capture.setup(setup);
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_capture_release(user: Handle) -> i32 {
    state().capture.release(user)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_images(owner: Handle, count: *mut u32, images: *mut u64) -> i32 {
    unsafe { rsf_fgvk_images(owner.cast(), count, images) }
}

/// A provider change needs the engine to recreate its WSI owner; the Runtime presenter manages
/// guide sizes at retired slot reuse.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_acquire(
    owner: Handle,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let state = state();
    let mut backend = 0;
    if unsafe { rsf_fgvk_backend(owner.cast(), &mut backend) } != backend_result::OK {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    }
    if backend
        != state
            .providers(&state.settings, &creation::render_receipt())
            .1
    {
        return vk::Result::ERROR_OUT_OF_DATE_KHR.as_raw();
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_present(owner: Handle, info: *const c_void) -> i32 {
    let mut state = state();
    if !state.explicit {
        unsafe { rsf_latencyvk_submit_end(state.frame) };
    }
    state.capture_present(owner.cast(), info, 0, true);
    let result = unsafe { rsf_fgvk_present(owner.cast(), info) };
    state.capture_present(owner.cast(), info, result, false);
    let mut presented = rsf_fgvk_status {
        struct_size: size_of::<rsf_fgvk_status>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { rsf_fgvk_get_status(owner.cast(), &mut presented) } == backend_result::OK {
        let frames = state.settings.generated_frames;
        let status = &mut state.status;
        status.fg_active = presented.enabled;
        status.fg_result = presented.result;
        status.generated_callbacks = presented.generated_callbacks;
        status.generated_frames = if presented.enabled != 0 { frames } else { 0 };
        status.total_presented = presented.last_present_count;
        status.tagged_frames = presented.tagged;
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_recording_begin(commands: Handle) {
    let state = state();
    state.capture.retire(commands);
    // Godot reports the recycled recording only after its previous use retired.
    unsafe { linked::rsf_core_srvk_session_retire(state.sr, commands) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_submitted(commands: *const Handle, count: u32) {
    let state = state();
    if !state.presenter.is_null() {
        unsafe { rsf_fgvk_submitted(state.presenter, commands, count) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_destroy(owner: Handle) -> i32 {
    let mut state = state();
    let live = owner == state.presenter.cast();
    if live {
        let user = state.capture.user();
        if state.capture.release(user) != backend_result::OK {
            return backend_result::NOT_READY;
        }
    }
    let result = unsafe { rsf_fgvk_destroy(owner.cast()) };
    if result == backend_result::OK && live {
        state.presenter = ptr::null_mut();
        state.graphics = rsf_srvk_setup::default();
        let status = &mut state.status;
        status.fg_available = 0;
        status.fg_active = 0;
        status.fg_backend = 0;
        status.max_generated_frames = 0;
        status.generated_frames = 0;
        if state.shutdown_pending {
            if unsafe { rsf_latencyvk_shutdown() } == backend_result::OK {
                state.shutdown_pending = false;
            } else {
                state.log(c"Godot Vulkan SDK shutdown refused after owner retirement; retaining the mapped session.");
            }
        }
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_drain(owner: Handle) -> i32 {
    let _state = state();
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
    let Some(handoff) = (unsafe { rsf_game_sys::ffi_util::checked(handoff) }) else {
        return 0;
    };
    if handoff.abi_version != RSF_GODOT_VK_HANDOFF_ABI_VERSION {
        return 0;
    }
    unsafe { render(commands, packet, Some(handoff), continuation) }
}

/// One SR or final-scene pass of `rsfg_render_vulkan`: the FG recording, then the SR session's
/// evaluation or the Runtime's spatial fallback.
unsafe fn render(
    commands: Handle,
    packet: *const rsf_godot_vk_packet,
    handoff: Option<&rsf_godot_vk_handoff>,
    continuation: *mut Handle,
) -> i32 {
    if !continuation.is_null() {
        unsafe { continuation.write(commands) };
    }
    if !booted() || commands.is_null() {
        return 0;
    }
    let Some(packet) = (unsafe { rsf_game_sys::ffi_util::checked(packet) }) else {
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
    let mut state = state();
    if rsf_game_sys::depth_domains_requested(&pass.depth_domains) {
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
        ..Default::default()
    };
    if features().capture.snapshot.is_some() {
        let backend = state.settings.sr_backend;
        state.capture.render(commands, packet, record, backend);
    }

    // The FG outcome of this pass: no presenter on this device is not ready.
    let settings = state.settings;
    let enabled = u32::from(
        settings.fg_mode != 0 && settings.gameplay != 0 && pass.screen == RSF_SCREEN_FLIGHT,
    );
    let owner = state.presenter;
    let options = if !owner.is_null() && state.graphics.device == packet.device {
        unsafe { rsf_fgvk_enable_options(owner, enabled, settings.generated_frames) }
    } else {
        backend_result::NOT_READY
    };
    let fg_result = if options != backend_result::OK {
        options
    } else {
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
        unsafe {
            if pass.role == RSF_GAME_RENDER_SR {
                rsf_fgvk_prepare(owner, commands, &fg)
            } else {
                rsf_fgvk_hudless(owner, commands, &fg)
            }
        }
    };
    state.status.generated_frames = if enabled != 0 && options == backend_result::OK {
        settings.generated_frames
    } else {
        0
    };
    state.status.fg_result = fg_result;
    if pass.role == RSF_GAME_RENDER_FINAL_SCENE {
        return i32::from(fg_result == backend_result::OK);
    }
    let mut sr = Sr {
        state,
        packet,
        handoff,
        continuation,
        commands,
        record: &record,
        plan: [0; 2],
    };
    sr.pass()
}

/// The SR section of one pass, under the guard. Every outcome is reported to the status record
/// once.
struct Sr<'a> {
    state: MutexGuard<'static, State>,
    packet: &'a rsf_godot_vk_packet,
    handoff: Option<&'a rsf_godot_vk_handoff>,
    continuation: *mut Handle,
    /// The command buffer recording continues on.
    commands: Handle,
    record: &'a rsf_frame_record,
    plan: [u32; 2],
}

impl Sr<'_> {
    fn pass(&mut self) -> c_int {
        let settings = self.state.settings;
        let receipt = creation::device_receipt(self.packet.device.addr());
        let backend = self.state.providers(&settings, &receipt).0;
        let camera = self.packet.pass.camera;
        let output = (camera.output_width, camera.output_height);
        // A failed open, a new SR request, another resolved provider, another device or an output
        // other than the last reported pass's retires the session.
        let state = &mut *self.state;
        let stale = !state.sr_ready
            || state.generation != state.sr_generation
            || backend != state.sr_backend
            || self.packet.device != state.sr_device
            || (state.status.output_width, state.status.output_height) != output;
        if !state.sr.is_null()
            && stale
            && ((!state.presenter.is_null()
                && unsafe { rsf_fgvk_drain(state.presenter) } != backend_result::OK)
                || !state.retire())
        {
            return self.record(None, backend_result::NOT_READY, false);
        }
        let status = &mut state.status;
        status.requested_sr = settings.sr_backend;
        (status.output_width, status.output_height) = output;
        status.render_width = camera.render_width;
        status.render_height = camera.render_height;
        status.source_frame_id = self.packet.pass.source_frame_id;
        // Auto with no servable provider: the engine rendered at Auto's reduced extent.
        if backend == RSF_SR_NONE && settings.sr_backend == RSF_SR_AUTO {
            return self.record(None, backend_result::NOT_SUPPORTED, false);
        }
        // Godot evaluates FSR1 itself; no session passes the engine's output through.
        if backend == RSF_SR_NONE || backend == RSF_SR_SPATIAL {
            self.report(backend_result::OK, Some(backend));
            return 1;
        }
        if self.state.sr.is_null() {
            let result = self.open(backend, &settings);
            if self.state.sr.is_null() || result != backend_result::OK {
                return self.record(None, result, false);
            }
        }
        let state = &mut *self.state;
        let (mut width, mut height) = (0, 0);
        let _ = unsafe { linked::rsf_core_srvk_session_plan(state.sr, &mut width, &mut height) };
        self.plan = [width, height];
        state.status.render_width = width;
        state.status.render_height = height;
        let sharpening = sharpening(&settings);
        self.record(Some(sharpening), backend_result::OK, true)
    }

    /// Opens the Runtime SR session for `backend` on the pass's device.
    fn open(&mut self, backend: u32, settings: &Settings) -> i32 {
        let packet = self.packet;
        let camera = packet.pass.camera;
        let state = &mut *self.state;
        state.sr_ready = false;
        state.sr_device = packet.device;
        state.sr_device_proc = packet.device_proc;
        let setup = rsf_srvk_setup {
            struct_size: size_of::<rsf_srvk_setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: packet.instance,
            physical_device: packet.physical_device,
            device: packet.device,
            instance_proc: packet.instance_proc,
            device_proc: packet.device_proc,
            backend,
            quality: settings.quality,
            output_width: camera.output_width,
            output_height: camera.output_height,
            depth_inverted: camera.depth_inverted,
            // The pinned Godot producer supplies finite z_far.
            depth_infinite: 0,
            motion_jittered: camera.motion_jittered,
            runtime_directory_utf8: state.directory.as_ptr(),
        };
        let handoff = self
            .handoff
            .filter(|handoff| !self.continuation.is_null() && handoff.split_record.is_some())
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
                state.fsr4.as_ptr(),
                settings.dlss_preset,
                &mut owner,
            )
        };
        state.sr = owner;
        if !owner.is_null() && result == backend_result::OK {
            state.sr_generation = state.generation;
            state.sr_backend = backend;
            state.sr_ready = true;
        }
        result
    }

    /// Records the session's evaluation when `sharpening` is given, else only the Runtime's
    /// spatial fallback, and reports the outcome. Diagnostics are best effort and never change it.
    fn record(
        &mut self,
        sharpening: Option<rsf_sharpening_options>,
        result: i32,
        evaluate: bool,
    ) -> c_int {
        let packet = self.packet;
        let fallback = rsf_srvk_fallback {
            struct_size: size_of::<rsf_srvk_fallback>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: packet.instance,
            physical_device: packet.physical_device,
            device: packet.device,
            instance_proc: packet.instance_proc,
            device_proc: packet.device_proc,
            command_buffer: self.commands,
            color: image(packet, &packet.color, SHADER_READ),
            output: image(packet, &packet.output, TRANSFER_DST),
        };
        let owner = if evaluate {
            self.state.sr
        } else {
            ptr::null_mut()
        };
        let frame = sharpening
            .filter(|_| evaluate)
            .map(|sharpening| rsf_srvk_frame {
                struct_size: size_of::<rsf_srvk_frame>() as u32,
                abi_version: RSF_SRVK_ABI_VERSION,
                record: self.record,
                color: fallback.color,
                depth: image(packet, &packet.depth, SHADER_READ),
                motion: image(packet, &packet.motion, SHADER_READ),
                output: fallback.output,
                motion_scale_x: -(self.record.render_width as f32) * packet.pass.motion_to_uv[0],
                motion_scale_y: -(self.record.render_height as f32) * packet.pass.motion_to_uv[1],
                pre_exposure: 1.0,
                sharpening,
                log: self.state.log,
                log_user: self.state.log_user,
                reactive: Default::default(),
                composition: Default::default(),
                exposure: Default::default(),
                bias_current_color: Default::default(),
            });
        let mut next = ptr::null_mut();
        let mut provider = 0;
        let output = unsafe {
            linked::record(
                owner,
                frame.as_ref().map_or(ptr::null(), ptr::from_ref),
                &fallback,
                self.handoff.is_some() && !self.continuation.is_null(),
                &mut provider,
                &mut next,
            )
        };
        self.commands = next;
        if !self.continuation.is_null() {
            unsafe { self.continuation.write(next) };
        }
        let result = if owner.is_null() { result } else { provider };
        let backend = self.state.sr_backend;
        if evaluate {
            if backend == RSF_SR_FSR4 && result != backend_result::OK && self.state.spend() {
                let continuing = u32::from(!next.is_null());
                self.state.log_text(format!(
                    "Godot Vulkan FSR4 dispatch: result={result} continuation={continuing}"
                ));
            }
            unsafe { rsf_input_dump_result(self.record, c"sr".as_ptr(), backend, result) };
            if result == backend_result::INVALID_ARGUMENT && self.state.spend() {
                self.log_input_refusal();
            }
        }
        if evaluate && result == backend_result::OK && output == backend_result::OK {
            self.report(result, Some(backend));
        } else {
            let refused = !next.is_null();
            if refused && self.state.spend() {
                self.state.log_text(format!(
                    "Vulkan SR rejected: provider={result} spatial fallback={output}"
                ));
            }
            self.state.status.sr_result = result;
            self.state.status.active_sr = 0;
            if refused {
                self.state.status.refused_frames = self.state.status.refused_frames.wrapping_add(1);
            }
        }
        c_int::from(output == backend_result::OK)
    }

    /// An evaluated pass of `backend`.
    fn report(&mut self, result: i32, backend: Option<u32>) {
        let status = &mut self.state.status;
        status.sr_result = result;
        status.active_sr = backend.unwrap_or(0);
        status.evaluated_frames = status.evaluated_frames.wrapping_add(1);
    }

    fn log_input_refusal(&self) {
        let record = self.record;
        let packet = self.packet;
        let [width, height] = self.plan;
        self.state.log_text(format!(
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
        ));
    }
}
