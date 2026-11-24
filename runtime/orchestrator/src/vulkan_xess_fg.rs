// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_DX12VK_INTEROP_ABI_MISMATCH, RSF_DX12VK_INTEROP_ABI_VERSION,
    RSF_DX12VK_INTEROP_INVALID_ARGUMENT, rsf_dx12vk_interop_callbacks as InteropCallbacks,
    rsf_dx12vk_shared_image as InteropImage,
};
use crate::sys::{
    RSF_FGVK_ABI_VERSION, RSF_FGVK_LATENCY_ABI_VERSION, rsf_fgvk_frame as Frame,
    rsf_fgvk_latency_status as LatencyStatus, rsf_fgvk_setup as Setup, rsf_fgvk_status as Status,
};
use crate::sys::{RSF_SRVK_ABI_VERSION, rsf_srvk_setup as Graphics};
use crate::vulkan_dispatch::succeeded;
use crate::vulkan_dx12_fg::{self as shared, *};
use crate::vulkan_latency_frames::{Marker, legacy_present_source};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, rsf_frame_record as Record};
// Linking the XeSS session also provides the rsf_core_xess_bridge_* entries bridge.c calls.
use crate::sys::rsf_xessgvk_options as Options;
use bridge_abi::{
    RSF_XESS_BRIDGE_ABI_MISMATCH as XESS_ABI_MISMATCH, RSF_XESS_BRIDGE_ABI_VERSION as BRIDGE_ABI,
    RSF_XESS_BRIDGE_INVALID_ARGUMENT as XESS_INVALID, rsf_xess_bridge_create as BridgeCreate,
    rsf_xess_bridge_frame as BridgeFrame, rsf_xess_bridge_present_status as BridgeStatus,
    rsf_xess_bridge_start as BridgeStart,
};
use rsf_vk_xess_fg::bridge_abi;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard};

const ABI: u32 = 1;
const REQUIRED_MARKERS: u8 = 0b1111;
const LAST_MARKER: u32 = 6;

// Integrated bridge exports from native_fg_xess_vulkan_present.c. The shared
// graphics table and importer are this crate's own and are selected below.
#[repr(C)]
struct Ops {
    interop_create: unsafe extern "C" fn(*const Graphics, Handle, u32, *mut Handle) -> i32,
    create: unsafe extern "C" fn(*const BridgeCreate, *mut Handle) -> i32,
    start: unsafe extern "C" fn(Handle, *const BridgeStart) -> i32,
    max_generated: unsafe extern "C" fn(Handle) -> u32,
    generated: unsafe extern "C" fn(Handle, u32) -> i32,
    enable: unsafe extern "C" fn(Handle, u32) -> i32,
    present: unsafe extern "C" fn(Handle, *const BridgeFrame, *mut BridgeStatus) -> i32,
    capture_source: unsafe extern "C" fn(Handle, *const Record) -> i32,
    generated_callbacks: unsafe extern "C" fn(Handle) -> u64,
    idle: Release,
    prepare_destroy: Release,
    destroy: Release,
    sleep: unsafe extern "C" fn(Handle, u32) -> i32,
    marker: unsafe extern "C" fn(Handle, u32, i32) -> i32,
    latency_options: unsafe extern "C" fn(Handle, u32, u32) -> i32,
}

#[repr(C)]
struct ImportCreate {
    adapter_luid: unsafe extern "C" fn(Handle) -> u64,
    create: unsafe extern "C" fn(*const Graphics, *const InteropCallbacks, u32, *mut Handle) -> i32,
    create_image: unsafe extern "C" fn(Handle, u32, u32, u32, *mut Handle, *mut Handle) -> i32,
    release_image: unsafe extern "C" fn(Handle),
    create_fence: unsafe extern "C" fn(Handle, *mut Handle) -> i32,
}

const _: () = {
    assert!(size_of::<Options>() == 32);
    assert!(size_of::<BridgeCreate>() == 24);
    assert!(size_of::<BridgeStart>() == 32);
    assert!(size_of::<BridgeFrame>() == 216);
    assert!(size_of::<BridgeStatus>() == 24);
    assert!(size_of::<Ops>() == 15 * size_of::<Handle>());
    assert!(size_of::<ImportCreate>() == 5 * size_of::<Handle>());
};

