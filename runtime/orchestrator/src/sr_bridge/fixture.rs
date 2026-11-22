// SPDX-License-Identifier: MIT
use super::*;
use std::ffi::{CStr, c_char};
use windows::Win32::Foundation::{E_FAIL, HMODULE};
use windows::Win32::Graphics::{
    Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN},
    Direct3D11::*,
    Dxgi::{Common::*, CreateDXGIFactory, IDXGIAdapter, IDXGIDevice, IDXGIFactory},
};
use windows::core::Interface;

const SIZE: u32 = 128;

unsafe extern "C" fn log(_: Handle, message: *const c_char) {
    eprintln!("{}", unsafe { CStr::from_ptr(message) }.to_string_lossy());
}

fn texture(
    device: &ID3D11Device,
    format: DXGI_FORMAT,
    bind: u32,
    width: u32,
    input: Option<&D3D11_SUBRESOURCE_DATA>,
) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: width,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: bind,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, input.map(ptr::from_ref), Some(&mut texture)) }?;
    texture.ok_or_else(|| E_FAIL.into())
}

fn output_matches(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    output: &ID3D11Texture2D,
) -> bool {
    let read = || -> windows::core::Result<f32> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { output.GetDesc(&mut desc) };
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }?;
        let staging = staging.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let half = unsafe {
            context.CopyResource(&staging, output);
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let offset = 64 * mapped.RowPitch as usize + 64 * 8;
            let half = mapped
                .pData
                .cast::<u8>()
                .add(offset)
                .cast::<u16>()
                .read_unaligned();
            context.Unmap(&staging, 0);
            half
        };
        Ok(f32::from(1024 + (half & 1023)) * 2.0f32.powi(i32::from((half >> 10) & 31) - 25))
    };
    read().is_ok_and(|red| {
        eprintln!("output center red: {red:.6} (expected 0.25)");
        red.is_finite() && (red - 0.25).abs() < 0.05
    })
}

