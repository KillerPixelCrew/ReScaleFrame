// SPDX-License-Identifier: MIT
//! The native host library: its load, the `rsfg_*` export table and every engine forwarder.
//!
//! bridge.cpp converts Godot's project path to UTF-16 and reports a load failure through
//! Godot's error printer. Rust loads the library, resolves each export once, and decides which
//! exports are required, which are optional, and which engine state gates each forwarder.
//! The Vulkan driver fact remains a Godot query, read through the leaf registered at load.

use core::ffi::{CStr, c_void};
use core::mem::{size_of, transmute};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::cpu_facts::build_cpu_event;
use crate::frame_event_abi::rsf_game_cpu_event;
use crate::host;
use crate::packet::rsf_godot_execution_pass;
use crate::packet_abi::rsf_game_render_pass;

unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
    fn QueryPerformanceFrequency(value: *mut i64) -> i32;
}

// game_renderer.h: RSF_GAME_RENDER_FRAME. native.h checks the value.
const RENDER_FRAME: u32 = 13;
// VK_IMAGE_LAYOUT_PRESENT_SRC_KHR, the layout of an engine-owned WSI image.
const PRESENT_LAYOUT: u32 = 1_000_001_002;
// VK_ERROR_INITIALIZATION_FAILED for a request that reaches an unloaded host.
const UNLOADED: i32 = -3;

const BOOT: usize = 0;
const BOOT_VULKAN: usize = 1;
const RENDER_VULKAN: usize = 2;
const RENDER_VULKAN_HANDOFF: usize = 3;
const VK_HOST_CAPABILITIES: usize = 4;
const VK_RESOLVER: usize = 5;
const VK_WINDOW: usize = 6;
const VK_PRESENT_MARKER: usize = 7;
const VK_NATIVE_PRESENT: usize = 8;
const VK_WSI_CALL: usize = 9;
const VK_CREATE: usize = 10;
const VK_IMAGES: usize = 11;
const VK_ACQUIRE: usize = 12;
const VK_PRESENT: usize = 13;
const VK_SUBMITTED: usize = 14;
const VK_RECORDING_BEGIN: usize = 15;
const VK_DESTROY: usize = 16;
const VK_DRAIN: usize = 17;
const VK_IMAGE_LAYOUT: usize = 18;
const VK_FOREIGN_CALL: usize = 19;
const CREATE_SWAPCHAIN: usize = 20;
const CONFIGURE: usize = 21;
const STATUS: usize = 22;
const RENDER: usize = 23;
const CPU: usize = 24;
const RENDER_BEGIN: usize = 25;
const SUBMITTED: usize = 26;
const PRESENT: usize = 27;
const SHUTDOWN: usize = 28;
/// Exports below this index are required. A host without one refuses boot.
const REQUIRED: usize = 29;
const VK_EXECUTION: usize = 29;
const VK_CAPTURE_SETUP: usize = 30;
const VK_CAPTURE_RELEASE: usize = 31;
const ATTACH_ENGINE: usize = 32;
const COUNT: usize = 33;

const NAMES: [&CStr; COUNT] = [
    c"rsfg_boot",
    c"rsfg_boot_vulkan",
    c"rsfg_render_vulkan",
    c"rsfg_render_vulkan_handoff",
    c"rsfg_vk_host_capabilities",
    c"rsfg_vk_resolver",
    c"rsfg_vk_window",
    c"rsfg_vk_present_marker",
    c"rsfg_vk_native_present",
    c"rsfg_vk_wsi_call",
    c"rsfg_vk_create",
    c"rsfg_vk_images",
    c"rsfg_vk_acquire",
    c"rsfg_vk_present",
    c"rsfg_vk_submitted",
    c"rsfg_vk_recording_begin",
    c"rsfg_vk_destroy",
    c"rsfg_vk_drain",
    c"rsfg_vk_image_layout",
    c"rsfg_vk_foreign_call",
    c"rsfg_create_swapchain",
    c"rsfg_configure",
    c"rsfg_status",
    c"rsfg_render",
    c"rsfg_cpu",
    c"rsfg_render_begin",
    c"rsfg_submitted",
    c"rsfg_present",
    c"rsfg_shutdown",
    c"rsfg_vk_execution",
    c"rsfg_vk_capture_setup",
    c"rsfg_vk_capture_release",
    c"rsfg_attach_engine",
];

