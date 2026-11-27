// SPDX-License-Identifier: MIT
use super::{bundles, contents, directory_argument, entry, root, utf8};
use crate::sys::*;
use rsf_test_support::{Window, frame_record, qpc, qpc_frequency, warp_swapchain, wide_os};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::{fs, path::Path, ptr};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPED;
use windows::core::{BOOL, IUnknown, Interface, PCWSTR, w};

const RAW: u32 = RSF_FIXTURE_DUMP_VIEW_RAW as u32;
const HDR: u32 = RSF_FIXTURE_DUMP_VIEW_HDR as u32;
const SCALAR: u32 = RSF_FIXTURE_DUMP_VIEW_SCALAR as u32;
const MOTION: u32 = RSF_FIXTURE_DUMP_VIEW_DECODED as u32;
const SOURCE_INPUT: u32 = RSF_FIXTURE_FRAME_SOURCE_INPUT as u32;
const SOURCE_RENDERED: u32 = RSF_FIXTURE_FRAME_SOURCE_RENDERED as u32;

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn window() -> Option<Window> {
    Window::new(w!("RSF_InputDumpTest"), w!(""), WS_OVERLAPPED, 32, 32).ok()
}

fn warp(window: &Window) -> Option<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain)> {
    let swap = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: 4,
            Height: 2,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferCount: 1,
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        OutputWindow: window.handle,
        Windowed: BOOL(1),
        ..Default::default()
    };
    [D3D11_CREATE_DEVICE_DEBUG, D3D11_CREATE_DEVICE_FLAG(0)]
        .into_iter()
        .find_map(|flags| warp_swapchain(flags, &swap).ok())
}

fn png(bundle: &Path, name: &str) -> Vec<u8> {
    let path = bundle.join(name);
    let wide = wide_os(&path);
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(
            &CLSID_WICImagingFactory,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
        .unwrap();
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnLoad,
            )
            .unwrap();
        let image = decoder.GetFrame(0).unwrap();
        let (mut width, mut height) = (0, 0);
        image.GetSize(&mut width, &mut height).unwrap();
        assert_eq!((width, height), (4, 2));
        let converter = factory.CreateFormatConverter().unwrap();
        converter
            .Initialize(
                &image,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .unwrap();
        let mut pixels = vec![0; 32];
        converter.CopyPixels(ptr::null(), 16, &mut pixels).unwrap();
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        pixels
    }
}

fn texture<T>(
    device: &ID3D11Device,
    format: DXGI_FORMAT,
    bytes: &[T],
    pitch: u32,
) -> ID3D11Texture2D {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 4,
        Height: 2,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.as_ptr().cast(),
        SysMemPitch: pitch,
        SysMemSlicePitch: 0,
    };
    let mut resource = None;
    unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut resource)) }.unwrap();
    resource.unwrap()
}

fn debug_messages(device: &ID3D11Device) {
    let Ok(info) = device.cast::<ID3D11InfoQueue>() else {
        return;
    };
    for index in 0..unsafe { info.GetNumStoredMessages() } {
        let mut length = 0;
        unsafe { info.GetMessage(index, None, &mut length) }.unwrap();
        let mut storage = vec![0usize; length.div_ceil(size_of::<usize>())];
        let message = storage.as_mut_ptr().cast::<D3D11_MESSAGE>();
        unsafe { info.GetMessage(index, Some(message), &mut length) }.unwrap();
        let message = unsafe { &*message };
        assert!(
            message.Severity != D3D11_MESSAGE_SEVERITY_ERROR
                && message.Severity != D3D11_MESSAGE_SEVERITY_CORRUPTION,
            "{}",
            unsafe { CStr::from_ptr(message.pDescription.cast()) }.to_string_lossy()
        );
    }
}

fn clocks(bundle: &Path, pass: &rsf_game_render_pass) {
    let mut record = rsf_frame_record {
        frame_time_ms: 16.67,
        ..frame_record(9, 101, pass.view_key as u32, [4, 2], [4, 2])
    };
    let mut fg = rsf_fg_frame {
        struct_size: size_of::<rsf_fg_frame>() as u32,
        interpolate: 1,
        motion_scale_x: 1.0,
        motion_scale_y: 1.0,
        ..Default::default()
    };
    unsafe {
        record.input_qpc = qpc();
        record.source_provenance = SOURCE_INPUT;
        fg.record = &record;
        rsf_input_dump_fg_prepare(pass, &fg, 0);
        let recorded = contents(bundle, "fg_constants.json");
        assert!(recorded.contains(&format!("\"input_qpc\":{},", record.input_qpc)));
        for key in ["source_provenance", "source_qpc", "source_qpc_frequency"] {
            assert!(recorded.contains(&format!("\"{key}\":0,")));
        }
        record.input_qpc = 0;
        record.source_provenance = SOURCE_RENDERED;
        record.source_qpc = qpc();
        record.source_qpc_frequency = qpc_frequency();
        fg.record = &record;
        rsf_input_dump_fg_prepare(pass, &fg, 0);
        let rendered = contents(bundle, "fg_constants.json");
        for (key, value) in [
            ("input_qpc", 0),
            ("source_provenance", 1),
            ("source_qpc", record.source_qpc),
            ("source_qpc_frequency", record.source_qpc_frequency),
        ] {
            assert!(rendered.contains(&format!("\"{key}\":{value},")));
        }
    }
}

