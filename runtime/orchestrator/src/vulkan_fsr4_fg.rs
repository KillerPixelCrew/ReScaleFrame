// SPDX-License-Identifier: MIT
use crate::fsr4_fg_bridge::{
    BRIDGE_ABI, BridgeCreate, BridgeFrame, BridgeInfo, BridgeStatus, UNSUPPORTED,
};
use crate::sr_session::has_capacity;
use crate::vulkan_dx12_fg::{self as shared, *};
use crate::vulkan_fg_abi::{
    RSF_FGVK_ABI_VERSION, rsf_fgvk_frame as Frame, rsf_fgvk_setup as Setup,
    rsf_fgvk_status as Status,
};
use crate::vulkan_fsr4_sr::directory;
use crate::vulkan_sr_abi::{RSF_SRVK_ABI_VERSION, rsf_srvk_setup as Graphics};
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{CStr, c_void};
use std::ptr;

#[repr(C)]
struct Options {
    struct_size: u32,
    abi_version: u32,
    window: Handle,
    timeline_enabled: u32,
    generated_count: u32,
}

#[repr(C)]
struct Ops {
    probe: unsafe extern "C" fn(u64, *const u16, *mut BridgeInfo) -> i32,
    bridge_create: unsafe extern "C" fn(*const BridgeCreate, *mut Handle) -> i32,
    bridge_info: unsafe extern "C" fn(Handle, *mut BridgeInfo) -> i32,
    bridge_start: unsafe extern "C" fn(Handle, Handle) -> i32,
    bridge_present: unsafe extern "C" fn(Handle, *const BridgeFrame, *mut BridgeStatus) -> i32,
    bridge_drain: Release,
    bridge_prepare_destroy: Release,
    bridge_destroy: Release,
    interop_create: unsafe extern "C" fn(*const Graphics, Handle, u64, u32, *mut Handle) -> i32,
    importer: InteropOps,
    graphics: GraphicsOps,
}

const _: () = {
    assert!(size_of::<Options>() == 24);
    assert!(size_of::<Ops>() == 43 * size_of::<Handle>());
};

type Owner = shared::Owner<Fsr4>;