/// Published once by the main thread at boot, before the host reports ready.
static TABLE: [AtomicPtr<c_void>; COUNT] = [const { AtomicPtr::new(null_mut()) }; COUNT];
/// bridge.cpp's Godot query: the OS rendering driver is vulkan.
static VULKAN_DRIVER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

type Callback = unsafe extern "C" fn(*mut c_void) -> i32;
type Present = unsafe extern "C" fn(*mut c_void, *const c_void) -> i32;

/// The resolved export with the C signature `F`, or `None` before a successful load.
///
/// # Safety
/// `F` must be the function pointer type of the export at `index` in api.h.
unsafe fn export<F: Copy>(index: usize) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    let address = TABLE[index].load(Ordering::Acquire);
    // SAFETY: a nonnull entry is the export named at `index`, whose type the caller supplies.
    (!address.is_null()).then(|| unsafe { core::mem::transmute_copy::<*mut c_void, F>(&address) })
}

macro_rules! native {
    ($index:expr, fn($($arg:ty),*) $(-> $ret:ty)?) => {
        // SAFETY: each call site names the api.h signature of its export.
        unsafe { export::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>($index) }
    };
}

fn vulkan_enabled() -> bool {
    if host::rsf_godot_host_ready() == 0 {
        return false;
    }
    let leaf = VULKAN_DRIVER.load(Ordering::Acquire);
    // SAFETY: a nonnull leaf is the `unsafe extern "C" fn() -> u32` registered by the load.
    !leaf.is_null()
        && unsafe { transmute::<*mut c_void, unsafe extern "C" fn() -> u32>(leaf)() } != 0
}

fn execution_supported() -> bool {
    vulkan_enabled() && !TABLE[VK_EXECUTION].load(Ordering::Acquire).is_null()
}

/// Main thread, once during boot: loads the native host and resolves its exports. Returns 1
/// when every required export resolved. A failed library load stores its Windows error in
/// `error`; a missing export leaves the library loaded, stores zero and publishes nothing.
///
/// # Safety
/// `path` must be a NUL-terminated UTF-16 path, `vulkan_driver` a callable leaf for the
/// module's lifetime, and `error` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_load(
    path: *const u16,
    vulkan_driver: unsafe extern "C" fn() -> u32,
    error: *mut u32,
) -> u32 {
    // SAFETY: valid path and error storage, see above.
    unsafe {
        let module = LoadLibraryW(path);
        if module.is_null() {
            error.write(GetLastError());
            return 0;
        }
        error.write(0);
        let mut resolved = [null_mut(); COUNT];
        for (index, name) in NAMES.iter().enumerate() {
            resolved[index] = GetProcAddress(module, name.as_ptr().cast());
            if index < REQUIRED && resolved[index].is_null() {
                return 0;
            }
        }
        VULKAN_DRIVER.store(vulkan_driver as *mut c_void, Ordering::Release);
        for (slot, address) in TABLE.iter().zip(resolved) {
            slot.store(address, Ordering::Release);
        }
    }
    1
}

/// `rsfg_boot_vulkan` or `rsfg_boot`. Returns 1 when the host started.
///
/// # Safety
/// `setup` must point to a valid `rsf_godot_setup` for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_boot(vulkan: u32, setup: *const c_void) -> u32 {
    let boot = native!(
        if vulkan != 0 { BOOT_VULKAN } else { BOOT },
        fn(*const c_void) -> i32
    );
    // SAFETY: valid setup, see above.
    boot.is_some_and(|boot| unsafe { boot(setup) } != 0) as u32
}

/// `rsfg_vk_host_capabilities`.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_capabilities(flags: u32) {
    if let Some(capabilities) = native!(VK_HOST_CAPABILITIES, fn(u32) -> i32) {
        // SAFETY: a plain value argument.
        unsafe { capabilities(flags) };
    }
}