fn evaluate(
    bridge: *mut Bridge,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
) -> windows::core::Result<bool> {
    let mut textures = Vec::with_capacity(4);
    for (format, red) in [
        (DXGI_FORMAT_R16G16B16A16_FLOAT, 0.25),
        (DXGI_FORMAT_R32_FLOAT, 0.5),
        (DXGI_FORMAT_R16G16_FLOAT, 0.0),
        (DXGI_FORMAT_R16G16B16A16_FLOAT, 0.25),
    ] {
        let texture = texture(
            device,
            format,
            (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
            SIZE,
            None,
        )?;
        let mut view = None;
        unsafe { device.CreateUnorderedAccessView(&texture, None, Some(&mut view)) }?;
        let view = view.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        unsafe { context.ClearUnorderedAccessViewFloat(&view, &[red, 0.0, 0.0, 1.0]) };
        textures.push(texture);
    }
    let exposure_value: f32 = 1.0;
    let exposure_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: (&raw const exposure_value).cast(),
        SysMemPitch: size_of::<f32>() as u32,
        SysMemSlicePitch: 0,
    };
    let exposure = texture(
        device,
        DXGI_FORMAT_R32_FLOAT,
        D3D11_BIND_SHADER_RESOURCE.0 as u32,
        1,
        Some(&exposure_data),
    )?;
    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    record.struct_size = size_of_val(&record) as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 1;
    record.resource_generation = 1;
    record.frame_time_ms = 16.67;
    record.output_width = SIZE;
    record.output_height = SIZE;
    record.camera.struct_size = size_of_val(&record.camera) as u32;
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    record.camera.near_plane = 0.1;
    record.camera.far_plane = 10_000.0;
    record.camera.vertical_fov_radians = 1.0;
    record.camera.depth_inverted = 1;
    let mut frame: rsf_sr_frame = unsafe { std::mem::zeroed() };
    frame.struct_size = size_of_val(&frame) as u32;
    frame.pre_exposure = 1.0;
    frame.view_space_to_meters = 1.0;
    frame.motion_scale_x = 1.0;
    frame.motion_scale_y = 1.0;
    let mut passed = true;
    for (backend, quality) in [(2, 0), (3, 1), (5, 0), (2, 3), (4, 0), (5, 4), (4, 4)] {
        let result = unsafe { rsf_sr_bridge_select(bridge, backend, quality, 0) };
        let mut status: rsf_sr_session_status = unsafe { std::mem::zeroed() };
        status.struct_size = size_of_val(&status) as u32;
        unsafe { rsf_sr_bridge_get_status(bridge, &mut status) };
        let version = unsafe { CStr::from_ptr(status.version_name.as_ptr()) };
        eprintln!(
            "select {backend}: {result} effective {} version {}",
            status.effective,
            version.to_string_lossy()
        );
        if backend == 4 && result == NOT_SUPPORTED {
            continue;
        }
        if result != OK {
            passed = false;
            continue;
        }
        record.render_width = status.render_width;
        record.render_height = status.render_height;
        record.resource_generation += 1;
        for (i, resource) in [
            &mut frame.color,
            &mut frame.depth,
            &mut frame.motion,
            &mut frame.output,
        ]
        .into_iter()
        .enumerate()
        {
            resource.struct_size = size_of_val(resource) as u32;
            resource.resource = textures[i].as_raw();
            resource.generation = record.resource_generation;
            (resource.width, resource.height) = if i == 3 {
                (SIZE, SIZE)
            } else {
                (record.render_width, record.render_height)
            };
        }
        // Reuse all command slots before switching SDK contexts again.
        for i in 0..12 {
            if i == 6 || i == 9 {
                passed &=
                    unsafe { rsf_sr_bridge_set_auto_exposure(bridge, u32::from(i == 9)) } == OK;
                frame.exposure = unsafe { std::mem::zeroed() };
                if i == 6 {
                    frame.exposure.struct_size = size_of_val(&frame.exposure) as u32;
                    frame.exposure.resource = exposure.as_raw();
                    frame.exposure.width = 1;
                    frame.exposure.height = 1;
                    frame.exposure.generation = record.resource_generation;
                }
            }
            record.frame_id += 1;
            frame.record = &raw const record;
            frame.jitter_x = match i {
                0 => -0.25,
                1 => 0.25,
                _ => 0.0,
            };
            let result = unsafe { rsf_sr_bridge_evaluate(bridge, context.as_raw(), &frame) };
            eprintln!("frame {}: {result}", record.frame_id);
            passed &= result == OK;
        }
        passed &= output_matches(device, context, &textures[3]);
    }
    Ok(passed)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_sr_bridge_fixture(argc: i32, argv: *const *const c_char) -> i32 {
    if argc != 3 && argc != 4 {
        println!(
            "provide FFX and XeSS runtime directories [adapter index] to run hardware evaluation"
        );
        return 77;
    }
    let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
    let selected_adapter = if argc == 4 {
        let index = unsafe { CStr::from_ptr(args[3]) }.to_string_lossy();
        let Ok(index) = index.parse() else {
            return 1;
        };
        let Ok(adapter) = (unsafe {
            CreateDXGIFactory::<IDXGIFactory>().and_then(|factory| factory.EnumAdapters(index))
        }) else {
            return 1;
        };
        Some(adapter)
    } else {
        None::<IDXGIAdapter>
    };
    let driver = if selected_adapter.is_some() {
        D3D_DRIVER_TYPE_UNKNOWN
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };
    let (mut device, mut context) = (None, None);
    if unsafe {
        D3D11CreateDevice(
            selected_adapter.as_ref(),
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .is_err()
    {
        return 77;
    }
    let (Some(device), Some(context)) = (device, context) else {
        return 77;
    };
    if let Ok(desc) = device
        .cast::<IDXGIDevice>()
        .and_then(|dxgi| unsafe { dxgi.GetAdapter()?.GetDesc() })
    {
        let length = desc
            .Description
            .iter()
            .position(|&ch| ch == 0)
            .unwrap_or(desc.Description.len());
        let description = String::from_utf16_lossy(&desc.Description[..length]);
        eprintln!(
            "adapter: {description} vendor {:x} device {:x}",
            desc.VendorId, desc.DeviceId
        );
    }
    let mut setup: rsf_sr_session_setup = unsafe { std::mem::zeroed() };
    setup.struct_size = size_of_val(&setup) as u32;
    setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
    setup.open.struct_size = size_of_val(&setup.open) as u32;
    setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.open.api = API_D3D11;
    setup.open.device = device.as_raw();
    setup.open.output_width = SIZE;
    setup.open.output_height = SIZE;
    setup.open.hdr = 1;
    setup.open.inverted_depth = 1;
    setup.open.auto_exposure = 1;
    setup.open.dynamic_resolution = 1;
    setup.open.log = Some(log);
    setup.fsr2_directory_utf8 = args[1];
    setup.fsr3_directory_utf8 = args[1];
    setup.fsr4_directory_utf8 = args[1];
    setup.xess_directory_utf8 = args[2];
    let mut bridge = ptr::null_mut();
    let result = unsafe { rsf_sr_bridge_create(&setup, &mut bridge) };
    eprintln!("bridge create: {result}");
    if result != OK {
        return 1;
    }
    let passed = evaluate(bridge, &device, &context).unwrap_or_else(|error| {
        eprintln!("bridge fixture failed: {error}");
        false
    });
    let retired = unsafe { rsf_sr_bridge_close(bridge) } == OK;
    i32::from(!(passed && retired))
}
