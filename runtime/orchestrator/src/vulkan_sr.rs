// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::vulkan_sr_abi::*;
use crate::vulkan_sr_split::normalize_frame;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::{LazyLock, Mutex};

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const WRONG_API: i32 = -12;

// Vulkan C ABI values, checked against the native headers at the graphics bridge.
const R8_UNORM: u32 = 9;
const RG16_FLOAT: u32 = 83;
const RGBA16_FLOAT: u32 = 97;
const R32_FLOAT: u32 = 100;
const D32_FLOAT: u32 = 126;
const SHADER_READ: u32 = 5;
const TRANSFER_DST: u32 = 7;

type Setup = rsf_srvk_setup;
type Frame = rsf_srvk_frame;
type Image = rsf_srvk_image;
type Device = *mut c_void;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_setup_admit(setup: *const Setup) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let s = unsafe { &*setup };
    if s.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if s.instance.is_null()
        || s.physical_device.is_null()
        || s.device.is_null()
        || s.device_proc.is_none()
        || s.instance_proc.is_none()
        || s.runtime_directory_utf8.is_null()
        || s.output_width == 0
        || s.output_height == 0
        || (s.quality > 4 && (s.backend != RSF_SR_XESS || s.quality > 5))
        || s.depth_inverted > 1
        || s.depth_infinite > 1
        || s.motion_jittered > 1
    {
        return INVALID;
    }
    if matches!(
        s.backend,
        RSF_SR_DLSS | RSF_SR_XESS | RSF_SR_FSR2 | RSF_SR_FSR3
    ) {
        0
    } else {
        NOT_SUPPORTED
    }
}