/// The optional `rsfg_attach_engine`. Returns 1 when present and accepted.
///
/// # Safety
/// `services` must point to valid engine services for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_attach(services: *const c_void) -> u32 {
    let attach = native!(ATTACH_ENGINE, fn(*const c_void) -> i32);
    // SAFETY: valid services, see above.
    attach.is_some_and(|attach| unsafe { attach(services) } != 0) as u32
}

/// `rsfg_status`. Returns 1 when the host filled `status`.
///
/// # Safety
/// `status` must point to an `rsf_godot_status` with its header initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_status(status: *mut c_void) -> u32 {
    let read = native!(STATUS, fn(*mut c_void) -> i32);
    // SAFETY: valid status, see above.
    read.is_some_and(|read| unsafe { read(status) } != 0) as u32
}

/// `rsfg_shutdown`. Returns 1 when the host stopped.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_shutdown() -> u32 {
    let shutdown = native!(SHUTDOWN, fn() -> i32);
    // SAFETY: no arguments.
    shutdown.is_some_and(|shutdown| unsafe { shutdown() } != 0) as u32
}

/// `rsfg_configure`. Returns 1 when the host accepted the settings.
///
/// # Safety
/// `settings` must point to valid `rsf_godot_settings` for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_configure(settings: *const c_void) -> u32 {
    let configure = native!(CONFIGURE, fn(*const c_void) -> i32);
    // SAFETY: valid settings, see above.
    configure.is_some_and(|configure| unsafe { configure(settings) } != 0) as u32
}

/// `rsfg_vk_window` with the engine's main window.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_vk_window(window: *mut c_void) {
    if let Some(report) = native!(VK_WINDOW, fn(*mut c_void)) {
        // SAFETY: the host only records the window identity.
        unsafe { report(window) };
    }
}

/// Main loop thread, frame.rs's report leaf: timestamps one CPU event and passes it to the
/// host while the module is active.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_report_cpu(
    stage: u32,
    source_frame_id: u64,
    render_expected: u32,
    input_kind: u32,
    unframed_input: u32,
) {
    if host::rsf_godot_host_active() == 0 {
        return;
    }
    let Some(report) = native!(CPU, fn(*const rsf_game_cpu_event)) else {
        return;
    };
    let (mut now, mut frequency) = (0_i64, 0_i64);
    // SAFETY: local storage.
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut frequency);
    }
    let event = build_cpu_event(
        stage,
        host::rsf_godot_host_session(),
        source_frame_id,
        now as u64,
        frequency as u64,
        render_expected,
        input_kind,
        unframed_input,
    );
    // SAFETY: the host reads the event synchronously.
    unsafe { report(&event) };
}

/// `rsfg_vk_execution` for one engine boundary, when Vulkan reports execution receipts.
/// Returns 1 when the host accepted the boundary.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_vk_execution(
    role: u32,
    session: u64,
    source: u64,
    window: u64,
    begin: u32,
) -> u32 {
    if !vulkan_enabled() {
        return 0;
    }
    let Some(execution) = native!(VK_EXECUTION, fn(*const rsf_game_render_pass, u32) -> i32) else {
        return 0;
    };
    let mut pass = rsf_game_render_pass::default();
    // SAFETY: local storage; the host reads the pass synchronously.
    unsafe {
        rsf_godot_execution_pass(role, session, source, window, &mut pass);
        (execution(&pass, begin) != 0) as u32
    }
}

/// Any thread: the native host is ready on the Vulkan driver.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_vulkan_enabled() -> u32 {
    vulkan_enabled() as u32
}

/// Any thread: the Vulkan host also reports execution receipts.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_vk_execution_supported() -> u32 {
    execution_supported() as u32
}

