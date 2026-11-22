// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::dx12_vulkan_interop::{Owner as Interop, checked};
use crate::dx12_vulkan_interop_abi::rsf_dx12vk_shared_image;
use crate::frame_capture_vk::sr_capture_inputs;
use crate::sharpening_policy;
use crate::sr_session::has_capacity;
use crate::vulkan_external_copy::{self, Barrier, FAMILY_EXTERNAL, FAMILY_IGNORED, Transfer};
use crate::vulkan_sr::{
    GENERAL, R32_FLOAT, RG16_FLOAT, RGBA16_FLOAT, SAMPLED_USAGE, SHADER_READ, SHARPENING_VENDOR,
    STORAGE_USAGE, TRANSFER_DST, valid_frame_record, valid_image,
};
use crate::vulkan_sr_abi::*;
use crate::vulkan_sr_split::{SplitLease, admit_setup, private_setup_result};
use rsf_game_sys::{RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_SR, RSF_FRAME_FLAG_RESET};
use std::ffi::{CStr, c_void};
use std::ptr;

#[path = "../../vulkan/fsr4/bridge_abi.rs"]
mod bridge_abi;
use bridge_abi::{
    RSF_FSR4_BRIDGE_ABI_MISMATCH as ABI_MISMATCH, RSF_FSR4_BRIDGE_ABI_VERSION,
    RSF_FSR4_BRIDGE_INVALID_ARGUMENT as INVALID, rsf_fsr4_bridge_create as BridgeCreate,
    rsf_fsr4_bridge_frame as BridgeFrame, rsf_fsr4_bridge_info as Info,
};

#[path = "vulkan_fsr4_sr_abi.rs"]
mod platform_abi;
use platform_abi::rsf_fsr4vk_platform as Platform;

type Handle = *mut c_void;
type Setup = rsf_srvk_setup;
type Handoff = rsf_fsr4vk_handoff;
type Frame = rsf_srvk_frame;
type Record = rsf_game_sys::rsf_frame_record;
type Image = rsf_srvk_image;
type SharedImage = rsf_dx12vk_shared_image;

const OUT_OF_HOST_MEMORY: i32 = -1;
const INITIALIZATION_FAILED: i32 = -3;
const NOT_READY: i32 = 1;
const UNKNOWN: i32 = -13;
const UNDEFINED: u32 = 0;
const TRANSFER_SRC: u32 = 6;
const WAIT_NS: u64 = 10_000_000_000;
const INPUT_FORMATS: [u32; 3] = [RGBA16_FLOAT, R32_FLOAT, RG16_FLOAT];

pub(super) mod directory {
    use std::ffi::CStr;

    pub(crate) enum Error {
        Invalid,
        Allocation,
    }

    /// Convert a nonempty runtime directory to a NUL-terminated Windows path,
    /// with at most 32767 UTF-16 units including the terminator.
    pub(crate) fn encode(path: &CStr) -> Result<Vec<u16>, Error> {
        const MAX_UNITS: usize = 32767;
        let path = path.to_str().map_err(|_| Error::Invalid)?;
        if path.is_empty() || path.len() > (MAX_UNITS - 1) * 3 {
            return Err(Error::Invalid);
        }
        let units = path.encode_utf16().count() + 1;
        if units > MAX_UNITS {
            return Err(Error::Invalid);
        }
        let mut wide = Vec::new();
        wide.try_reserve_exact(units)
            .map_err(|_| Error::Allocation)?;
        wide.extend(path.encode_utf16());
        wide.push(0);
        Ok(wide)
    }
}

struct Graphics {
    ops: &'static Platform,
    handle: Handle,
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe { (self.ops.close.unwrap())(self.handle) };
    }
}

#[derive(Default)]
struct History {
    last_frame: u64,
    done: u64,
    session: u64,
    view: u32,
    generation: u32,
    valid: bool,
}

struct Owner {
    graphics: Graphics,
    bridge: Handle,
    interop: Option<Interop>,
    setup: Setup,
    handoff: Handoff,
    lease: SplitLease,
    render: [u32; 2],
    images: Option<[SharedImage; 4]>,
    history: History,
    terminal: bool,
}