fn valid_image(
    image: &Image,
    device: Device,
    extent: [u32; 2],
    layout: u32,
    formats: &[u32],
) -> bool {
    image.device == device
        && image.image != 0
        && [image.width, image.height] == extent
        && image.layout == layout
        && formats.contains(&image.format)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fallback_admit(call: *const rsf_srvk_fallback) -> i32 {
    if !unsafe { has_capacity(call) } {
        return INVALID;
    }
    let call = unsafe { &*call };
    if call.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let source = &call.color;
    let output = &call.output;
    if call.instance.is_null()
        || call.physical_device.is_null()
        || call.device.is_null()
        || call.instance_proc.is_none()
        || call.device_proc.is_none()
        || call.command_buffer.is_null()
        || !valid_image(
            source,
            call.device,
            [source.width, source.height],
            SHADER_READ,
            &[RGBA16_FLOAT],
        )
        || !valid_image(
            output,
            call.device,
            [output.width, output.height],
            TRANSFER_DST,
            &[RGBA16_FLOAT],
        )
        || source.image == output.image
        || source.copy_source == 0
        || [source.width, source.height, output.width, output.height]
            .into_iter()
            .any(|dim| dim == 0 || dim > i32::MAX as u32)
    {
        return INVALID;
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_normalize(frame: *const Frame, out: *mut Frame) -> i32 {
    match unsafe { normalize_frame(frame) } {
        Ok(frame) => {
            unsafe { out.write(frame) };
            0
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_frame_admit(
    setup: &Setup,
    frame: &Frame,
    planned_width: u32,
    planned_height: u32,
    xess: u32,
) -> i32 {
    if !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    let camera = &record.camera;
    // XeSS owns a queried inclusive range. All leased images still follow
    // the actual camera/record size, even when it differs from the plan.
    let render = if xess != 0 {
        [record.render_width, record.render_height]
    } else {
        [planned_width, planned_height]
    };
    let output = [setup.output_width, setup.output_height];
    if render.contains(&0)
        || record.abi_version != RSF_GAME_FRAME_ABI_VERSION
        || camera.struct_size < size_of_val(camera) as u32
        || camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
        || record.session_id == 0
        || record.frame_id == 0
        || [record.render_width, record.render_height] != render
        || [record.output_width, record.output_height] != output
        || [camera.render_width, camera.render_height] != render
        || [camera.output_width, camera.output_height] != output
        || camera.depth_inverted != setup.depth_inverted
        || camera.motion_jittered != setup.motion_jittered
        || !record.frame_time_ms.is_finite()
        || record.frame_time_ms <= 0.0
        || !frame.motion_scale_x.is_finite()
        || !frame.motion_scale_y.is_finite()
        || frame.motion_scale_x == 0.0
        || frame.motion_scale_y == 0.0
        || !frame.pre_exposure.is_finite()
        || frame.pre_exposure <= 0.0
        || !camera.near_plane.is_finite()
        || camera.near_plane <= 0.0
        || !camera.far_plane.is_finite()
        || camera.far_plane < 0.0
        || (setup.depth_infinite == 0 && camera.far_plane <= camera.near_plane)
        || !(0.0..=std::f32::consts::PI).contains(&camera.vertical_fov_radians)
        || camera.vertical_fov_radians == 0.0
        || !camera.jitter_pixels.into_iter().all(f32::is_finite)
    {
        return INVALID;
    }
    let device = setup.device;
    if !valid_image(&frame.color, device, render, SHADER_READ, &[RGBA16_FLOAT])
        || !valid_image(
            &frame.depth,
            device,
            render,
            SHADER_READ,
            &[D32_FLOAT, R32_FLOAT],
        )
        || !valid_image(&frame.motion, device, render, SHADER_READ, &[RG16_FLOAT])
        || !valid_image(&frame.output, device, output, TRANSFER_DST, &[RGBA16_FLOAT])
        || [
            &frame.reactive,
            &frame.composition,
            &frame.bias_current_color,
        ]
        .into_iter()
        .any(|mask| mask.image != 0 && !valid_image(mask, device, render, SHADER_READ, &[R8_UNORM]))
    {
        return INVALID;
    }
    0
}

type DeviceProc = unsafe extern "C" fn(Device, *const c_char) -> rsf_vk_function;
type Lookup = unsafe extern "C" fn(DeviceProc, Device, *const c_char, *mut rsf_vk_function) -> i32;

struct Dispatch {
    proc: DeviceProc,
    references: u32,
}

static DISPATCH: LazyLock<Mutex<HashMap<usize, Dispatch>>> = LazyLock::new(Mutex::default);

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_dispatch_register(device: Device, proc: rsf_vk_get_proc) -> i32 {
    let Some(proc) = proc else { return INVALID };
    let mut dispatch = DISPATCH.lock().unwrap();
    if let Some(entry) = dispatch.get_mut(&(device as usize)) {
        if !std::ptr::fn_addr_eq(entry.proc, proc) {
            return WRONG_API;
        }
        let Some(count) = entry.references.checked_add(1) else {
            return INIT_FAILED;
        };
        entry.references = count;
    } else {
        if dispatch.try_reserve(1).is_err() {
            return INIT_FAILED;
        }
        dispatch.insert(
            device as usize,
            Dispatch {
                proc,
                references: 1,
            },
        );
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_dispatch_unregister(device: Device) {
    let mut dispatch = DISPATCH.lock().unwrap();
    if let Some(entry) = dispatch.get_mut(&(device as usize)) {
        entry.references -= 1;
        if entry.references == 0 {
            dispatch.remove(&(device as usize));
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_dispatch_lookup(
    device: Device,
    name: *const c_char,
    lookup: Lookup,
) -> rsf_vk_function {
    let proc = DISPATCH
        .lock()
        .unwrap()
        .get(&(device as usize))
        .map(|entry| entry.proc)?;
    let mut result = None;
    // The native caller holds the device lease. Invoke the supplied proc outside the lock.
    if unsafe { lookup(proc, device, name, &mut result) } == 0 {
        return None;
    }
    if result.is_some() || name.is_null() {
        return result;
    }
    // Older SDKs ask for KHR spellings even when Vulkan exposes their promoted core equivalents.
    let alias = match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkGetBufferMemoryRequirements2KHR" => c"vkGetBufferMemoryRequirements2",
        b"vkGetImageMemoryRequirements2KHR" => c"vkGetImageMemoryRequirements2",
        b"vkBindBufferMemory2KHR" => c"vkBindBufferMemory2",
        b"vkBindImageMemory2KHR" => c"vkBindImageMemory2",
        _ => return None,
    };
    if unsafe { lookup(proc, device, alias.as_ptr(), &mut result) } != 0 {
        result
    } else {
        None
    }
}

#[repr(C)]
struct History {
    frame: u64,
    session: u64,
    view: u32,
    generation: u32,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_history_reset(history: &History, record: &rsf_frame_record) -> u32 {
    u32::from(
        record.flags & RSF_FRAME_FLAG_RESET != 0
            || history.frame.wrapping_add(1) != record.frame_id
            || history.session != record.session_id
            || history.view != record.view_id
            || history.generation != record.resource_generation,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_history_commit(history: &mut History, record: &rsf_frame_record) {
    *history = History {
        frame: record.frame_id,
        session: record.session_id,
        view: record.view_id,
        generation: record.resource_generation,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xess_actual_extent_may_differ_from_the_plan() {
        let device = std::ptr::dangling_mut::<u8>().cast();
        let setup = Setup {
            device,
            output_width: 128,
            output_height: 72,
            depth_infinite: 1,
            ..unsafe { std::mem::zeroed() }
        };
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = 9;
        record.session_id = 1;
        record.render_width = 64;
        record.render_height = 36;
        record.output_width = 128;
        record.output_height = 72;
        record.frame_time_ms = 16.0;
        record.camera.struct_size = size_of_val(&record.camera) as u32;
        record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        record.camera.render_width = 64;
        record.camera.render_height = 36;
        record.camera.output_width = 128;
        record.camera.output_height = 72;
        record.camera.near_plane = 0.1;
        record.camera.vertical_fov_radians = 1.0;
        let image = |id, format| Image {
            device,
            image: id,
            width: 64,
            height: 36,
            format,
            layout: SHADER_READ,
            copy_source: 1,
        };
        let mut frame = Frame {
            record: &record,
            color: image(1, RGBA16_FLOAT),
            depth: image(2, D32_FLOAT),
            motion: image(3, RG16_FLOAT),
            output: Image {
                width: 128,
                height: 72,
                layout: TRANSFER_DST,
                ..image(4, RGBA16_FLOAT)
            },
            motion_scale_x: -1.0,
            motion_scale_y: 1.0,
            pre_exposure: 1.0,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            assert_eq!(rsf_core_srvk_frame_admit(&setup, &frame, 80, 45, 1), 0);
            assert_eq!(
                rsf_core_srvk_frame_admit(&setup, &frame, 80, 45, 0),
                INVALID
            );
            frame.depth.width = 80;
            assert_eq!(
                rsf_core_srvk_frame_admit(&setup, &frame, 80, 45, 1),
                INVALID
            );
        }
    }

    unsafe extern "C" fn promoted() {}
    unsafe extern "C" fn original() {}

    unsafe extern "C" fn adapter(_: Device, name: *const c_char) -> rsf_vk_function {
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"vkGetBufferMemoryRequirements2" => Some(promoted),
            b"vkOther" => Some(original),
            _ => None,
        }
    }

    unsafe extern "C" fn different(_: Device, _: *const c_char) -> rsf_vk_function {
        None
    }

    unsafe extern "C" fn call(
        proc: DeviceProc,
        device: Device,
        name: *const c_char,
        out: *mut rsf_vk_function,
    ) -> i32 {
        unsafe { out.write(proc(device, name)) };
        1
    }

    #[test]
    fn vulkan_dispatch_keeps_aliases_and_retires_only_after_last_registration() {
        let mut device_id = 0u8;
        let device = std::ptr::from_mut(&mut device_id).cast();
        assert_eq!(rsf_core_srvk_dispatch_register(device, Some(adapter)), 0);
        assert_eq!(rsf_core_srvk_dispatch_register(device, Some(adapter)), 0);
        assert_eq!(
            rsf_core_srvk_dispatch_register(device, Some(different)),
            WRONG_API
        );
        let alias = unsafe {
            rsf_core_srvk_dispatch_lookup(
                device,
                c"vkGetBufferMemoryRequirements2KHR".as_ptr(),
                call,
            )
        };
        assert!(alias.is_some_and(|p| std::ptr::fn_addr_eq(p, promoted as unsafe extern "C" fn())));
        rsf_core_srvk_dispatch_unregister(device);
        assert!(
            unsafe { rsf_core_srvk_dispatch_lookup(device, c"vkOther".as_ptr(), call) }.is_some()
        );
        rsf_core_srvk_dispatch_unregister(device);
        assert!(
            unsafe { rsf_core_srvk_dispatch_lookup(device, c"vkOther".as_ptr(), call) }.is_none()
        );
    }
}
