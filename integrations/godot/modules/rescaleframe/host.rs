// SPDX-License-Identifier: MIT
//! Engine module host lifecycle: boot admission and order, the ready, enabled, session and
//! requested SR state, the `configure` sequence, shutdown order, and the status-driven
//! viewport extent updates.
//!
//! The C++ side keeps every Godot read and call, the native DLL load and export resolution,
//! the Dictionary option parser and the `rsfg_*` calls as typed leaves. Rust decides when each
//! leaf runs. `ready` and `session` were plain globals written once by the main thread at boot,
//! so they are relaxed atomics; `enabled` and the requested SR keep the sequentially consistent
//! loads and stores of the `std::atomic` values they replace.

use core::ffi::c_void;
use core::mem::{align_of, offset_of, size_of};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::{frame, primary};

// api.h: RSF_GODOT_VK_GRAPH_HANDOFF. host.h checks the value.
const VK_GRAPH_HANDOFF: u32 = 1;
// The requested SR before the first accepted `configure`, as the module's global started.
const INITIAL_SR: u32 = 7;

/// Engine facts read once by `RSFGodot::boot`. Every read is a pure query.
#[repr(C)]
pub struct BootFacts {
    enabled_setting: u32,
    headless: u32,
    editor: u32,
    vulkan: u32,
}

/// Module loader and driver leaves for boot.
#[repr(C)]
pub struct BootCallbacks {
    /// Reads the library, runtime and project settings, loads the native DLL and resolves its
    /// required exports. Returns zero on failure, after logging a load failure.
    load: unsafe extern "C" fn() -> u32,
    /// `rsfg_vk_host_capabilities`.
    capabilities: unsafe extern "C" fn(flags: u32),
    /// `rsfg_boot_vulkan` or `rsfg_boot` with the setup built from the loaded settings.
    start: unsafe extern "C" fn(vulkan: u32) -> u32,
    /// Reports a refused start.
    rejected: unsafe extern "C" fn(),
    /// Resolves `rsfg_attach_engine` and registers the engine services. Zero on failure.
    attach: unsafe extern "C" fn() -> u32,
    /// `rsfg_status` session identity, zero when status was refused.
    session: unsafe extern "C" fn() -> u64,
}

/// The parsed settings prefix Rust reads. host.h checks the offsets in `rsf_godot_settings`.
#[repr(C)]
pub struct SettingsHead {
    _struct_size: u32,
    _abi_version: u32,
    sr_backend: u32,
    quality: u32,
}

/// Engine facts read by `ReScaleFrameBridge::configure`. Every read is a pure query.
#[repr(C)]
pub struct ConfigureFacts {
    viewport_valid: u32,
    vulkan: u32,
}

/// Godot and driver leaves for `configure`. `context` is passed back to every leaf.
#[repr(C)]
pub struct ConfigureCallbacks {
    context: *mut c_void,
    /// The scene tree's root viewport is the requested viewport.
    is_root: unsafe extern "C" fn(context: *mut c_void) -> u32,
    /// Parses the Dictionary into caller-owned settings and returns them.
    parse: unsafe extern "C" fn(context: *mut c_void) -> *const SettingsHead,
    /// `rsfg_vk_window` with the display server's main window.
    report_window: unsafe extern "C" fn(context: *mut c_void),
    /// `rsfg_configure`. Zero on refusal.
    configure: unsafe extern "C" fn(context: *mut c_void, settings: *const SettingsHead) -> u32,
    /// Queues `rsf_godot_primary_set` for the viewport on the rendering server thread.
    queue_primary: unsafe extern "C" fn(context: *mut c_void),
    /// Sets the viewport's scaling mode for a `viewport.h` mode, then its scale.
    apply_scaling: unsafe extern "C" fn(context: *mut c_void, mode: u32, scale: f32),
}

/// Shutdown leaves.
#[repr(C)]
pub struct ShutdownCallbacks {
    /// Releases the motion resources on the rendering server thread and waits for it, when
    /// the rendering server exists.
    release_render: unsafe extern "C" fn(),
    /// `rsfg_shutdown`.
    native_shutdown: unsafe extern "C" fn(),
}

/// The status fields that drive the viewport extent.
#[repr(C)]
pub struct Extent {
    requested_sr: u32,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
}