fn legacy(
    root: &Path,
    device: &ID3D11Device,
    immediate: &ID3D11DeviceContext,
    chain: &IDXGISwapChain,
    color: &ID3D11Texture2D,
    ui: &ID3D11Texture2D,
    hdr_bytes: &[u16],
) {
    let z = texture(
        device,
        DXGI_FORMAT_R24G8_TYPELESS,
        &[0u32, 1, 0x7fffff, 0xffffff, 0, 100, 1000, 10000],
        16,
    );
    let vectors = texture(
        device,
        DXGI_FORMAT_R32G32_FLOAT,
        &[
            0.0f32, 0.0, 1.0, -1.0, -1.0, 1.0, -1000.0, -1000.0, 0.0, 0.0, 2.0, 3.0, 4.0, 5.0, 6.0,
            7.0,
        ],
        32,
    );
    let mask = texture(
        device,
        DXGI_FORMAT_R8_UNORM,
        &[0u8, 255, 64, 128, 0, 255, 64, 128],
        4,
    );
    let ctx = immediate.as_raw();
    let chain_raw = chain.as_raw();
    let mut pass = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        role: RSF_GAME_RENDER_SR,
        session_id: 9,
        source_frame_id: 101,
        native_frame: 33,
        view_key: 11,
        family_key: 12,
        scope_id: 55,
        viewport_key: 13,
        flags: RSF_GAME_RENDER_PRIMARY,
        render_rect: [0, 0, 4, 2],
        output_rect: [0, 0, 4, 2],
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        rsf_input_dump_request();
        rsf_input_dump_pass(ctx, &pass, 1);
        assert_ne!(rsf_input_dump_active(), 0);
        let mut predicate = None;
        let query = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_OCCLUSION_PREDICATE,
            MiscFlags: 0,
        };
        device
            .CreatePredicate(&query, Some(&mut predicate))
            .unwrap();
        let predicate: ID3D11Predicate = predicate.unwrap();
        immediate.Begin(&predicate);
        immediate.End(&predicate);
        immediate.SetPredication(&predicate, true);
        rsf_input_dump_texture(
            ctx,
            c"sr_source_jittered".as_ptr(),
            c"fixture HDR".as_ptr(),
            color.as_raw(),
            HDR,
            1.0,
        );
        let (mut restored, mut value) = (None, BOOL(0));
        immediate.GetPredication(Some(&mut restored), Some(&mut value));
        assert!(restored.as_ref() == Some(&predicate) && value == BOOL(1));
        immediate.SetPredication(None::<&ID3D11Predicate>, false);
        for (name, desc, texture, view) in [
            (c"sr_reactive_mask", c"fixture mask", &mask, SCALAR),
            (c"sr_output", c"fixture output", color, HDR),
            (c"sr_game_motion", c"fixture game vectors", &vectors, MOTION),
            (
                c"sr_motion_decoded",
                c"fixture sparse motion",
                &vectors,
                MOTION,
            ),
        ] {
            rsf_input_dump_texture(
                ctx,
                name.as_ptr(),
                desc.as_ptr(),
                texture.as_raw(),
                view,
                1.0,
            );
        }
        let frame = rsf_dlss_frame {
            struct_size: size_of::<rsf_dlss_frame>() as u32,
            color_in: color.as_raw(),
            depth: z.as_raw(),
            motion: vectors.as_raw(),
            render_width: 4,
            output_width: 4,
            render_height: 2,
            output_height: 2,
            far_plane: f32::INFINITY,
            jitter_x: 0.25,
            jitter_y: -0.125,
            motion_scale_x: 0.25,
            motion_scale_y: 0.5,
            camera_motion_included: 1,
            depth_inverted: 1,
            ..Default::default()
        };
        rsf_input_dump_sr(ctx, &frame, 1);
        rsf_input_dump_fg(ctx, &pass, &frame);
        rsf_input_dump_sr_result(0);
        rsf_input_dump_pass(ctx, &pass, 0);
        assert_eq!(rsf_input_dump_active(), 0);
        pass.role = RSF_GAME_RENDER_UI_COMPOSITE;
        pass.color_input = ui.as_raw();
        pass.color_output = ui.as_raw();
        pass.ui_input = ui.as_raw();
        let wrong = rsf_game_render_pass {
            source_frame_id: 102,
            ..pass
        };
        rsf_input_dump_pass(ctx, &wrong, 1);
        rsf_input_dump_pass(ctx, &wrong, 0);
        let bundle = bundles(root).next().unwrap();
        assert!(!bundle.join("hud_color.bin").exists());
        rsf_input_dump_pass(ctx, &pass, 1);
        rsf_input_dump_pass(ctx, &pass, 0);
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
        pass.scene_surface = ui.as_raw();
        rsf_input_dump_pass(ctx, &pass, 0);
        let backbuffer: ID3D11Texture2D = chain.GetBuffer(0).unwrap();
        immediate.CopyResource(&backbuffer, ui);
        pass.role = RSF_GAME_RENDER_WINDOW;
        pass.window_key = 14;
        pass.rhi_viewport_key = 15;
        pass.swapchain = chain_raw;
        let wrong = rsf_game_render_pass {
            source_frame_id: 102,
            ..pass
        };
        assert_ne!(rsf_native_window_scope(&wrong, 1), 0);
        rsf_input_dump_present(chain_raw, ctx, &wrong);
        rsf_native_window_scope(&wrong, 0);
        assert!(!bundle.join("final_color.bin").exists());
        assert_ne!(rsf_native_window_scope(&pass, 1), 0);
        rsf_input_dump_present(chain_raw, ctx, &pass);
        clocks(&bundle, &pass);
        rsf_input_dump_presented(&pass, 0);
        rsf_native_window_scope(&pass, 0);
        let manifest = contents(&bundle, "manifest.json");
        assert!(
            manifest.contains("\"status\":\"complete\"")
                && manifest.contains("\"status\":\"unavailable\"")
        );
        assert!(contents(&bundle, "sr_constants.json").contains("\"far_plane\":null"));
        assert!(contents(&bundle, "fg_constants.json").contains("\"source_frame_id\":101"));
        let expected: Vec<_> = hdr_bytes
            .iter()
            .copied()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            fs::read(bundle.join("sr_source_jittered.bin")).unwrap(),
            expected
        );
        let color = png(&bundle, "sr_source_jittered.png");
        assert!(
            (187..=189).contains(&color[0])
                && (212..=214).contains(&color[1])
                && (155..=157).contains(&color[2])
        );
        let alpha = png(&bundle, "hud_alpha.png");
        assert_eq!([alpha[0], alpha[4], alpha[8]], [0, 128, 255]);
        assert_eq!(png(&bundle, "sr_reactive_mask.png")[4..7], [255; 3]);
        let depth = png(&bundle, "sr_depth.png");
        assert!(depth[0] == 0 && depth[12] == 255 && depth[8] > depth[4]);
        let motion = png(&bundle, "sr_motion_decoded.png");
        assert_eq!(
            [motion[0], motion[1], motion[12], motion[14]],
            [128, 128, 0, 220]
        );
        assert_eq!(png(&bundle, "sr_motion.png")[12..15], [0, 0, 128]);
        for file in ["hud_color.png", "fg_scene_color.png", "final_color.png"] {
            png(&bundle, file);
        }
        println!("Website fixture bundle: {}", bundle.display());
    }
}