fn status_result(result: i32) -> i32 {
    match result {
        INVALID | RSF_DX12VK_INTEROP_INVALID_ARGUMENT => XESS_INVALID,
        ABI_MISMATCH | RSF_DX12VK_INTEROP_ABI_MISMATCH => XESS_ABI_MISMATCH,
        other => other,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_interop_create(
    ops: &ImportCreate,
    setup: *const Graphics,
    bridge: Handle,
    timeline_enabled: u32,
    output: *mut Handle,
) -> i32 {
    if output.is_null() {
        return XESS_INVALID;
    }
    unsafe { output.write(ptr::null_mut()) };
    if !unsafe { has_capacity(setup) } || bridge.is_null() || timeline_enabled != 1 {
        return XESS_INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.instance.is_null()
        || setup.physical_device.is_null()
        || setup.device.is_null()
        || setup.instance_proc.is_none()
        || setup.device_proc.is_none()
    {
        return XESS_INVALID;
    }
    if setup.abi_version != RSF_SRVK_ABI_VERSION {
        return XESS_ABI_MISMATCH;
    }
    let callbacks = InteropCallbacks {
        struct_size: size_of::<InteropCallbacks>() as u32,
        abi_version: RSF_DX12VK_INTEROP_ABI_VERSION,
        adapter_luid: unsafe { (ops.adapter_luid)(bridge) },
        user: bridge,
        create_image: Some(ops.create_image),
        release_image: Some(ops.release_image),
        create_fence: Some(ops.create_fence),
    };
    status_result(unsafe { (ops.create)(setup, &callbacks, timeline_enabled, output) })
}

use crate::dx12_vulkan_interop::{rsf_dx12vk_interop_destroy, rsf_dx12vk_interop_image};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessvk_interop_image(
    owner: Handle,
    format: u32,
    width: u32,
    height: u32,
    output: *mut InteropImage,
) -> i32 {
    status_result(unsafe { rsf_dx12vk_interop_image(owner, format, width, height, output) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessvk_interop_destroy(owner: Handle, queues_drained: u32) -> i32 {
    status_result(unsafe { rsf_dx12vk_interop_destroy(owner, queues_drained) })
}

#[derive(Clone, Copy, Default)]
struct CpuFrame {
    id: u32,
    markers: u8,
    pending: u8,
}

struct Latency {
    status: LatencyStatus,
    minimum_interval_us: u32,
    last_attempted: u32,
    frames: [CpuFrame; SLOT_COUNT],
}

impl Latency {
    fn new() -> Self {
        Self {
            status: LatencyStatus {
                struct_size: size_of::<LatencyStatus>() as u32,
                abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
                mode: 0,
                available: 0,
                sleep_calls: 0,
                marker_calls: 0,
                result: 0,
            },
            minimum_interval_us: 0,
            last_attempted: 0,
            frames: [CpuFrame::default(); SLOT_COUNT],
        }
    }

    fn frame(&mut self, id: u32) -> &mut CpuFrame {
        &mut self.frames[id as usize % SLOT_COUNT]
    }

    fn matches(&mut self, id: u32) -> bool {
        let frame = self.frame(id);
        frame.id == id && frame.markers & REQUIRED_MARKERS == REQUIRED_MARKERS
    }
}

#[derive(Default)]
struct PresentState {
    last_sdk_id: u32,
    active: bool,
}

type Owner = shared::Owner<Xess>;

pub(crate) struct Xess {
    ops: &'static Ops,
    directory: *const u16,
    bridge: Handle,
    latency: Mutex<Latency>,
    // Sleep holds only this gate. Marker and Present never acquire it.
    pacing: Mutex<()>,
    presentation: Mutex<PresentState>,
}

impl Xess {
    fn new(ops: &'static Ops, directory: *const u16) -> Self {
        Self {
            ops,
            directory,
            bridge: ptr::null_mut(),
            latency: Mutex::new(Latency::new()),
            pacing: Mutex::new(()),
            presentation: Mutex::new(PresentState::default()),
        }
    }

    fn sleep(&self, id: u32) -> i32 {
        if id == 0 {
            return XESS_INVALID;
        }
        let Ok(_pacing) = self.pacing.try_lock() else {
            return NOT_READY;
        };
        {
            let mut latency = self.latency.lock().unwrap();
            if latency.status.available == 0 || id <= latency.last_attempted {
                return NOT_READY;
            }
            latency.last_attempted = id;
        }
        // A new frame may be waiting for an older frame to Present. Publish
        // the new mapping only after sleep succeeds, including on slot reuse.
        let result = unsafe { (self.ops.sleep)(self.bridge, id) };
        let mut latency = self.latency.lock().unwrap();
        latency.status.result = result;
        if result == 0 {
            latency.status.sleep_calls += 1;
            *latency.frame(id) = CpuFrame {
                id,
                ..CpuFrame::default()
            };
        }
        result
    }

    fn marker(&self, id: u32, marker: u32) -> i32 {
        if id == 0 || marker > LAST_MARKER {
            return XESS_INVALID;
        }
        let bit = 1 << marker;
        {
            let mut latency = self.latency.lock().unwrap();
            let frame = latency.frame(id);
            if frame.id != id {
                return NOT_READY;
            }
            if frame.markers & bit != 0 {
                return 0;
            }
            if frame.pending & bit != 0 {
                return NOT_READY;
            }
            frame.pending |= bit;
        }
        let result = unsafe { (self.ops.marker)(self.bridge, id, marker as i32) };
        let mut latency = self.latency.lock().unwrap();
        latency.status.result = result;
        latency.status.marker_calls += u64::from(result == 0);
        let frame = latency.frame(id);
        if frame.id == id {
            frame.pending &= !bit;
            if result == 0 {
                frame.markers |= bit;
            }
        }
        result
    }

    fn options(&self, mode: u32, interval: u32) -> i32 {
        if mode > 1 {
            return XESS_INVALID;
        }
        let Ok(_pacing) = self.pacing.try_lock() else {
            return NOT_READY;
        };
        {
            let latency = self.latency.lock().unwrap();
            if latency.status.available == 0 {
                return NOT_READY;
            }
            if latency.status.mode == mode && latency.minimum_interval_us == interval {
                return 0;
            }
        }
        // SetSleepMode requires the DX12 work on the XeLL context to finish.
        // Exclude new DX12 Present work only while applying a changed mode.
        let Ok(_present) = self.presentation.try_lock() else {
            return NOT_READY;
        };
        let mut result = unsafe { (self.ops.idle)(self.bridge) };
        if result == 0 {
            result = unsafe { (self.ops.latency_options)(self.bridge, interval, mode) };
        }
        let mut latency = self.latency.lock().unwrap();
        latency.status.result = result;
        if result == 0 {
            latency.status.mode = mode;
            latency.minimum_interval_us = interval;
        }
        result
    }
}

impl Vendor for Xess {
    const MAX_SOURCE_FRAME: u64 = u32::MAX as u64;
    type Submission<'a> = XessSubmission<'a>;

    fn create(&mut self, _: &Graphics, _: &Swapchain, luid: u64, _: u32) -> Result<u64, i32> {
        let create = BridgeCreate {
            struct_size: size_of::<BridgeCreate>() as u32,
            abi_version: BRIDGE_ABI,
            adapter_luid: luid,
            runtime_directory: std::mem::replace(&mut self.directory, ptr::null()),
        };
        succeeded(unsafe { (self.ops.create)(&create, &mut self.bridge) })?;
        Ok(0)
    }

    fn start(&self, graphics: &Graphics, swapchain: &Swapchain, options: Start) -> Result<(), i32> {
        let start = BridgeStart {
            struct_size: size_of::<BridgeStart>() as u32,
            abi_version: BRIDGE_ABI,
            window: options.window,
            width: swapchain.width,
            height: swapchain.height,
            inverted_depth: graphics.depth_inverted,
            generated_count: options.generated_count,
        };
        succeeded(unsafe { (self.ops.start)(self.bridge, &start) })?;
        let mut latency = self.latency.lock().unwrap();
        latency.status.available = 1;
        // Creation applies enabled mode with a zero minimum interval.
        latency.status.mode = 1;
        Ok(())
    }

    fn capture_backend(&self) -> u32 {
        5
    }

    fn implicit_present_source(&self) -> Option<u64> {
        legacy_present_source()
    }

    fn begin_present(&self, frame: u64, requested: bool) -> Result<XessSubmission<'_>, i32> {
        let mut state = self.presentation.lock().unwrap();
        let cpu_id = u32::try_from(frame).ok().filter(|&id| id != 0);
        let source_id = cpu_id.filter(|&id| id > state.last_sdk_id);
        let enabled =
            requested && source_id.is_some_and(|id| self.latency.lock().unwrap().matches(id));
        let sdk_id = source_id
            .or_else(|| state.last_sdk_id.checked_add(1))
            .ok_or(UNKNOWN)?;
        if state.active != enabled {
            let result = unsafe { (self.ops.enable)(self.bridge, u32::from(enabled)) };
            if result < 0 {
                return Err(result);
            }
            state.active = enabled;
        }
        Ok(XessSubmission {
            vendor: self,
            state,
            sdk_id,
            cpu_id,
        })
    }

    fn set_generated(&self, count: u32) -> Result<(), i32> {
        let _present = self.presentation.lock().unwrap();
        let result = unsafe { (self.ops.generated)(self.bridge, count) };
        if result < 0 { Err(result) } else { Ok(()) }
    }

    fn drain(&self) -> Result<(), i32> {
        if self.bridge.is_null() {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.idle)(self.bridge) })
    }

    fn prepare_destroy(&mut self) -> Result<(), i32> {
        if self.bridge.is_null() {
            return Ok(());
        }
        self.latency.get_mut().unwrap().status.available = 0;
        succeeded(unsafe { (self.ops.prepare_destroy)(self.bridge) })
    }

    fn destroy(&mut self) -> Result<(), i32> {
        if self.bridge.is_null() {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.destroy)(self.bridge) })?;
        self.bridge = ptr::null_mut();
        Ok(())
    }

    fn import(&self, graphics: &Graphics, _: u64, enabled: u32) -> Result<Handle, i32> {
        let mut interop = ptr::null_mut();
        succeeded(unsafe {
            (self.ops.interop_create)(graphics, self.bridge, enabled, &mut interop)
        })?;
        Ok(interop)
    }

    fn importer_ops(&self) -> &InteropOps {
        &IMPORTER
    }
}

pub(crate) struct XessSubmission<'a> {
    vendor: &'a Xess,
    state: MutexGuard<'a, PresentState>,
    sdk_id: u32,
    cpu_id: Option<u32>,
}