/// Rendering server thread leaves for the queued render frame.
#[repr(C)]
pub struct RenderCallbacks {
    /// `rsfg_render_begin`.
    render_begin: unsafe extern "C" fn(source_frame_id: u64),
    /// `rsfg_status`, copied into `out`. Zero when status was refused.
    status_extent: unsafe extern "C" fn(out: *mut Extent) -> u32,
    /// `viewport_set_scaling_3d_scale` on the viewport with this RID identity.
    set_scale: unsafe extern "C" fn(viewport: u64, scale: f64),
}

const _: () = {
    assert!(size_of::<BootFacts>() == 16 && align_of::<BootFacts>() == 4);
    assert!(size_of::<BootCallbacks>() == 6 * size_of::<usize>());
    assert!(offset_of!(SettingsHead, sr_backend) == 8);
    assert!(offset_of!(SettingsHead, quality) == 12);
    assert!(size_of::<ConfigureFacts>() == 8 && align_of::<ConfigureFacts>() == 4);
    assert!(size_of::<ConfigureCallbacks>() == 7 * size_of::<usize>());
    assert!(size_of::<ShutdownCallbacks>() == 2 * size_of::<usize>());
    assert!(size_of::<Extent>() == 20 && align_of::<Extent>() == 4);
    assert!(offset_of!(Extent, render_width) == 4);
    assert!(offset_of!(Extent, output_height) == 16);
    assert!(size_of::<RenderCallbacks>() == 3 * size_of::<usize>());
};

static READY: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static REQUESTED_SR: AtomicU32 = AtomicU32::new(INITIAL_SR);
static SESSION: AtomicU64 = AtomicU64::new(0);

const R: Ordering = Ordering::Relaxed;
const S: Ordering = Ordering::SeqCst;

fn ready() -> bool {
    READY.load(R)
}

fn active() -> bool {
    ready() && ENABLED.load(S)
}

/// Any thread: the native host booted and attached its engine services.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_host_ready() -> u32 {
    ready() as u32
}

/// Any thread: the native host is ready and a `configure` was accepted since boot or shutdown.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_host_active() -> u32 {
    active() as u32
}

/// Any thread: the native session identity read after boot, zero before it.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_host_session() -> u64 {
    SESSION.load(R)
}

/// Main thread, once from `Main::setup2`: decides whether the module boots the native host and
/// runs the loader and driver leaves in order.
///
/// # Safety
/// `facts` and `callbacks` must point to valid records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_host_boot(
    facts: *const BootFacts,
    callbacks: *const BootCallbacks,
) {
    let (facts, callbacks) = unsafe { (&*facts, &*callbacks) };
    if facts.enabled_setting == 0 || facts.headless != 0 || facts.editor != 0 {
        return;
    }
    // SAFETY: valid callbacks, see above.
    unsafe {
        if (callbacks.load)() == 0 {
            return;
        }
        (callbacks.capabilities)(VK_GRAPH_HANDOFF);
        let started = (callbacks.start)(facts.vulkan) != 0;
        READY.store(started, R);
        if !started {
            (callbacks.rejected)();
            return;
        }
        if (callbacks.attach)() == 0 {
            READY.store(false, R);
            return;
        }
        SESSION.store((callbacks.session)(), R);
    }
}

/// The caller's thread: `ReScaleFrameBridge::configure` for the root viewport. Returns 1 when
/// the native host accepted the settings and the viewport now renders through the module.
///
/// # Safety
/// `facts` and `callbacks` must point to valid records, and `parse` must return valid settings
/// that stay alive until this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_host_configure(
    facts: *const ConfigureFacts,
    callbacks: *const ConfigureCallbacks,
) -> u32 {
    let (facts, callbacks) = unsafe { (&*facts, &*callbacks) };
    let context = callbacks.context;
    // SAFETY: valid callbacks and settings, see above.
    unsafe {
        if !ready() || facts.viewport_valid == 0 || (callbacks.is_root)(context) == 0 {
            return 0;
        }
        let settings = (callbacks.parse)(context);
        if facts.vulkan != 0 {
            (callbacks.report_window)(context);
        }
        if (callbacks.configure)(context, settings) == 0 {
            return 0;
        }
        let (sr_backend, quality) = ((*settings).sr_backend, (*settings).quality);
        REQUESTED_SR.store(sr_backend, S);
        (callbacks.queue_primary)(context);
        let mode = crate::rsf_godot_viewport_mode(sr_backend);
        (callbacks.apply_scaling)(
            context,
            mode,
            crate::rsf_godot_viewport_initial_scale(mode, quality),
        );
    }
    ENABLED.store(true, S);
    1
}

