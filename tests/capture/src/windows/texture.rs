// SPDX-License-Identifier: MIT
// Dump known motion and mask textures through the native ABI and inspect the files.
use super::{entry, utf8};
use crate::sys::*;
use std::ffi::{CStr, c_char, c_void};
use std::{fs, path::Path, ptr};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;
use windows::core::Interface;

const SCALE: f32 = 0.499 * 0.5;
const BIAS: f32 = 32767.0 / 65535.0;

#[derive(Default)]
struct Log {
    expected: *mut c_void,
    lines: Vec<String>,
    mismatched: bool,
}

unsafe extern "C" fn collect(user: *mut c_void, message: *const c_char) {
    if user.is_null() || message.is_null() {
        return;
    }
    let log = unsafe { &mut *user.cast::<Log>() };
    log.mismatched |= log.expected != user;
    log.lines.push(
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned(),
    );
}

fn device(driver: D3D_DRIVER_TYPE) -> Option<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .ok()?;
    Some((device?, context?))
}

fn texture(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
    source: Option<&D3D11_SUBRESOURCE_DATA>,
) -> Option<ID3D11Texture2D> {
    let mut texture = None;
    unsafe { device.CreateTexture2D(desc, source.map(ptr::from_ref), Some(&mut texture)) }.ok()?;
    texture
}

fn near(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected}"
    );
}