impl Owner {
    fn ops(&self) -> &'static Platform {
        self.graphics.ops
    }

    fn output(&self) -> [u32; 2] {
        [self.setup.output_width, self.setup.output_height]
    }

    fn info(&self, info: *mut Info) -> i32 {
        unsafe { (self.ops().bridge_info.unwrap())(self.bridge, info) }
    }

    fn initialize(&mut self, luid: u64, directory: &[u16]) -> Result<(), i32> {
        let ops = self.ops();
        let output = self.output();
        let request = BridgeCreate {
            struct_size: size_of::<BridgeCreate>() as u32,
            abi_version: RSF_FSR4_BRIDGE_ABI_VERSION,
            adapter_luid: luid,
            runtime_directory: directory.as_ptr(),
            render_width: output[0],
            render_height: output[1],
            output_width: output[0],
            output_height: output[1],
            depth_inverted: self.setup.depth_inverted,
            depth_infinite: self.setup.depth_infinite,
            motion_jittered: self.setup.motion_jittered,
            reserved: 0,
        };
        checked(unsafe { (ops.bridge_create.unwrap())(&request, &mut self.bridge) })?;
        let [width, height] = &mut self.render;
        checked(unsafe {
            (ops.bridge_plan.unwrap())(self.bridge, self.setup.quality, width, height)
        })?;
        let mut info = Info {
            struct_size: size_of::<Info>() as u32,
            abi_version: RSF_FSR4_BRIDGE_ABI_VERSION,
            adapter_luid: 0,
            provider_version: 0,
            vendor_id: 0,
            int8_compatibility: 0,
        };
        checked(self.info(&mut info))?;
        checked(unsafe {
            (ops.capture_initialize.unwrap())(
                self.graphics.handle,
                &self.setup,
                info.provider_version,
            )
        })?;

        let interop = self.interop.as_mut().unwrap();
        interop.import_timeline(self.bridge, ops.create_fence.unwrap())?;
        let mut import = |format, extent: [u32; 2], writable| {
            let extra_usage = if writable {
                SAMPLED_USAGE | STORAGE_USAGE
            } else {
                0
            };
            interop.add_image(format, extent, extra_usage, |handle, resource| unsafe {
                (ops.create_image.unwrap())(
                    self.bridge,
                    extent[0],
                    extent[1],
                    format,
                    u32::from(writable),
                    handle,
                    resource,
                )
            })
        };
        let [color, depth, motion] = INPUT_FORMATS;
        self.images = Some([
            import(color, self.render, false)?,
            import(depth, self.render, false)?,
            import(motion, self.render, false)?,
            import(RGBA16_FLOAT, output, true)?,
        ]);
        Ok(())
    }

    fn admits(&self, frame: &Frame, record: &Record) -> bool {
        let history = &self.history;
        let device = self.setup.device;
        valid_frame_record(&self.setup, frame, record, self.render, self.output())
            && record.frame_id > history.last_frame
            && record.view_id != 0
            && record.resource_generation != 0
            && (history.session == 0
                || (history.session == record.session_id && history.view == record.view_id))
            && record.flags & (RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_AMBIGUOUS_ID) == 0
            && frame.reactive.image == 0
            && frame.composition.image == 0
            && [&frame.color, &frame.depth, &frame.motion]
                .into_iter()
                .zip(INPUT_FORMATS)
                .all(|(image, format)| {
                    image.copy_source != 0
                        && image.image != frame.output.image
                        && valid_image(image, device, self.render, SHADER_READ, &[format])
                })
            && valid_image(
                &frame.output,
                device,
                self.output(),
                TRANSFER_DST,
                &[RGBA16_FLOAT],
            )
    }

    fn transition(&self, commands: Handle, barrier: &Barrier) {
        unsafe { (self.ops().transition.unwrap())(self.graphics.handle, commands, barrier) };
    }

    fn copy(&self, commands: Handle, source: u64, target: u64, [width, height]: [u32; 2]) {
        unsafe {
            (self.ops().copy.unwrap())(
                self.graphics.handle,
                commands,
                source,
                target,
                width,
                height,
            )
        };
    }

    fn capture(&self, commands: Handle, image: &Image, record: &Record, role: &CStr, reset: u32) {
        unsafe {
            (self.ops().capture_record.unwrap())(
                self.graphics.handle,
                commands,
                image,
                record,
                role.as_ptr(),
                reset,
            )
        };
    }

    fn record_inputs(
        &self,
        commands: Handle,
        frame: &Frame,
        record: &Record,
        images: &[SharedImage; 4],
        reset: u32,
    ) {
        unsafe { (self.ops().capture_begin.unwrap())(record) };
        for (image, role) in sr_capture_inputs(frame) {
            self.capture(commands, image, record, role, reset);
        }
        let initialized = self.history.done != 0;
        let family = self.handoff.queue_family;
        for (source, target) in [frame.color, frame.depth, frame.motion]
            .into_iter()
            .zip(images)
        {
            let transfer = Transfer {
                source: source.image,
                target: target.image,
                source_extent: self.render,
                target_extent: self.render,
            };
            let plan = vulkan_external_copy::plan(
                transfer,
                source.layout,
                family,
                initialized,
                FAMILY_IGNORED,
            );
            for barrier in &plan.before {
                self.transition(commands, barrier);
            }
            self.copy(
                commands,
                plan.transfer.source,
                plan.transfer.target,
                plan.transfer.target_extent,
            );
            for barrier in &plan.after {
                self.transition(commands, barrier);
            }
        }
        if !initialized {
            let output = images[3].image;
            self.transition(
                commands,
                &Barrier::new(output, UNDEFINED, GENERAL, FAMILY_IGNORED, FAMILY_IGNORED),
            );
            self.transition(
                commands,
                &Barrier::new(output, GENERAL, GENERAL, family, FAMILY_EXTERNAL),
            );
        }
    }

    fn record_output(
        &self,
        commands: Handle,
        frame: &Frame,
        record: &Record,
        output: SharedImage,
        reset: u32,
    ) {
        let family = self.handoff.queue_family;
        self.transition(
            commands,
            &Barrier::new(output.image, GENERAL, GENERAL, FAMILY_EXTERNAL, family),
        );
        // Capture the SDK output after acquiring it and before RSF sharpening.
        let captured = Image {
            device: self.setup.device,
            image: output.image,
            format: output.format,
            width: output.width,
            height: output.height,
            layout: GENERAL,
            copy_source: 1,
        };
        unsafe { (self.ops().capture_parameters.unwrap())(frame, reset) };
        self.capture(commands, &captured, record, c"sr-output-linear", reset);
        unsafe {
            (self.ops().sharpen.unwrap())(
                self.graphics.handle,
                &self.setup,
                commands,
                frame,
                output.image,
            )
        };
        self.transition(
            commands,
            &Barrier::new(
                output.image,
                GENERAL,
                TRANSFER_SRC,
                FAMILY_IGNORED,
                FAMILY_IGNORED,
            ),
        );
        self.copy(commands, output.image, frame.output.image, self.output());
        self.transition(
            commands,
            &Barrier::new(output.image, TRANSFER_SRC, GENERAL, family, FAMILY_EXTERNAL),
        );
    }

    fn evaluate(
        &mut self,
        commands: Handle,
        frame: &Frame,
        record: &Record,
        images: [SharedImage; 4],
        continuation: &mut Handle,
    ) -> Result<(), i32> {
        let Some(done) = self.history.done.checked_add(2) else {
            return Err(INVALID);
        };
        if self.lease.pending() {
            return Err(NOT_READY);
        }
        let ops = self.ops();
        let reset = u32::from(
            !self.history.valid
                || self.history.last_frame.checked_add(1) != Some(record.frame_id)
                || record.flags & RSF_FRAME_FLAG_RESET != 0
                || self.history.generation != record.resource_generation,
        );
        self.record_inputs(commands, frame, record, &images, reset);
        let timeline = self.interop.as_ref().unwrap().timeline();
        let ready = done - 1;
        self.lease.begin(commands);
        *continuation = ptr::null_mut();
        let result = unsafe {
            (ops.split.unwrap())(
                &self.handoff,
                commands,
                timeline,
                self.history.done,
                ready,
                done,
                continuation,
            )
        };
        if result != 0 || continuation.is_null() || *continuation == commands {
            *continuation = ptr::null_mut();
            self.terminal = true;
            return Err(if result == 0 {
                INITIALIZATION_FAILED
            } else {
                result
            });
        }

        self.lease.resume(*continuation);
        self.history = History {
            last_frame: record.frame_id,
            done,
            session: record.session_id,
            view: record.view_id,
            generation: record.resource_generation,
            valid: false,
        };
        let camera = &record.camera;
        let [color, depth, motion, output] = images.map(|image| image.dx12_resource);
        let [output_width, output_height] = self.output();
        let dispatch = BridgeFrame {
            struct_size: size_of::<BridgeFrame>() as u32,
            abi_version: RSF_FSR4_BRIDGE_ABI_VERSION,
            color,
            depth,
            motion,
            output,
            render_width: self.render[0],
            render_height: self.render[1],
            output_width,
            output_height,
            jitter_x: camera.jitter_pixels[0],
            jitter_y: camera.jitter_pixels[1],
            motion_scale_x: frame.motion_scale_x,
            motion_scale_y: frame.motion_scale_y,
            delta_ms: record.frame_time_ms,
            near_plane: camera.near_plane,
            far_plane: camera.far_plane,
            vertical_fov: camera.vertical_fov_radians,
            pre_exposure: frame.pre_exposure,
            reset,
            ready_value: ready,
            done_value: done,
        };
        let settings = &frame.sharpening;
        let strength = if settings.struct_size != 0 && settings.mode == SHARPENING_VENDOR {
            settings.sharpness
        } else {
            0.0
        };
        unsafe { (ops.bridge_sharpness.unwrap())(self.bridge, strength) };
        // The bridge orders its done signal after the producer's ready value
        // even when the SDK refuses the dispatch. The host can record fallback
        // on this continuation, with no copy from the failed provider output.
        checked(unsafe { (ops.bridge_evaluate.unwrap())(self.bridge, &dispatch) })?;
        self.history.valid = true;
        self.record_output(*continuation, frame, record, images[3], dispatch.reset);
        Ok(())
    }

    fn destroy(&mut self) -> Result<(), i32> {
        if self.terminal || self.lease.pending() {
            return Err(NOT_READY);
        }
        let ops = self.ops();
        let graphics = self.graphics.handle;
        if self.history.done != 0 {
            checked(unsafe {
                (ops.wait.unwrap())(
                    graphics,
                    self.interop.as_ref().unwrap().timeline(),
                    self.history.done,
                    WAIT_NS,
                )
            })?;
        }
        // DX12 done protects the shared payload. Vulkan idle additionally
        // covers the continuation, including copies, captures and sharpening.
        checked(unsafe { (ops.idle.unwrap())(graphics) })?;
        // Once SDK teardown starts, keep the imports for retries but close
        // admission. A capture or SDK failure can leave the context half retired.
        self.images = None;
        if !self.bridge.is_null() {
            checked(unsafe { (ops.bridge_prepare_destroy.unwrap())(self.bridge) })?;
        }
        if unsafe { (ops.capture_flush.unwrap())(graphics) } == 0 {
            return Err(UNKNOWN);
        }

        self.history.done = 0;
        unsafe { (ops.clear_sharpen.unwrap())(graphics) };
        drop(self.interop.take());
        checked(unsafe { (ops.bridge_destroy.unwrap())(self.bridge) })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4vk_create(
    ops: &'static Platform,
    setup: *const Setup,
    handoff: *const Handoff,
    output: *mut *mut Owner,
) -> i32 {
    if output.is_null() {
        return INVALID;
    }
    unsafe { output.write(ptr::null_mut()) };
    let (setup, handoff) = match unsafe { admit_setup(setup, handoff) } {
        Ok(args) => args,
        Err(error) => return private_setup_result(error),
    };
    unsafe { create_admitted(ops, setup, handoff, output) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4vk_create_admitted(
    ops: &'static Platform,
    setup: &Setup,
    handoff: &Handoff,
    output: *mut *mut Owner,
) -> i32 {
    unsafe { create_admitted(ops, *setup, *handoff, output) }
}

unsafe fn create_admitted(
    ops: &'static Platform,
    mut setup: Setup,
    handoff: Handoff,
    output: *mut *mut Owner,
) -> i32 {
    let wide = match directory::encode(unsafe { CStr::from_ptr(setup.runtime_directory_utf8) }) {
        Ok(wide) => wide,
        Err(directory::Error::Invalid) => return INVALID,
        Err(directory::Error::Allocation) => return OUT_OF_HOST_MEMORY,
    };
    setup.runtime_directory_utf8 = ptr::null();
    let mut graphics = ptr::null_mut();
    let result = unsafe { (ops.open.unwrap())(&setup, &mut graphics) };
    if result != 0 {
        return result;
    }
    let graphics = Graphics {
        ops,
        handle: graphics,
    };
    let Some(interop_ops) = (unsafe { ops.interop.as_ref() }) else {
        return INITIALIZATION_FAILED;
    };
    let interop = match Interop::open(interop_ops, &setup, ops.release_image.unwrap()) {
        Ok(interop) => interop,
        Err(error) => return error,
    };
    let luid = match Interop::adapter_luid(interop_ops, &setup) {
        Ok(luid) => luid,
        Err(error) => return error,
    };
    let owner = allocate_handle(Owner {
        graphics,
        bridge: ptr::null_mut(),
        interop: Some(interop),
        setup,
        handoff,
        lease: SplitLease::new(handoff.abi_version),
        render: [0; 2],
        images: None,
        history: History::default(),
        terminal: false,
    });
    if owner.is_null() {
        return OUT_OF_HOST_MEMORY;
    }
    unsafe { output.write(owner) };
    if let Err(error) = unsafe { &mut *owner }.initialize(luid, &wide) {
        if unsafe { rsf_fsr4vk_destroy(owner) } == 0 {
            unsafe { output.write(ptr::null_mut()) };
        }
        return error;
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_plan(
    owner: *const Owner,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if owner.images.is_none() || owner.terminal || width.is_null() || height.is_null() {
        return INVALID;
    }
    unsafe {
        width.write(owner.render[0]);
        height.write(owner.render[1]);
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_info(owner: *const Owner, info: *mut Info) -> i32 {
    unsafe { owner.as_ref() }.map_or(INVALID, |owner| owner.info(info))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_evaluate(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
    continuation: *mut Handle,
) -> i32 {
    let Some(continuation) = (unsafe { continuation.as_mut() }) else {
        return INVALID;
    };
    *continuation = ptr::null_mut();
    let owner = unsafe { owner.as_mut() };
    if owner
        .as_ref()
        .is_some_and(|owner| owner.terminal || owner.images.is_none())
    {
        return INVALID;
    }
    *continuation = commands;
    let Some(owner) = owner else {
        return INVALID;
    };
    if commands.is_null() || !unsafe { has_capacity(frame) } || unsafe { (*frame).record.is_null() }
    {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if frame.sharpening.struct_size != 0 && !sharpening_policy::valid(&frame.sharpening) {
        return INVALID;
    }
    if !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    if !owner.admits(frame, record) {
        return INVALID;
    }
    owner
        .evaluate(commands, frame, record, owner.images.unwrap(), continuation)
        .err()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_destroy(owner: *mut Owner) -> i32 {
    let Some(owned) = (unsafe { owner.as_mut() }) else {
        return 0;
    };
    if let Err(error) = owned.destroy() {
        return error;
    }
    unsafe { drop(Box::from_raw(owner)) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_retire_commands(owner: *mut Owner, commands: Handle) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        unsafe { (owner.ops().retire.unwrap())(owner.graphics.handle, commands) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_submitted(owner: *mut Owner, commands: Handle) -> i32 {
    if let Some(owner) = unsafe { owner.as_mut() }
        && owner.lease.submitted(commands)
    {
        return 0;
    }
    INVALID
}