/// Rendering server thread, host.rs's render-begin leaf: announces the source as a Vulkan
/// frame receipt when supported, otherwise through `rsfg_render_begin`.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_render_begin(source_frame_id: u64) {
    if execution_supported() {
        rsf_godot_native_vk_execution(
            RENDER_FRAME,
            host::rsf_godot_host_session(),
            source_frame_id,
            0,
            1,
        );
    } else if let Some(begin) = native!(RENDER_BEGIN, fn(u64)) {
        // SAFETY: a plain value argument.
        unsafe { begin(source_frame_id) };
    }
}

/// D3D12 swapchain creation through the host. Returns 1 when the host created `out`.
///
/// # Safety
/// The DXGI arguments must be valid for `IDXGIFactory2::CreateSwapChainForHwnd`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_create_swapchain(
    factory: *mut c_void,
    queue: *mut c_void,
    window: *mut c_void,
    description: *const c_void,
    out: *mut *mut c_void,
) -> u32 {
    if host::rsf_godot_host_ready() == 0 {
        return 0;
    }
    let create = native!(
        CREATE_SWAPCHAIN,
        fn(*mut c_void, *mut c_void, *mut c_void, *const c_void, *mut *mut c_void) -> i32
    );
    // SAFETY: valid DXGI arguments, see above. A nonnegative HRESULT is success.
    create.is_some_and(|create| unsafe { create(factory, queue, window, description, out) } >= 0)
        as u32
}

/// The host's `vkGetInstanceProcAddr`, or zero when Volk keeps the loader's resolver.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_vk_resolver() -> u64 {
    if !vulkan_enabled() {
        return 0;
    }
    let resolver = native!(VK_RESOLVER, fn() -> *mut c_void);
    // SAFETY: no arguments.
    resolver.map_or(0, |resolver| unsafe { resolver() } as u64)
}

/// The legacy Vulkan Present marker, used when execution receipts are unavailable.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_native_vk_present_marker(window: u64, begin: u32) -> u32 {
    if !vulkan_enabled() {
        return 0;
    }
    let marker = native!(VK_PRESENT_MARKER, fn(u64, u32) -> i32);
    // SAFETY: plain value arguments.
    marker.is_some_and(|marker| unsafe { marker(window, begin) } != 0) as u32
}

/// Presents through the host while it owns Vulkan WSI, otherwise calls the engine's
/// `vkQueuePresentKHR` at `callback` directly.
///
/// # Safety
/// `callback` must be the address of `vkQueuePresentKHR`, with a valid queue and present info.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_native_present(
    callback: u64,
    queue: *mut c_void,
    info: *const c_void,
) -> i32 {
    if vulkan_enabled()
        && let Some(present) = native!(
            VK_NATIVE_PRESENT,
            fn(u64, *mut c_void, *const c_void) -> i32
        )
    {
        // SAFETY: valid present arguments, see above.
        return unsafe { present(callback, queue, info) };
    }
    // SAFETY: `callback` is vkQueuePresentKHR, see above.
    unsafe { transmute::<*const (), Present>(callback as usize as *const ())(queue, info) }
}

/// Runs a WSI lease inside the host's guard while it owns Vulkan WSI, otherwise directly.
///
/// # Safety
/// `callback` must be callable with `user` for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_wsi_call(
    callback: Callback,
    user: *mut c_void,
) -> i32 {
    unsafe { lease(VK_WSI_CALL, callback, user) }
}

/// Runs a foreign Vulkan call inside the host's guard while it owns Vulkan, otherwise directly.
///
/// # Safety
/// `callback` must be callable with `user` for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_foreign_call(
    callback: Callback,
    user: *mut c_void,
) -> i32 {
    unsafe { lease(VK_FOREIGN_CALL, callback, user) }
}

unsafe fn lease(index: usize, callback: Callback, user: *mut c_void) -> i32 {
    if vulkan_enabled()
        && let Some(guarded) = native!(index, fn(Callback, *mut c_void) -> i32)
    {
        // SAFETY: callable lease, see the exported callers.
        return unsafe { guarded(callback, user) };
    }
    // SAFETY: callable lease, see the exported callers.
    unsafe { callback(user) }
}

