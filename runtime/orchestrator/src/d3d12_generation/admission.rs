// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d12_host::{borrow, owned_by};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_RESET, RSF_FRAME_SOURCE_RENDERED,
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_camera_frame,
};
use windows::Win32::Graphics::Direct3D12::*;

const UI_COMPLETE: u32 = 5;
// The existing FG ABI admission cutoff, rounded to f32.
const MAX_VERTICAL_FOV: f32 = f32::from_bits(0x4049_0fdc);

// Read only the size field until the caller proves the rest of the record is present.
unsafe fn sized<T>(raw: *const T) -> bool {
    !raw.is_null() && unsafe { raw.cast::<u32>().read() } as usize >= size_of::<T>()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg12_setup(setup: *const Setup) -> i32 {
    let setup = unsafe { &*setup };
    let chain = &setup.chain;
    if chain.d3d12_device.is_null()
        || chain.d3d12_queue.is_null()
        || chain.width == 0
        || chain.height == 0
        || chain.buffer_count < 2
        || setup.runtime_directory_utf8.is_null()
        || chain.max_generated_frames == 0
        || !setup.view_space_to_meters.is_finite()
        || setup.view_space_to_meters <= 0.0
        || chain.width > i32::MAX as u32
        || chain.height > i32::MAX as u32
        || !matches!(chain.ui_mode, UI_NONE | UI_LAYER | HUDLESS | HUDLESS_UI)
    {
        return INVALID;
    }
    let queue: &ID3D12CommandQueue = unsafe { borrow(&chain.d3d12_queue) };
    if unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT
        || !owned_by(queue, chain.d3d12_device)
    {
        return INVALID;
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg12_command(command: Raw, device: Raw) -> i32 {
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&command) }) else {
        return INVALID;
    };
    if unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT || !owned_by(list, device) {
        return INVALID;
    }
    OK
}

fn finite_camera(camera: &rsf_camera_frame) -> bool {
    [
        camera.view_to_clip.as_slice(),
        &camera.clip_to_previous_clip,
        &camera.view_to_world,
        &camera.world_to_view,
        &camera.jitter_pixels,
        &camera.clip_to_view,
    ]
    .into_iter()
    .flatten()
    .all(|value| value.is_finite())
}

fn frame_resource(
    resource: &crate::render_abi::rsf_backend_resource,
    device: Raw,
    generation: u32,
    extent: Option<(u32, u32)>,
) -> Result<(), i32> {
    if (resource.struct_size as usize) < size_of_val(resource) || resource.resource.is_null() {
        return Err(INVALID);
    }
    if resource.generation != generation {
        return Err(STALE);
    }
    let texture: &ID3D12Resource = unsafe { borrow(&resource.resource) };
    let desc = unsafe { texture.GetDesc() };
    if !owned_by(texture, device)
        || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.SampleDesc.Count != 1
        || resource.width == 0
        || resource.height == 0
        || u64::from(resource.x) + u64::from(resource.width) > desc.Width
        || u64::from(resource.y) + u64::from(resource.height) > u64::from(desc.Height)
        || extent.is_some_and(|size| size != (resource.width, resource.height))
    {
        return Err(INVALID);
    }
    Ok(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg12_frame(
    frame: *const Frame,
    device: Raw,
    allows_fg: unsafe extern "C" fn(*const Record) -> i32,
    has_rendered_source: unsafe extern "C" fn(*const Record) -> i32,
) -> i32 {
    if !unsafe { sized(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if !unsafe { sized(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    let camera = &record.camera;
    if record.abi_version != RSF_GAME_FRAME_ABI_VERSION
        || camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    if record.frame_id == 0
        || (camera.struct_size as usize) < size_of_val(camera)
        || record.output_width == 0
        || record.output_height == 0
        || record.render_width == 0
        || record.render_height == 0
        || record.output_width > i32::MAX as u32
        || record.output_height > i32::MAX as u32
    {
        return INVALID;
    }
    let rendered = record.source_provenance == RSF_FRAME_SOURCE_RENDERED
        && unsafe { has_rendered_source(record) } != 0;
    if record.source_provenance > RSF_FRAME_SOURCE_RENDERED
        || (record.source_provenance == RSF_FRAME_SOURCE_RENDERED && !rendered)
    {
        return INVALID;
    }
    if frame.interpolate == 0 {
        return OK;
    }
    if record.session_id == 0
        || record.resource_generation == 0
        || (record.input_qpc == 0 && !rendered)
        || record.phase != UI_COMPLETE
        || unsafe { allows_fg(record) } == 0
        || record.flags & (RSF_FRAME_FLAG_RESET | RSF_FRAME_FLAG_AMBIGUOUS_ID) != 0
    {
        return NOT_READY;
    }
    if !frame.motion_scale_x.is_finite()
        || !frame.motion_scale_y.is_finite()
        || frame.motion_scale_x == 0.0
        || frame.motion_scale_y == 0.0
        || !record.frame_time_ms.is_finite()
        || record.frame_time_ms <= 0.0
        || !camera.near_plane.is_finite()
        || camera.near_plane <= 0.0
        || !camera.vertical_fov_radians.is_finite()
        || camera.vertical_fov_radians <= 0.0
        || camera.vertical_fov_radians >= MAX_VERTICAL_FOV
        || !camera.far_plane.is_finite()
        || camera.far_plane < 0.0
        || !finite_camera(camera)
    {
        return INVALID;
    }
    let render = Some((record.render_width, record.render_height));
    let display = Some((record.output_width, record.output_height));
    for (resource, extent, optional) in [
        (&frame.backbuffer, display, false),
        (&frame.depth, render, false),
        (&frame.motion, None, false),
        (&frame.hudless, display, true),
        (&frame.ui, display, true),
    ] {
        if optional && resource.resource.is_null() {
            continue;
        }
        if let Err(result) = frame_resource(resource, device, record.resource_generation, extent) {
            return result;
        }
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fg12_streamline_loaded() -> u32 {
    use windows::core::w;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    u32::from(!unsafe { GetModuleHandleW(w!("sl.dlss_g.dll").as_ptr()) }.is_null())
}