/// Main thread, at scene-level module uninitialization: disables the hooks, releases the render
/// resources, then stops a booted native host. `ready` stays set, as before.
///
/// # Safety
/// `callbacks` must point to valid callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_host_shutdown(callbacks: *const ShutdownCallbacks) {
    let callbacks = unsafe { &*callbacks };
    ENABLED.store(false, S);
    // SAFETY: valid callbacks, see above.
    unsafe {
        (callbacks.release_render)();
        if ready() {
            (callbacks.native_shutdown)();
        }
    }
}

/// The status extent for the current request, if status reports it.
///
/// # Safety
/// `status_extent` must be a valid leaf.
unsafe fn current_extent(
    status_extent: unsafe extern "C" fn(*mut Extent) -> u32,
    requested: u32,
) -> Option<Extent> {
    let mut extent = Extent {
        requested_sr: 0,
        render_width: 0,
        render_height: 0,
        output_width: 0,
        output_height: 0,
    };
    // SAFETY: valid leaf, see above.
    let ok = unsafe { status_extent(&mut extent) } != 0;
    (ok && extent.requested_sr == requested && REQUESTED_SR.load(S) == requested).then_some(extent)
}

/// Rendering server thread: the queued draw begins. Stores the render frame, notifies the
/// native host, then applies the reported width ratio to the primary viewport while a temporal
/// request is current.
///
/// # Safety
/// `callbacks` must point to valid callbacks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_host_render_begin(
    source_frame_id: u64,
    gameplay: u32,
    seconds: f64,
    callbacks: *const RenderCallbacks,
) {
    let callbacks = unsafe { &*callbacks };
    frame::rsf_godot_render_frame_begin(source_frame_id, gameplay, seconds);
    // SAFETY: valid callbacks, see above.
    unsafe { (callbacks.render_begin)(source_frame_id) };
    let requested = REQUESTED_SR.load(S);
    let viewport = primary::rsf_godot_primary_viewport();
    if crate::rsf_godot_viewport_mode(requested) != crate::TEMPORAL || viewport == 0 {
        return;
    }
    // SAFETY: valid callbacks, see above.
    if let Some(extent) = unsafe { current_extent(callbacks.status_extent, requested) }
        && extent.render_width != 0
        && extent.output_width != 0
    {
        let scale = f64::from(extent.render_width) / f64::from(extent.output_width);
        // SAFETY: valid callbacks, see above.
        unsafe { (callbacks.set_scale)(viewport, scale) };
    }
}

/// Rendering server thread, from the patched viewport sizing: the internal extent for the
/// primary target. Native requests use the output size, Spatial keeps Godot's size, and other
/// requests use the reported render size when it matches this output.
///
/// # Safety
/// `internal` must point to two writable `int32_t` values, and `status_extent` must be a valid
/// leaf.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_host_render_size(
    target: u64,
    output_width: i32,
    output_height: i32,
    internal: *mut i32,
    status_extent: unsafe extern "C" fn(*mut Extent) -> u32,
) {
    if primary::rsf_godot_primary_sizing(active() as u32, target) == 0 {
        return;
    }
    let requested = REQUESTED_SR.load(S);
    let mode = crate::rsf_godot_viewport_mode(requested);
    let size = if mode == crate::NATIVE {
        (output_width, output_height)
    } else if mode == crate::SPATIAL {
        return;
    } else {
        // SAFETY: valid leaf, see above.
        match unsafe { current_extent(status_extent, requested) } {
            Some(extent)
                if extent.output_width == output_width as u32
                    && extent.output_height == output_height as u32
                    && extent.render_width != 0
                    && extent.render_height != 0 =>
            {
                (extent.render_width as i32, extent.render_height as i32)
            }
            _ => return,
        }
    };
    // SAFETY: two writable values, see above.
    unsafe {
        internal.write(size.0);
        internal.add(1).write(size.1);
    }
}