/// `rsfg_vk_create` for the primary swapchain. Returns 1 when the host created a proxy.
///
/// # Safety
/// `setup` must be a valid `rsf_fgvk_setup`, and `owner` and `chain` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_create(
    setup: *const c_void,
    owner: *mut *mut c_void,
    chain: *mut u64,
) -> u32 {
    if !vulkan_enabled() {
        return 0;
    }
    let create = native!(
        VK_CREATE,
        fn(*const c_void, *mut *mut c_void, *mut u64) -> i32
    );
    // SAFETY: valid arguments, see above.
    create.is_some_and(|create| unsafe { create(setup, owner, chain) } == 0) as u32
}

/// The optional `rsfg_vk_capture_setup`, independent of the FG proxy.
///
/// # Safety
/// `setup` must be a valid `rsf_fgvk_setup` for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_capture_setup(setup: *const c_void) {
    if vulkan_enabled()
        && let Some(capture) = native!(VK_CAPTURE_SETUP, fn(*const c_void))
    {
        // SAFETY: valid setup, see above.
        unsafe { capture(setup) };
    }
}

/// The optional `rsfg_vk_capture_release`. Returns 1 when no capture remains for the chain.
///
/// # Safety
/// `queue_user` must be the swapchain registered with the capture.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_capture_release(queue_user: *mut c_void) -> u32 {
    if !vulkan_enabled() {
        return 1;
    }
    let release = native!(VK_CAPTURE_RELEASE, fn(*mut c_void) -> i32);
    // SAFETY: valid chain, see above.
    release.is_none_or(|release| unsafe { release(queue_user) } == 0) as u32
}

/// `rsfg_vk_images` for a host proxy.
///
/// # Safety
/// `owner` must be a live proxy, with `count` and `images` as for `vkGetSwapchainImagesKHR`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_images(
    owner: *mut c_void,
    count: *mut u32,
    images: *mut u64,
) -> i32 {
    let read = native!(VK_IMAGES, fn(*mut c_void, *mut u32, *mut u64) -> i32);
    // SAFETY: valid arguments, see above.
    read.map_or(UNLOADED, |read| unsafe { read(owner, count, images) })
}

/// The final layout of the swapchain's game-facing images. An engine-owned chain, including
/// one created before the host loaded, presents from WSI.
///
/// # Safety
/// A nonnull `owner` must be a live proxy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_image_layout(owner: *mut c_void) -> u32 {
    let layout = native!(VK_IMAGE_LAYOUT, fn(*mut c_void) -> u32);
    // SAFETY: valid owner, see above.
    layout.map_or(PRESENT_LAYOUT, |layout| unsafe { layout(owner) })
}

/// `rsfg_vk_acquire` for a host proxy.
///
/// # Safety
/// `owner` must be a live proxy and `index` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_acquire(
    owner: *mut c_void,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let acquire = native!(VK_ACQUIRE, fn(*mut c_void, u64, u64, u64, *mut u32) -> i32);
    // SAFETY: valid arguments, see above.
    acquire.map_or(UNLOADED, |acquire| unsafe {
        acquire(owner, timeout, semaphore, fence, index)
    })
}

/// `rsfg_vk_present` for a host proxy.
///
/// # Safety
/// `owner` must be a live proxy and `info` a valid `VkPresentInfoKHR`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_present(
    owner: *mut c_void,
    info: *const c_void,
) -> i32 {
    let present = native!(VK_PRESENT, fn(*mut c_void, *const c_void) -> i32);
    // SAFETY: valid arguments, see above.
    present.map_or(UNLOADED, |present| unsafe { present(owner, info) })
}

/// `rsfg_vk_recording_begin` once the native host is ready.
///
/// # Safety
/// `commands` must be the engine's retired `VkCommandBuffer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_recording_begin(commands: *mut c_void) {
    if host::rsf_godot_host_ready() != 0
        && let Some(begin) = native!(VK_RECORDING_BEGIN, fn(*mut c_void))
    {
        // SAFETY: valid command buffer, see above.
        unsafe { begin(commands) };
    }
}