#[path = "d3d11/record.rs"]
mod record;

fn run(path: &Path) -> i32 {
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return 77;
    }
    let _apartment = Apartment;
    let Some(window) = window() else { return 77 };
    let Some((device, context, chain)) = warp(&window) else {
        return 77;
    };
    let hdr: [u16; 32] = [
        0x3c00, 0x4000, 0x3800, 0, 0, 0, 0, 0, 0x4400, 0, 0, 0, 0, 0x4400, 0, 0, 0, 0, 0x4400, 0,
        0x3c00, 0x3c00, 0x3c00, 0, 0x7c00, 0x7e00, 0, 0, 0, 0, 0, 0,
    ];
    let color = texture(&device, DXGI_FORMAT_R16G16B16A16_FLOAT, &hdr, 32);
    let ui = texture(
        &device,
        DXGI_FORMAT_R8G8B8A8_UNORM,
        &[
            0x000000ffu32,
            0x8000ff00,
            0xffff0000,
            0xffffffff,
            0,
            0,
            0,
            0,
        ],
        16,
    );
    let root = root(path, "input-dump").join("preview-Ω");
    unsafe {
        rsf_input_dump_configure(
            utf8(&root).as_ptr(),
            c"AC7".as_ptr(),
            c"fixture".as_ptr(),
            None,
            ptr::null_mut(),
        )
    };
    legacy(&root, &device, &context, &chain, &color, &ui, &hdr);
    record::inspect(&root, context.as_raw(), ui.as_raw());
    let options = rsf_texture_dump_options {
        struct_size: size_of::<rsf_texture_dump_options>() as u32,
        abi_version: RSF_TEXTURE_DUMP_ABI_VERSION,
        output_prefix_utf8: c"unused".as_ptr(),
        view: RAW,
        scale: 1.0,
        ..Default::default()
    };
    let mut deferred = None;
    unsafe {
        device
            .CreateDeferredContext(0, Some(&mut deferred))
            .unwrap();
        assert_eq!(
            rsf_dump_texture_png(
                device.as_raw(),
                deferred.unwrap().as_raw(),
                color.as_raw(),
                &options,
                ptr::null_mut()
            ),
            RSF_FIXTURE_TEXTURE_INVALID
        );
        rsf_native_window_reset();
        context.ClearState();
    }
    debug_messages(&device);
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_input_dump_d3d11(argc: c_int, argv: *const *const c_char) -> i32 {
    entry(unsafe { directory_argument(argc, argv) }, run)
}
