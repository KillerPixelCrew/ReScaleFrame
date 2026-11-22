// SPDX-License-Identifier: MIT
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_CPU_EVENT_VERSION, rsf_camera_frame, rsf_game_cpu_event,
    rsf_game_host_services, rsf_game_render_pass,
};
use std::ffi::c_void;
use std::ptr::{copy_nonoverlapping, null_mut};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::core::Interface;

#[path = "d3d11_host_device_abi.rs"]
mod abi;
#[path = "d3d11_host_device_fg.rs"]
mod fg;
#[path = "d3d11_host_device_recovery.rs"]
mod recovery;
#[path = "d3d11_host_device_sr.rs"]
mod sr;
use abi::*;

type Raw = *mut c_void;
const SCENE_COLOUR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

fn identity() -> [f32; 16] {
    let mut matrix = [0.0; 16];
    for diagonal in [0, 5, 10, 15] {
        matrix[diagonal] = 1.0;
    }
    matrix
}

fn native_pass(
    resources: &rsf_d3d11_fixture_resources,
    session_id: u64,
    source: u64,
    render_width: u32,
    render_height: u32,
) -> rsf_game_render_pass {
    let width = resources.width;
    let height = resources.height;
    let mut camera = rsf_camera_frame {
        struct_size: size_of::<rsf_camera_frame>() as u32,
        abi_version: RSF_GAME_CAMERA_ABI_VERSION,
        view_to_clip: identity(),
        clip_to_view: identity(),
        view_to_world: identity(),
        world_to_view: identity(),
        clip_to_previous_clip: identity(),
        jitter_pixels: [0.0; 2],
        previous_jitter_pixels: [0.0; 2],
        render_width,
        render_height,
        output_width: width,
        output_height: height,
        near_plane: 0.1,
        far_plane: 1000.0,
        vertical_fov_radians: 1.2,
        depth_inverted: 1,
        motion_jittered: 0,
        frame_time_seconds: 1.0 / 60.0,
    };
    camera.view_to_clip[0] = height as f32 / width as f32;
    camera.clip_to_view[0] = width as f32 / height as f32;
    rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        session_id,
        source_frame_id: source,
        submission_id: source,
        scope_id: source,
        family_key: 1,
        view_key: 1,
        history_key: 1,
        viewport_key: 1,
        window_key: 1,
        rhi_viewport_key: 1,
        native_frame: source,
        swapchain: resources.chain,
        color_input: resources.colour,
        color_output: resources.output,
        depth: resources.depth,
        motion: resources.motion,
        camera_valid: 1,
        motion_camera_included: 1,
        motion_to_uv: [-1.0; 2],
        previous_clip_to_clip: identity(),
        render_rect: [0, 0, render_width as i32, render_height as i32],
        output_rect: [0, 0, width as i32, height as i32],
        camera,
        ..unsafe { std::mem::zeroed() }
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_run_phase(
    phase: u32,
    scoped: u32,
    frame: rsf_d3d11_fixture_frame_fn,
    user: *mut c_void,
) {
    let Some(frame) = frame else { return };
    let count = match phase {
        RSF_D3D11_FIXTURE_REDUCED_WARMUP | RSF_D3D11_FIXTURE_LATE_ENABLE => 10,
        RSF_D3D11_FIXTURE_NATIVE_SR_OFF | RSF_D3D11_FIXTURE_NATIVE_SR_ON => 12,
        RSF_D3D11_FIXTURE_FG_OFF => 3,
        _ => return,
    };
    for index in 0..count {
        let first = index == 0;
        let (reconstruct, reset, expect_capture, cleanup) = match phase {
            RSF_D3D11_FIXTURE_REDUCED_WARMUP => (true, first, 0, scoped != 0 && index == 9),
            RSF_D3D11_FIXTURE_LATE_ENABLE => (true, false, if index < 2 { -1 } else { 1 }, false),
            RSF_D3D11_FIXTURE_NATIVE_SR_OFF => (false, first, if first { -1 } else { 1 }, false),
            RSF_D3D11_FIXTURE_NATIVE_SR_ON => (true, first, 1, false),
            RSF_D3D11_FIXTURE_FG_OFF => (false, false, 0, false),
            _ => unreachable!(),
        };
        if unsafe {
            frame(
                user,
                reconstruct.into(),
                reset.into(),
                expect_capture,
                cleanup.into(),
            )
        } != 0
        {
            break;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_texture(
    device: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    bindings: u32,
) -> *mut c_void {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device).unwrap() };
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT(format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bindings,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    if unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.is_err() {
        return null_mut();
    }
    texture.map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_read_texture(
    device: *mut c_void,
    context: *mut c_void,
    source: *mut c_void,
    stride: u32,
    bytes: *mut u8,
    length: usize,
) -> i32 {
    i32::from(unsafe {
        read_texture(
            device,
            context,
            source,
            stride,
            |source, pitch, width, height| {
                let row_bytes = width * stride as usize;
                if row_bytes.checked_mul(height) != Some(length) || length == 0 || bytes.is_null() {
                    return false;
                }
                for y in 0..height {
                    copy_nonoverlapping(source.add(y * pitch), bytes.add(y * row_bytes), row_bytes);
                }
                true
            },
        )
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_read_pixel(
    device: *mut c_void,
    context: *mut c_void,
    source: *mut c_void,
    x: u32,
    y: u32,
    stride: u32,
    pixel: *mut c_void,
) -> i32 {
    let (x, y) = (x as usize, y as usize);
    i32::from(unsafe {
        read_texture(
            device,
            context,
            source,
            stride,
            |source, pitch, width, height| {
                if x >= width || y >= height || pixel.is_null() {
                    return false;
                }
                let stride = stride as usize;
                copy_nonoverlapping(source.add(y * pitch + x * stride), pixel.cast(), stride);
                true
            },
        )
    })
}

unsafe fn read_texture(
    device: *mut c_void,
    context: *mut c_void,
    source: *mut c_void,
    stride: u32,
    read: impl FnOnce(*const u8, usize, usize, usize) -> bool,
) -> bool {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device).unwrap() };
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context).unwrap() };
    let source = unsafe { ID3D11Texture2D::from_raw_borrowed(&source).unwrap() };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    let (width, height) = (desc.Width as usize, desc.Height as usize);
    if width == 0 || height == 0 || stride == 0 {
        return false;
    }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.MiscFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    if unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.is_err() {
        return false;
    }
    let Some(staging) = staging else {
        return false;
    };
    unsafe { context.CopyResource(&staging, source) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.is_err() {
        return false;
    }
    let pitch = mapped.RowPitch as usize;
    let valid = !mapped.pData.is_null()
        && pitch >= width * stride as usize
        && read(mapped.pData.cast(), pitch, width, height);
    unsafe { context.Unmap(&staging, 0) };
    valid
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_host_fixture_cpu(
    host: *const rsf_game_host_services,
    source: u64,
    stage: u32,
    render_expected: u32,
) {
    let host = unsafe { &*host };
    let mut now = 0;
    let mut frequency = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut now) };
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    let event = rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        stage,
        session_id: host.session_id,
        source_frame_id: source,
        timestamp_qpc: now as u64,
        qpc_frequency: frequency as u64,
        render_expected,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { host.cpu_event.unwrap()(host.user, &event) };
}