fn run(path: &Path) -> i32 {
    let Some((device, context)) =
        device(D3D_DRIVER_TYPE_HARDWARE).or_else(|| device(D3D_DRIVER_TYPE_WARP))
    else {
        println!("no D3D11 device available, skipping");
        return 0;
    };
    let encode = |v: f32| (((v * SCALE + BIAS) * 65535.0) + 0.5) as u16;
    let pixels = [
        0,
        0,
        encode(0.0),
        encode(0.0),
        encode(0.5),
        encode(0.25),
        encode(-0.5),
        encode(-1.0),
    ];
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 4,
        Height: 1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R16G16_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: pixels.as_ptr().cast(),
        SysMemPitch: 16,
        SysMemSlicePitch: 0,
    };
    let source = texture(&device, &desc, Some(&initial)).expect("create velocity texture");
    let prefix = utf8(&path.join("velocity"));
    let mut log = Log::default();
    let user = ptr::from_mut(&mut log).cast();
    log.expected = user;
    let mut options = rsf_texture_dump_motion_options {
        base: rsf_texture_dump_options {
            struct_size: size_of::<rsf_texture_dump_motion_options>() as u32,
            abi_version: RSF_TEXTURE_DUMP_MOTION_ABI_VERSION,
            output_prefix_utf8: prefix.as_ptr(),
            view: RSF_FIXTURE_DUMP_VIEW_ENCODED as u32,
            log: Some(collect),
            log_user: user,
            ..Default::default()
        },
        motion_scale: [1.0 / SCALE; 2],
        motion_bias: [BIAS; 2],
        zero_x_unwritten: 1,
    };
    let mut report = rsf_texture_dump_report {
        struct_size: size_of::<rsf_texture_dump_report>() as u32,
        ..Default::default()
    };
    let dump = |source: &ID3D11Texture2D,
                options: &rsf_texture_dump_motion_options,
                report: &mut rsf_texture_dump_report| {
        unsafe {
            rsf_dump_texture(
                device.as_raw(),
                context.as_raw(),
                source.as_raw(),
                ptr::from_ref(options).cast(),
                report,
            )
        }
    };
    options.base.abi_version += 1;
    assert_eq!(
        dump(&source, &options, &mut report),
        RSF_FIXTURE_TEXTURE_ABI_MISMATCH
    );
    options.base.abi_version -= 1;
    assert_eq!(
        unsafe {
            rsf_dump_texture(
                ptr::null_mut(),
                context.as_raw(),
                source.as_raw(),
                ptr::from_ref(&options).cast(),
                &mut report,
            )
        },
        RSF_FIXTURE_TEXTURE_INVALID
    );
    assert_eq!(dump(&source, &options, &mut report), 0);
    assert_eq!((report.width, report.height), (4, 1));
    near(report.fraction_unwritten, 0.25, 0.001);
    for (actual, expected) in [
        (report.min_x, -0.5),
        (report.max_x, 0.5),
        (report.min_y, -1.0),
        (report.max_y, 0.25),
    ] {
        near(actual, expected, 0.01);
    }
    assert!(!log.mismatched);
    for step in ["creating staging copy", "mapping", "writing", "done"] {
        assert!(
            log.lines.iter().any(|line| line.contains(step)),
            "missing progress message: {step}"
        );
    }
    let targa = fs::read(path.join("velocity.tga")).unwrap();
    assert!(targa.len() >= 18 && targa[2] == 2 && targa[16] == 32);

    let mask_bytes = [0u8, 255, 64, 128, 255, 0, 17, 33];
    let mask_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: mask_bytes.as_ptr().cast(),
        SysMemPitch: 8,
        SysMemSlicePitch: 0,
    };
    let mask = texture(
        &device,
        &D3D11_TEXTURE2D_DESC {
            Format: DXGI_FORMAT_R8G8_UNORM,
            ..desc
        },
        Some(&mask_data),
    )
    .expect("create two-channel mask");
    let mask_prefix = utf8(&path.join("numeric_mask"));
    options.base.output_prefix_utf8 = mask_prefix.as_ptr();
    options.base.view = RSF_FIXTURE_DUMP_VIEW_RAW as u32;
    assert_eq!(dump(&mask, &options, &mut report), 0);
    assert_eq!(
        (report.format, report.width, report.fraction_unwritten),
        (DXGI_FORMAT_R8G8_UNORM.0 as u32, 4, 0.0)
    );
    for (actual, expected) in [
        (report.min_x, 0.0),
        (report.max_x, 1.0),
        (report.min_y, 0.0),
        (report.max_y, 1.0),
    ] {
        near(actual, expected, 1e-6);
    }
    assert_eq!(
        unsafe {
            rsf_dump_texture_bytes(
                device.as_raw(),
                context.as_raw(),
                mask.as_raw(),
                ptr::from_ref(&options).cast(),
            )
        },
        0
    );
    assert_eq!(fs::read(path.join("numeric_mask.bin")).unwrap(), mask_bytes);
    options.base.output_prefix_utf8 = prefix.as_ptr();
    options.base.view = RSF_FIXTURE_DUMP_VIEW_ENCODED as u32;
    let bytes = [0u8; 16];
    let unsupported_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.as_ptr().cast(),
        ..initial
    };
    if let Some(unsupported) = texture(
        &device,
        &D3D11_TEXTURE2D_DESC {
            Format: DXGI_FORMAT_R32_UINT,
            ..desc
        },
        Some(&unsupported_data),
    ) {
        assert_eq!(
            dump(&unsupported, &options, &mut report),
            RSF_FIXTURE_TEXTURE_UNSUPPORTED
        );
    }
    let mipped = D3D11_TEXTURE2D_DESC {
        Width: 8,
        Height: 8,
        MipLevels: 4,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET).0 as u32,
        ..desc
    };
    if let Some(mipped) = texture(&device, &mipped, None) {
        let mip_prefix = utf8(&path.join("mipped"));
        options.base.output_prefix_utf8 = mip_prefix.as_ptr();
        assert_eq!(dump(&mipped, &options, &mut report), 0);
        assert_eq!((report.width, report.height), (8, 8));
    }
    options.base.output_prefix_utf8 = prefix.as_ptr();
    if let Some((other_device, _)) = self::device(D3D_DRIVER_TYPE_WARP)
        && let Some(foreign) = texture(&other_device, &desc, Some(&initial))
    {
        assert_eq!(
            dump(&foreign, &options, &mut report),
            RSF_FIXTURE_TEXTURE_FOREIGN
        );
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_texture_dump(path: *const c_char) -> i32 {
    entry(path, run)
}