struct Fsr4 {
    ops: &'static Ops,
    directory: *const u16,
    bridge: Handle,
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_probe(
    ops: &'static Ops,
    setup: *const Graphics,
    info: *mut BridgeInfo,
) -> i32 {
    if !unsafe { has_capacity(setup) } || info.is_null() {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.instance.is_null()
        || setup.physical_device.is_null()
        || setup.instance_proc.is_none()
        || setup.runtime_directory_utf8.is_null()
    {
        return INVALID;
    }
    if setup.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let directory = match directory::encode(unsafe { CStr::from_ptr(setup.runtime_directory_utf8) })
    {
        Ok(path) => path,
        Err(error) => return directory_error(error),
    };
    let mut luid = 0;
    let result = unsafe { (ops.graphics.adapter_luid)(setup, &mut luid) };
    if result != 0 {
        return result;
    }
    unsafe { (ops.probe)(luid, directory.as_ptr(), info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_create(
    ops: &'static Ops,
    setup: *const Setup,
    options: *const Options,
    output: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    if output.is_null() || chain.is_null() {
        return INVALID;
    }
    unsafe {
        output.write(ptr::null_mut());
        chain.write(0);
    }
    if !unsafe { has_capacity(setup) && has_capacity(options) } {
        return INVALID;
    }
    let (setup, options) = unsafe { (&*setup, &*options) };
    if setup.abi_version != RSF_FGVK_ABI_VERSION || options.abi_version != BRIDGE_ABI {
        return ABI_MISMATCH;
    }
    let graphics = &setup.graphics;
    let Some(swapchain) = shared::describe(&ops.graphics, setup) else {
        return INVALID;
    };
    let options = Start {
        window: options.window,
        timeline_enabled: options.timeline_enabled,
        generated_count: options.generated_count,
    };
    if !setup_valid(setup, &options)
        || options.generated_count != 1
        || graphics.runtime_directory_utf8.is_null()
    {
        return INVALID;
    }
    if !swapchain.supported() {
        return FORMAT_UNSUPPORTED;
    }
    let directory =
        match directory::encode(unsafe { CStr::from_ptr(graphics.runtime_directory_utf8) }) {
            Ok(path) => path,
            Err(error) => return directory_error(error),
        };
    let vendor = Fsr4 {
        ops,
        directory: directory.as_ptr(),
        bridge: ptr::null_mut(),
    };
    unsafe {
        shared::create(
            vendor,
            &ops.graphics,
            setup,
            swapchain,
            options,
            output,
            chain,
        )
    }
}

fn directory_error(error: directory::Error) -> i32 {
    match error {
        directory::Error::Invalid => INVALID,
        directory::Error::Allocation => -1, // VK_ERROR_OUT_OF_HOST_MEMORY
    }
}

// The bridge is created and retired by the shared WSI owner. It keeps the
// vendor's own ABI and success/error reporting at the FSR4 boundary.
impl Vendor for Fsr4 {
    type Submission<'a> = Fsr4Submission<'a>;

    fn create(
        &mut self,
        graphics: &Graphics,
        swapchain: &Swapchain,
        luid: u64,
        count: u32,
    ) -> Result<u64, i32> {
        let create = BridgeCreate {
            struct_size: size_of::<BridgeCreate>() as u32,
            abi_version: BRIDGE_ABI,
            adapter_luid: luid,
            runtime_directory: std::mem::replace(&mut self.directory, ptr::null()),
            width: swapchain.width,
            height: swapchain.height,
            depth_inverted: graphics.depth_inverted,
            depth_infinite: graphics.depth_infinite,
            motion_jittered: graphics.motion_jittered,
            generated_count: count,
        };
        succeeded(unsafe { (self.ops.bridge_create)(&create, &mut self.bridge) })?;
        let mut info = BridgeInfo {
            struct_size: size_of::<BridgeInfo>() as u32,
            abi_version: BRIDGE_ABI,
            adapter_luid: 0,
            provider_version: 0,
            vendor_id: 0,
            max_generated: 0,
        };
        succeeded(unsafe { (self.ops.bridge_info)(self.bridge, &mut info) })?;
        if info.adapter_luid != luid {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        if info.provider_version == 0 || info.max_generated != 1 {
            return Err(UNSUPPORTED);
        }
        Ok(info.provider_version)
    }

    fn start(&self, _: &Graphics, _: &Swapchain, options: Start) -> Result<(), i32> {
        succeeded(unsafe { (self.ops.bridge_start)(self.bridge, options.window) })
    }

    fn capture_backend(&self) -> u32 {
        4
    }

    fn begin_present(&self, _: u64, enabled: bool) -> Result<Fsr4Submission<'_>, i32> {
        Ok(Fsr4Submission {
            vendor: self,
            enabled,
        })
    }

    fn set_generated(&self, count: u32) -> Result<(), i32> {
        if count == 1 { Ok(()) } else { Err(UNSUPPORTED) }
    }

    fn drain(&self) -> Result<(), i32> {
        if self.bridge.is_null() {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.bridge_drain)(self.bridge) })
    }

    fn prepare_destroy(&mut self) -> Result<(), i32> {
        if self.bridge.is_null() {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.bridge_prepare_destroy)(self.bridge) })
    }

    fn destroy(&mut self) -> Result<(), i32> {
        if self.bridge.is_null() {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.bridge_destroy)(self.bridge) })?;
        self.bridge = ptr::null_mut();
        Ok(())
    }

    fn import(&self, graphics: &Graphics, luid: u64, enabled: u32) -> Result<Handle, i32> {
        let mut importer = ptr::null_mut();
        succeeded(unsafe {
            (self.ops.interop_create)(graphics, self.bridge, luid, enabled, &mut importer)
        })?;
        Ok(importer)
    }

    fn importer_ops(&self) -> &InteropOps {
        &self.ops.importer
    }
}

