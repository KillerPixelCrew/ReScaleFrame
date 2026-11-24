// SPDX-License-Identifier: MIT
use super::{Handle, TestResult, check};
use crate::owned_win32_handle;
use crate::sys::rsf_sr12_setup as Setup;
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_game_render_pass as RenderPass};
use rsf_test_support::d3d12::{buffer, committed, copy_location, transition};
use std::ffi::{CString, c_char};
use std::fs;
use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::ptr;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

#[repr(C)]
struct Services {
    create: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    capture: unsafe extern "C" fn(Handle, *const c_char, u64, u32) -> i32,
    evaluate: unsafe extern "C" fn(Handle, Handle, *const RenderPass, u32) -> i32,
    flush: unsafe extern "C" fn(Handle, u32) -> i32,
    destroy: unsafe extern "C" fn(Handle),
}

struct Session<'a> {
    raw: Handle,
    api: &'a Services,
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.raw) };
    }
}

fn texture(
    device: &ID3D12Device,
    size: u32,
    format: DXGI_FORMAT,
    state: D3D12_RESOURCE_STATES,
) -> windows::core::Result<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(size),
        Height: size,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };
    committed(device, D3D12_HEAP_TYPE_DEFAULT, &desc, state)
}

fn record_upload(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    exposure: &ID3D12Resource,
    values: &[f32; 4],
) -> TestResult<ID3D12Resource> {
    let upload = buffer(device, 256, D3D12_HEAP_TYPE_UPLOAD)?;
    let mut mapped = ptr::null_mut();
    unsafe { upload.Map(0, Some(&D3D12_RANGE::default()), Some(&mut mapped)) }?;
    if mapped.is_null() {
        unsafe { upload.Unmap(0, None) };
        return Err("null mapped upload".into());
    }
    unsafe {
        ptr::copy_nonoverlapping(values.as_ptr(), mapped.cast(), values.len());
        upload.Unmap(0, None);
    }
    let source = copy_location(
        &upload,
        Some(D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
            Offset: 0,
            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                Width: 1,
                Height: 1,
                Depth: 1,
                RowPitch: 256,
            },
        }),
    );
    let target = copy_location(exposure, None);
    unsafe { list.CopyTextureRegion(&target, 0, 0, 0, &source, None) };
    transition(
        list,
        exposure,
        D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    );
    Ok(upload)
}

fn run(api: &Services) -> TestResult {
    let (_, device) = rsf_test_support::warp_d3d12()?;
    let queue_desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&queue_desc) }?;
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(queue_desc.Type) }?;
    let list: ID3D12GraphicsCommandList =
        unsafe { device.CreateCommandList(0, queue_desc.Type, &allocator, None) }?;
    let read = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    let color = texture(&device, 8, DXGI_FORMAT_R16G16B16A16_FLOAT, read)?;
    let depth = texture(&device, 8, DXGI_FORMAT_R32_FLOAT, read)?;
    let motion = texture(&device, 8, DXGI_FORMAT_R16G16_FLOAT, read)?;
    let exposure = texture(
        &device,
        1,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        D3D12_RESOURCE_STATE_COPY_DEST,
    )?;
    let values = [3.25, 17.0, 29.0, 43.0];
    let _upload = record_upload(&device, &list, &exposure, &values)?;

    // Zero-initialized setup selects the same optional providers/settings as the C fixture.
    let setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: 2,
        device: device.as_raw(),
        output_width: 8,
        output_height: 8,
        render_width: 8,
        render_height: 8,
        engine_input_extents: 1,
        external_exposure: 1,
        ..unsafe { std::mem::zeroed() }
    };
    let mut raw = ptr::null_mut();
    check(
        unsafe { (api.create)(&setup, &mut raw) } == 0 && !raw.is_null(),
        "create D3D12 SR session",
    )?;
    let session = Session { raw, api };
    let temp = std::env::temp_dir();
    let directory = temp.join(format!("rsf-exposure-{}", std::process::id()));
    fs::create_dir(&directory)?;
    let capture_path = CString::new(directory.to_string_lossy().as_bytes())?;
    check(
        unsafe { (api.capture)(session.raw, capture_path.as_ptr(), 1, 1) } == 0,
        "request exposure capture",
    )?;

    let mut pass: RenderPass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<RenderPass>() as u32;
    pass.session_id = 7;
    pass.native_frame = 1;
    pass.source_frame_id = 1;
    pass.history_key = 1;
    pass.resource_generation = 1;
    pass.camera_valid = 1;
    pass.color_input = color.as_raw();
    pass.depth = depth.as_raw();
    pass.motion = motion.as_raw();
    pass.exposure = exposure.as_raw();
    pass.motion_to_uv = [-0.125; 2];
    pass.camera.struct_size = size_of_val(&pass.camera) as u32;
    pass.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    pass.camera.render_width = 8;
    pass.camera.render_height = 8;
    pass.camera.output_width = 8;
    pass.camera.output_height = 8;
    check(
        unsafe { (api.evaluate)(session.raw, list.as_raw(), &pass, 0) } == 0,
        "evaluate D3D12 SR",
    )?;
    let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
    let event = unsafe { owned_win32_handle(CreateEventW(ptr::null(), 0, 0, ptr::null())) }
        .ok_or("create fence completion event")?;
    unsafe {
        list.Close()?;
        queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>()?)]);
        queue.Signal(&fence, 1)?;
        fence.SetEventOnCompletion(1, HANDLE(event.as_raw_handle()))?;
        check(
            WaitForSingleObject(event.as_raw_handle(), 10_000) == WAIT_OBJECT_0,
            "WARP fence wait",
        )?;
    }
    check(unsafe { (api.flush)(session.raw, 0) } == 0, "flush capture")?;
    drop(session);
    let mut found = false;
    for file in fs::read_dir(&directory)? {
        let file = file?;
        if file.path().extension().is_some_and(|ext| ext == "raw")
            && file.file_name().to_string_lossy().contains("sr-exposure")
        {
            let mut bytes = [0; 4];
            fs::File::open(file.path())?.read_exact(&mut bytes)?;
            check(
                (f32::from_le_bytes(bytes) - values[0]).abs() <= 1e-6,
                "first RGBA32F component must survive normalization",
            )?;
            found = true;
        }
    }
    check(found, "no raw exposure capture found")?;
    check(
        directory.canonicalize()? == temp.canonicalize()?.join(directory.file_name().unwrap()),
        "fixture directory ownership",
    )?;
    fs::remove_dir_all(directory)?;
    Ok(())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_exposure_normalization_fixture(api: *const Services) -> i32 {
    let Some(api) = (unsafe { api.as_ref() }) else {
        return 1;
    };
    match run(api) {
        Ok(()) => {
            println!(
                "External RGBA32F exposure normalized to R32F: GPU readback preserves the declared first component."
            );
            0
        }
        Err(error) => {
            eprintln!("exposure_normalization FAILED: {error}");
            1
        }
    }
}