/// `rsfg_vk_submitted` once the native host is ready.
///
/// # Safety
/// `commands` must point to `count` submitted `VkCommandBuffer` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_submitted(commands: *const *mut c_void, count: u32) {
    if host::rsf_godot_host_ready() != 0
        && let Some(submitted) = native!(VK_SUBMITTED, fn(*const *mut c_void, u32))
    {
        // SAFETY: valid command list, see above.
        unsafe { submitted(commands, count) };
    }
}

/// `rsfg_vk_destroy`. Returns 1 when the proxy retired.
///
/// # Safety
/// `owner` must be a live proxy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_destroy(owner: *mut c_void) -> u32 {
    let destroy = native!(VK_DESTROY, fn(*mut c_void) -> i32);
    // SAFETY: valid owner, see above.
    destroy.is_some_and(|destroy| unsafe { destroy(owner) } == 0) as u32
}

/// `rsfg_vk_drain`. Returns 1 when the proxy has no outstanding work.
///
/// # Safety
/// `owner` must be a live proxy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_vk_drain(owner: *mut c_void) -> u32 {
    let drain = native!(VK_DRAIN, fn(*mut c_void) -> i32);
    // SAFETY: valid owner, see above.
    drain.is_some_and(|drain| unsafe { drain(owner) } == 0) as u32
}

/// D3D12 `rsfg_submitted` while the module is active.
///
/// # Safety
/// `lists` must point to `count` command lists executed on `queue`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_submitted(
    queue: *mut c_void,
    lists: *const *mut c_void,
    count: u32,
) {
    if host::rsf_godot_host_active() != 0
        && let Some(submitted) = native!(SUBMITTED, fn(*mut c_void, *const *mut c_void, u32))
    {
        // SAFETY: valid lists, see above.
        unsafe { submitted(queue, lists, count) };
    }
}

/// D3D12 `rsfg_present` while the module is active.
///
/// # Safety
/// `swapchain` must be the engine's live `IDXGISwapChain`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_present(swapchain: *mut c_void) {
    if host::rsf_godot_host_active() != 0
        && let Some(present) = native!(PRESENT, fn(*mut c_void))
    {
        // SAFETY: valid swapchain, see above.
        unsafe { present(swapchain) };
    }
}

/// D3D12 `rsfg_render` for packet.rs's driver routing.
///
/// # Safety
/// The D3D12 objects and the pass must be valid for the recording.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_render(
    device: *mut c_void,
    queue: *mut c_void,
    commands: *mut c_void,
    pass: *const rsf_game_render_pass,
) -> i32 {
    let render = native!(
        RENDER,
        fn(*mut c_void, *mut c_void, *mut c_void, *const rsf_game_render_pass) -> i32
    );
    // SAFETY: valid recording arguments, see above.
    render.map_or(0, |render| unsafe { render(device, queue, commands, pass) })
}

/// Vulkan `rsfg_render_vulkan` for packet.rs's driver routing.
///
/// # Safety
/// `commands` must be recording and `packet` valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_render_vulkan(
    commands: *mut c_void,
    packet: *const c_void,
) -> i32 {
    let render = native!(RENDER_VULKAN, fn(*mut c_void, *const c_void) -> i32);
    // SAFETY: valid recording arguments, see above.
    render.map_or(0, |render| unsafe { render(commands, packet) })
}

/// Vulkan `rsfg_render_vulkan_handoff` for packet.rs's driver routing.
///
/// # Safety
/// `commands` must be recording, with `packet`, `handoff` and `continuation` valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_native_render_vulkan_handoff(
    commands: *mut c_void,
    packet: *const c_void,
    handoff: *const c_void,
    continuation: *mut *mut c_void,
) -> i32 {
    let render = native!(
        RENDER_VULKAN_HANDOFF,
        fn(*mut c_void, *const c_void, *const c_void, *mut *mut c_void) -> i32
    );
    // SAFETY: valid recording arguments, see above.
    render.map_or(0, |render| unsafe {
        render(commands, packet, handoff, continuation)
    })
}