impl Submission for XessSubmission<'_> {
    fn enabled(&self) -> bool {
        self.state.active
    }

    fn present(&mut self, frame: PresentFrame<'_>) -> (i32, PresentStatus) {
        const IDENTITY: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let record = frame
            .record
            .filter(|record| record.frame_id == u64::from(self.sdk_id));
        let enabled_record = record.filter(|_| self.enabled());
        let [jitter_x, jitter_y] =
            enabled_record.map_or([0.0; 2], |record| record.camera.jitter_pixels);
        let [motion_scale_x, motion_scale_y] = frame.scales;
        let input = BridgeFrame {
            struct_size: size_of::<BridgeFrame>() as u32,
            abi_version: BRIDGE_ABI,
            frame_id: self.sdk_id,
            reset: u32::from(
                enabled_record.is_none_or(|record| record.flags & RSF_FRAME_FLAG_RESET != 0),
            ),
            vsync: frame.vsync,
            reserved: 0,
            color: frame.color,
            depth: frame.depth,
            motion: frame.motion,
            hudless: frame.hudless,
            view_matrix: enabled_record.map_or(IDENTITY, |record| record.camera.world_to_view),
            projection_matrix: enabled_record.map_or(IDENTITY, |record| record.camera.view_to_clip),
            jitter_x,
            jitter_y,
            motion_scale_x,
            motion_scale_y,
            ready_fence_value: frame.ready,
            done_fence_value: frame.done,
        };
        let mut status = BridgeStatus {
            struct_size: size_of::<BridgeStatus>() as u32,
            abi_version: BRIDGE_ABI,
            frames_presented: 0,
            fg_enabled: 0,
            fg_result: 0,
            reserved: 0,
        };
        let vendor = self.vendor;
        if let Some(record) = record {
            unsafe { (vendor.ops.capture_source)(vendor.bridge, record) };
        }
        // The render boundary can identify a real pass-through frame even
        // without guides. Only that CPU identity can receive XeLL markers.
        if let Some(id) = self.cpu_id {
            vendor.marker(id, Marker::PresentBegin as u32);
        }
        let result = unsafe { (vendor.ops.present)(vendor.bridge, &input, &mut status) };
        if let Some(id) = self.cpu_id {
            vendor.marker(id, Marker::PresentEnd as u32);
        }
        self.state.last_sdk_id = self.sdk_id;
        (
            result,
            PresentStatus {
                frames_presented: u64::from(status.frames_presented),
                enabled: status.fg_enabled,
                result: status.fg_result,
                // The bridge queues a recovery signal on a failed Present. A
                // real timeline and DX12 drain determine whether retirement is safe.
                terminal: false,
                generated_callbacks: unsafe { (vendor.ops.generated_callbacks)(vendor.bridge) },
            },
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessgvk_create(
    ops: &'static Ops,
    setup: *const Setup,
    options: *const Options,
    output: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    if output.is_null() || chain.is_null() {
        return XESS_INVALID;
    }
    unsafe {
        output.write(ptr::null_mut());
        chain.write(0);
    }
    if !unsafe { has_capacity(setup) && has_capacity(options) } {
        return XESS_INVALID;
    }
    let (setup, options) = unsafe { (&*setup, &*options) };
    if setup.abi_version != RSF_FGVK_ABI_VERSION || options.abi_version != ABI {
        return XESS_ABI_MISMATCH;
    }
    let start = Start {
        window: options.window,
        timeline_enabled: options.timeline_enabled,
        generated_count: options.generated_count,
    };
    let Some(swapchain) = shared::describe(platform::graphics(), setup) else {
        return XESS_INVALID;
    };
    if !setup_valid(setup, &start)
        || start.generated_count == 0
        || options.intel_runtime_directory.is_null()
    {
        return XESS_INVALID;
    }
    let vendor = Xess::new(ops, options.intel_runtime_directory);
    status_result(unsafe {
        shared::create(
            vendor,
            platform::graphics(),
            setup,
            swapchain,
            start,
            output,
            chain,
        )
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_images(
    owner: *const Owner,
    count: *mut u32,
    out: *mut u64,
) -> i32 {
    unsafe { shared::images(owner, count, out) }
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_acquire(
    owner: *mut Owner,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    unsafe { shared::acquire(owner, timeout, semaphore, fence, index) }
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_prepare(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    status_result(unsafe { shared::prepare(owner, commands, frame) })
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_hudless(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    status_result(unsafe { shared::hudless(owner, commands, frame) })
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_submitted(
    owner: *mut Owner,
    commands: *const Handle,
    count: u32,
) {
    unsafe { shared::submitted(owner, commands, count) };
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_enable(owner: *mut Owner, enabled: u32) -> i32 {
    status_result(unsafe { shared::enable(owner, enabled, None) })
}
pub(crate) unsafe fn rsf_xessgvk_enable_options(
    owner: *mut Owner,
    enabled: u32,
    count: u32,
) -> i32 {
    status_result(unsafe { shared::enable_options(owner, enabled, count) })
}
pub(crate) unsafe fn rsf_xessgvk_max_generated(owner: *const Owner) -> u32 {
    unsafe { owner.as_ref() }.map_or(0, |owner| unsafe {
        (owner.vendor.ops.max_generated)(owner.vendor.bridge)
    })
}
unsafe extern "C" fn rsf_core_xessgvk_present(
    owner: *mut Owner,
    info: *const PresentInfo,
    source: *const Record,
    explicit: u32,
) -> i32 {
    unsafe { shared::present(owner, info, source, explicit) }
}

/// The marshaller's callback: the owner behind the provider handle.
unsafe extern "C" fn present_owner(
    owner: Handle,
    info: &PresentInfo,
    source: *const Record,
    exact: u32,
) -> i32 {
    unsafe { rsf_core_xessgvk_present(owner.cast(), info, source, exact) }
}

/// Present routes through the shared V02 marshaller (vulkan_dx12_fg/platform.rs).
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_present(owner: *mut Owner, info: *const c_void) -> i32 {
    unsafe {
        crate::vulkan_dx12_fg::platform::rsf_dx12gvk_present_marshaled(
            present_owner,
            owner.cast(),
            info,
            ptr::null(),
            0,
        )
    }
}

pub(crate) unsafe fn rsf_xessgvk_present_source(
    owner: *mut Owner,
    info: *const c_void,
    source: *const Record,
) -> i32 {
    unsafe {
        crate::vulkan_dx12_fg::platform::rsf_dx12gvk_present_marshaled(
            present_owner,
            owner.cast(),
            info,
            source,
            1,
        )
    }
}

/// native_fg_xess_vulkan_present.c binds the XeSS bridge exports, which exist only in builds with the
/// XeSS-FG SDK, and calls `rsf_core_xessgvk_create`. vulkan_fg dispatches to it outside unit tests,
/// which substitute a fake.
#[cfg(all(not(test), feature = "xess-fg-vk"))]
pub(crate) use crate::sys::rsf_xessgvk_create;

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_status(
    owner: *const Owner,
    status: *mut Status,
) -> i32 {
    status_result(unsafe { shared::status(owner, status) })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_drain(owner: *mut Owner) -> i32 {
    status_result(unsafe { shared::drain(owner) })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_destroy(owner: *mut Owner) -> i32 {
    status_result(unsafe { shared::destroy(owner) })
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_sleep(owner: *const Owner, id: u32) -> i32 {
    unsafe { owner.as_ref() }.map_or(XESS_INVALID, |owner| owner.vendor.sleep(id))
}
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_xessgvk_latency(
    owner: *const Owner,
    id: u32,
    marker: u32,
) -> i32 {
    unsafe { owner.as_ref() }.map_or(XESS_INVALID, |owner| owner.vendor.marker(id, marker))
}
pub(crate) unsafe fn rsf_xessgvk_latency_options(
    owner: *const Owner,
    mode: u32,
    interval: u32,
) -> i32 {
    unsafe { owner.as_ref() }.map_or(XESS_INVALID, |owner| owner.vendor.options(mode, interval))
}
pub(crate) unsafe fn rsf_xessgvk_latency_status(
    owner: *const Owner,
    status: *mut LatencyStatus,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return XESS_INVALID;
    };
    if !unsafe { has_capacity(status) } {
        return XESS_INVALID;
    }
    if unsafe { (*status).abi_version } != RSF_FGVK_LATENCY_ABI_VERSION {
        return XESS_ABI_MISMATCH;
    }
    unsafe { status.write(owner.vendor.latency.lock().unwrap().status) };
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::{Condvar, mpsc};
    use std::time::Duration;

    const PRIOR: u32 = 1;
    const NEXT: u32 = PRIOR + SLOT_COUNT as u32;
    const FAILED: u32 = NEXT + SLOT_COUNT as u32;
    const BOUND: Duration = Duration::from_secs(3);

    #[test]
    fn interop_validates_graphics_before_querying_the_bridge() {
        unsafe extern "C" fn luid(bridge: Handle) -> u64 {
            let calls = unsafe { &*bridge.cast::<Cell<u32>>() };
            calls.set(calls.get() + 1);
            7
        }
        unsafe extern "C" fn create(
            _: *const Graphics,
            callbacks: *const InteropCallbacks,
            timeline: u32,
            _: *mut Handle,
        ) -> i32 {
            let callbacks = unsafe { &*callbacks };
            assert_eq!(callbacks.adapter_luid, 7);
            assert_eq!(timeline, 1);
            -9
        }
        unsafe extern "C" fn resolver(
            _: Handle,
            _: *const std::ffi::c_char,
        ) -> crate::sys::rsf_vk_function {
            None
        }
        unsafe extern "C" fn image(
            _: Handle,
            _: u32,
            _: u32,
            _: u32,
            _: *mut Handle,
            _: *mut Handle,
        ) -> i32 {
            -9
        }
        unsafe extern "C" fn release(_: Handle) {}
        unsafe extern "C" fn fence(_: Handle, _: *mut Handle) -> i32 {
            -9
        }
        let operations = ImportCreate {
            adapter_luid: luid,
            create,
            create_image: image,
            release_image: release,
            create_fence: fence,
        };
        let calls = Cell::new(0u32);
        let bridge = ptr::from_ref(&calls).cast_mut().cast();
        let attempt = |setup, expected| {
            let mut out = ptr::without_provenance_mut(1);
            assert_eq!(
                unsafe { rsf_core_xessvk_interop_create(&operations, setup, bridge, 1, &mut out) },
                expected
            );
            assert!(out.is_null());
        };
        let mut setup: Graphics = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<Graphics>() as u32 - 1;
        setup.abi_version = RSF_SRVK_ABI_VERSION + 1;
        setup.device = bridge;
        setup.physical_device = bridge;
        setup.instance = bridge;
        setup.device_proc = Some(resolver);
        setup.instance_proc = Some(resolver);
        attempt(ptr::null(), XESS_INVALID);
        attempt(&setup, XESS_INVALID);
        setup.struct_size += 1;
        attempt(&setup, XESS_ABI_MISMATCH);
        assert_eq!(calls.get(), 0);
        setup.abi_version = RSF_SRVK_ABI_VERSION;
        attempt(&setup, -9);
        assert_eq!(calls.get(), 1);
    }

    #[derive(Default)]
    struct Calls {
        entered: bool,
        presented: bool,
        markers: Vec<(u32, i32)>,
        options: Vec<(u32, u32)>,
        idles: u32,
    }

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Calls>,
        changed: Condvar,
    }

    fn fake<'a>(bridge: Handle) -> &'a Fake {
        unsafe { &*bridge.cast::<Fake>() }
    }

    unsafe extern "C" fn sleep(bridge: Handle, id: u32) -> i32 {
        if id == FAILED {
            return -91;
        }
        if id != NEXT {
            return 0;
        }
        let fake = fake(bridge);
        let mut calls = fake.calls.lock().unwrap();
        calls.entered = true;
        fake.changed.notify_all();
        let (calls, _) = fake
            .changed
            .wait_timeout_while(calls, BOUND, |calls| !calls.presented)
            .unwrap();
        if calls.presented { 0 } else { -92 }
    }

    unsafe extern "C" fn mark(bridge: Handle, id: u32, marker: i32) -> i32 {
        let fake = fake(bridge);
        let mut calls = fake.calls.lock().unwrap();
        calls.markers.push((id, marker));
        if id == PRIOR && marker == Marker::PresentEnd as i32 {
            calls.presented = true;
            fake.changed.notify_all();
        }
        0
    }

    fn operations() -> Box<Ops> {
        Box::new(Ops {
            interop_create: unused_import,
            create: unused_create,
            start: unused_start,
            max_generated: generated_count,
            generated: toggle,
            enable: toggle,
            present,
            capture_source,
            generated_callbacks: callbacks,
            idle,
            prepare_destroy: release,
            destroy: release,
            sleep,
            marker: mark,
            latency_options: options,
        })
    }

    unsafe extern "C" fn unused_import(
        _: *const Graphics,
        _: Handle,
        _: u32,
        _: *mut Handle,
    ) -> i32 {
        XESS_INVALID
    }
    unsafe extern "C" fn unused_create(_: *const BridgeCreate, _: *mut Handle) -> i32 {
        XESS_INVALID
    }
    unsafe extern "C" fn unused_start(_: Handle, _: *const BridgeStart) -> i32 {
        XESS_INVALID
    }
    unsafe extern "C" fn generated_count(_: Handle) -> u32 {
        3
    }
    unsafe extern "C" fn toggle(_: Handle, _: u32) -> i32 {
        0
    }
    unsafe extern "C" fn present(_: Handle, _: *const BridgeFrame, _: *mut BridgeStatus) -> i32 {
        0
    }
    unsafe extern "C" fn capture_source(_: Handle, _: *const Record) -> i32 {
        0
    }
    unsafe extern "C" fn callbacks(_: Handle) -> u64 {
        0
    }
    unsafe extern "C" fn release(_: Handle) -> i32 {
        0
    }

    unsafe extern "C" fn idle(bridge: Handle) -> i32 {
        fake(bridge).calls.lock().unwrap().idles += 1;
        0
    }

    unsafe extern "C" fn options(bridge: Handle, interval: u32, mode: u32) -> i32 {
        fake(bridge)
            .calls
            .lock()
            .unwrap()
            .options
            .push((mode, interval));
        0
    }

    #[test]
    fn previous_present_completes_during_sleep_and_failed_sleep_keeps_mapping() {
        let fake = Fake::default();
        let ops = Box::into_raw(operations());
        let mut vendor = Box::new(Xess::new(unsafe { &*ops }, ptr::null()));
        vendor.bridge = ptr::from_ref(&fake).cast_mut().cast();
        {
            let mut state = vendor.latency.lock().unwrap();
            state.status.available = 1;
            state.status.mode = 1;
        }
        assert_eq!(vendor.sleep(PRIOR), 0);
        for marker in 0..4 {
            assert_eq!(vendor.marker(PRIOR, marker), 0);
        }
        assert_eq!(vendor.options(1, 0), 0);
        let address = ptr::from_ref(vendor.as_ref()).expose_provenance();
        std::thread::scope(|scope| {
            let (completed, slept) = mpsc::channel();
            scope.spawn(move || {
                // Scope joins both threads before the fake bridge or its owner is released.
                let vendor = unsafe { &*ptr::with_exposed_provenance::<Xess>(address) };
                completed.send(vendor.sleep(NEXT)).unwrap();
            });
            let calls = fake.calls.lock().unwrap();
            let (calls, _) = fake
                .changed
                .wait_timeout_while(calls, BOUND, |calls| !calls.entered)
                .unwrap();
            let entered = calls.entered;
            drop(calls);
            assert!(entered);

            assert_eq!(vendor.sleep(NEXT), NOT_READY);
            assert_eq!(vendor.marker(NEXT, 0), NOT_READY);
            assert_eq!(vendor.options(0, 1500), NOT_READY);
            assert!(
                vendor
                    .begin_present(u64::from(PRIOR), true)
                    .unwrap()
                    .enabled()
            );
            // The same real boundary still owes Present markers if FG is
            // unavailable for this frame and the SDK receives no guides.
            let mut older = vendor.begin_present(u64::from(PRIOR), false).unwrap();
            assert!(!older.enabled());
            assert_eq!(
                older
                    .present(PresentFrame {
                        record: None,
                        color: ptr::null_mut(),
                        depth: ptr::null_mut(),
                        motion: ptr::null_mut(),
                        hudless: ptr::null_mut(),
                        scales: [1.0; 2],
                        ready: 1,
                        done: 2,
                        vsync: 0,
                    })
                    .0,
                0
            );
            drop(older);
            assert_eq!(slept.recv_timeout(BOUND), Ok(0));
        });
        assert_eq!(vendor.marker(NEXT, 0), 0);
        assert_eq!(vendor.marker(NEXT, 0), 0);
        assert_eq!(vendor.sleep(FAILED), -91);
        assert_eq!(vendor.sleep(FAILED), NOT_READY);
        assert_eq!(vendor.sleep(PRIOR), NOT_READY);
        assert_eq!(vendor.marker(FAILED, 0), NOT_READY);
        assert_eq!(vendor.marker(NEXT, 1), 0);
        assert_eq!(vendor.options(0, 1500), 0);
        assert_eq!(vendor.options(0, 1500), 0);
        let status = vendor.latency.lock().unwrap().status;
        assert_eq!(
            (status.mode, status.sleep_calls, status.marker_calls),
            (0, 2, 8)
        );
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.idles, 1);
        assert_eq!(calls.options, [(0, 1500)]);
        assert!(!calls.markers.iter().any(|(id, _)| *id == FAILED));
        drop(calls);
        drop(vendor);
        drop(unsafe { Box::from_raw(ops) });
    }
}