struct Fsr4Submission<'a> {
    vendor: &'a Fsr4,
    enabled: bool,
}

impl Submission for Fsr4Submission<'_> {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn present(&mut self, frame: PresentFrame<'_>) -> (i32, PresentStatus) {
        let input = BridgeFrame {
            struct_size: size_of::<BridgeFrame>() as u32,
            abi_version: BRIDGE_ABI,
            record: frame.record.map_or(ptr::null(), ptr::from_ref),
            color: frame.color,
            depth: frame.depth,
            motion: frame.motion,
            hudless: frame.hudless,
            motion_scale_x: frame.scales[0],
            motion_scale_y: frame.scales[1],
            ready_value: frame.ready,
            done_value: frame.done,
            enabled: u32::from(self.enabled),
            vsync: frame.vsync,
        };
        let mut status = BridgeStatus {
            struct_size: size_of::<BridgeStatus>() as u32,
            abi_version: BRIDGE_ABI,
            frames_presented: 0,
            fg_enabled: 0,
            result: 0,
            terminal: 0,
            generated_callbacks: 0,
        };
        let result =
            unsafe { (self.vendor.ops.bridge_present)(self.vendor.bridge, &input, &mut status) };
        (
            result,
            PresentStatus {
                frames_presented: u64::from(status.frames_presented),
                enabled: status.fg_enabled,
                result: status.result,
                terminal: status.terminal != 0,
                generated_callbacks: status.generated_callbacks,
            },
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_images(
    owner: *const Owner,
    count: *mut u32,
    out: *mut u64,
) -> i32 {
    unsafe { shared::images(owner, count, out) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_acquire(
    owner: *mut Owner,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    unsafe { shared::acquire(owner, timeout, semaphore, fence, index) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_prepare(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { shared::prepare(owner, commands, frame) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_hudless(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { shared::hudless(owner, commands, frame) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_submitted(owner: *mut Owner, commands: *const Handle, count: u32) {
    unsafe { shared::submitted(owner, commands, count) };
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_enable(owner: *mut Owner, enabled: u32) -> i32 {
    unsafe { rsf_fsr4gvk_enable_options(owner, enabled, 1) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_enable_options(
    owner: *mut Owner,
    enabled: u32,
    count: u32,
) -> i32 {
    unsafe { shared::enable_options(owner, enabled, count) }
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fsr4gvk_max_generated(owner: *const Owner) -> u32 {
    u32::from(!owner.is_null())
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_present(
    owner: *mut Owner,
    info: *const PresentInfo,
    source: *const Record,
    explicit: u32,
) -> i32 {
    unsafe { shared::present(owner, info, source, explicit) }
}

type PresentCallback = unsafe extern "C" fn(Handle, &PresentInfo, *const Record, u32) -> i32;

unsafe extern "C" {
    fn rsf_dx12gvk_present_marshaled(
        call: PresentCallback,
        owner: Handle,
        info: *const c_void,
        source: *const Record,
        explicit: u32,
    ) -> i32;
}

unsafe extern "C" fn present_marshaled(
    owner: Handle,
    info: &PresentInfo,
    source: *const Record,
    explicit: u32,
) -> i32 {
    unsafe { rsf_core_fsr4gvk_present(owner.cast(), info, source, explicit) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_present(owner: *mut Owner, info: *const c_void) -> i32 {
    unsafe { rsf_dx12gvk_present_marshaled(present_marshaled, owner.cast(), info, ptr::null(), 0) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_present_source(
    owner: *mut Owner,
    info: *const c_void,
    source: *const Record,
) -> i32 {
    unsafe { rsf_dx12gvk_present_marshaled(present_marshaled, owner.cast(), info, source, 1) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_status(owner: *const Owner, status: *mut Status) -> i32 {
    unsafe { shared::status(owner, status) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_drain(owner: *mut Owner) -> i32 {
    unsafe { shared::drain(owner) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_destroy(owner: *mut Owner) -> i32 {
    unsafe { shared::destroy(owner) }
}
