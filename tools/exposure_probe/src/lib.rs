// SPDX-License-Identifier: MIT
//! Frozen-input precision experiment.
#![cfg(all(windows, not(test)))]

mod abi;

use abi::{RSF_DLSS_ABI_VERSION, rsf_dlss_frame as Frame};
use std::ffi::{c_char, c_void};
use std::fs::File;
use std::io::Read;
use std::ptr::{self, null_mut};
use std::slice;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory, IDXGIAdapter, IDXGIFactory};
use windows::Win32::System::Threading::Sleep;
use windows::core::Interface;

type Handle = *mut c_void;
type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

const OUTPUT: [u32; 2] = [1600, 900];
const NVIDIA: u32 = 0x10de;
const PERFORMANCE: u32 = 3;

unsafe extern "C" {
    fn strtoul(text: *const c_char, end: *mut *mut c_char, base: i32) -> u32;
    fn strtof(text: *const c_char, end: *mut *mut c_char) -> f32;

    fn rsf_probe_capture_open(prefix: *const c_char) -> i32;
    fn rsf_probe_capture_start(device: Handle);
    fn rsf_probe_capture_end(device: Handle) -> i32;
    fn rsf_probe_load(interposer: *const c_char, device: Handle) -> i32;
    fn rsf_probe_shutdown();
    fn rsf_probe_dump(
        device: Handle,
        context: Handle,
        texture: Handle,
        prefix: *const c_char,
    ) -> i32;
    fn rsf_dlss_evaluate(context: Handle, frame: *const Frame) -> i32;

    fn rsf_colour_transport_create(device: Handle, pass: *mut Handle) -> i32;
    fn rsf_colour_transport_encode(
        pass: Handle,
        context: Handle,
        source: Handle,
        exposure: Handle,
        width: u32,
        height: u32,
        encoded: *mut Handle,
    ) -> i32;
    fn rsf_colour_transport_decode(
        pass: Handle,
        context: Handle,
        target: Handle,
        exposure: Handle,
        width: u32,
        height: u32,
        scene: Handle,
        scene_width: u32,
        scene_height: u32,
        jitter: *const f32,
    ) -> i32;
    fn rsf_colour_transport_destroy(pass: Handle);
    fn rsf_colour_fidelity_create(device: Handle, pass: *mut Handle) -> i32;
    fn rsf_colour_fidelity_run(
        pass: Handle,
        context: Handle,
        source: Handle,
        output: Handle,
        depth: Handle,
        inverted: u32,
        jitter: *const f32,
    ) -> i32;
    fn rsf_colour_fidelity_destroy(pass: Handle);
}

struct Session;

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { rsf_probe_shutdown() };
    }
}

struct Inputs {
    render: [u32; 2],
    quality: u32,
    input_format: DXGI_FORMAT,
    output_format: DXGI_FORMAT,
    pixel_bytes: u32,
    exposure: f32,
    pixels: Vec<u8>,
}

impl Inputs {
    fn parse(args: &[*const c_char]) -> ProbeResult<Self> {
        let number = |index: usize, default| match args.get(index) {
            Some(&text) => unsafe { strtoul(text, null_mut(), 10) },
            None => default,
        };
        let render = [number(8, 800), number(9, 452)];
        if render[0] == 0 || render[1] == 0 || render[0] > OUTPUT[0] || render[1] > OUTPUT[1] {
            return Err("render size must fit the 1600x900 output".into());
        }
        let input_format = DXGI_FORMAT(number(6, DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32) as i32);
        let pixel_bytes = match input_format {
            DXGI_FORMAT_R11G11B10_FLOAT => 4,
            DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
            DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
            _ => return Err("unsupported input format".into()),
        };
        let output_format = DXGI_FORMAT(number(7, DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32) as i32);
        if !matches!(
            output_format,
            DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R32G32B32A32_FLOAT
        ) {
            return Err("unsupported output format".into());
        }
        let input = std::env::args_os().nth(2).ok_or("missing input path")?;
        let mut pixels = vec![0; (render[0] * render[1] * pixel_bytes) as usize];
        File::open(input)?.read_exact(&mut pixels)?;
        Ok(Self {
            render,
            quality: number(5, PERFORMANCE),
            input_format,
            output_format,
            pixel_bytes,
            exposure: unsafe { strtof(args[4], null_mut()) },
            pixels,
        })
    }
}

fn native_device() -> ProbeResult<(ID3D11Device, ID3D11DeviceContext)> {
    let factory: IDXGIFactory = unsafe { CreateDXGIFactory() }?;
    let mut index = 0;
    let adapter: IDXGIAdapter = loop {
        let adapter = unsafe { factory.EnumAdapters(index) }?;
        if unsafe { adapter.GetDesc() }?.VendorId == NVIDIA {
            break adapter;
        }
        index += 1;
    };
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            Default::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    Ok((
        device.ok_or("null D3D11 device")?,
        context.ok_or("null immediate context")?,
    ))
}

fn texture(
    device: &ID3D11Device,
    size: [u32; 2],
    format: DXGI_FORMAT,
    unordered: bool,
    data: Option<&D3D11_SUBRESOURCE_DATA>,
) -> ProbeResult<ID3D11Texture2D> {
    let mut bind = D3D11_BIND_SHADER_RESOURCE;
    if unordered {
        bind |= D3D11_BIND_UNORDERED_ACCESS;
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size[0],
        Height: size[1],
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: bind.0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, data.map(ptr::from_ref), Some(&mut texture)) }?;
    texture.ok_or_else(|| "null probe texture".into())
}

struct Surfaces {
    color: ID3D11Texture2D,
    output: ID3D11Texture2D,
    depth: ID3D11Texture2D,
    motion: ID3D11Texture2D,
    exposure: Option<ID3D11Texture2D>,
}

impl Surfaces {
    fn create(device: &ID3D11Device, inputs: &Inputs) -> ProbeResult<Self> {
        let init = |data, pitch| D3D11_SUBRESOURCE_DATA {
            pSysMem: data,
            SysMemPitch: pitch,
            SysMemSlicePitch: 0,
        };
        let render = inputs.render;
        let color = texture(
            device,
            render,
            inputs.input_format,
            false,
            Some(&init(
                inputs.pixels.as_ptr().cast(),
                render[0] * inputs.pixel_bytes,
            )),
        )?;
        let output = texture(device, OUTPUT, inputs.output_format, true, None)?;
        let count = (render[0] * render[1]) as usize;
        let mut depths = vec![0.5_f32; count];
        if let Some(path) = std::env::var_os("RSF_PROBE_DEPTH_FILE").filter(|path| !path.is_empty())
        {
            let bytes = unsafe { slice::from_raw_parts_mut(depths.as_mut_ptr().cast(), count * 4) };
            File::open(path)?.read_exact(bytes)?;
        }
        // The original probe reuses the output descriptor's UAV flag for its remaining textures.
        let depth = texture(
            device,
            render,
            DXGI_FORMAT_R32_FLOAT,
            true,
            Some(&init(depths.as_ptr().cast(), render[0] * 4)),
        )?;
        let motions = vec![0_u32; count];
        let motion = texture(
            device,
            render,
            DXGI_FORMAT_R16G16_FLOAT,
            true,
            Some(&init(motions.as_ptr().cast(), render[0] * 4)),
        )?;
        let exposure = if inputs.exposure > 0.0 {
            Some(texture(
                device,
                [1, 1],
                DXGI_FORMAT_R32_FLOAT,
                true,
                Some(&init(ptr::from_ref(&inputs.exposure).cast(), 4)),
            )?)
        } else {
            None
        };
        Ok(Self {
            color,
            output,
            depth,
            motion,
            exposure,
        })
    }

    fn exposure(&self) -> Handle {
        self.exposure.as_ref().map_or(null_mut(), Interface::as_raw)
    }

    fn frame(&self, input: &Inputs) -> Frame {
        let mut frame = Frame {
            struct_size: size_of::<Frame>() as u32,
            abi_version: RSF_DLSS_ABI_VERSION,
            color_in: self.color.as_raw(),
            color_out: self.output.as_raw(),
            depth: self.depth.as_raw(),
            motion: self.motion.as_raw(),
            exposure: self.exposure(),
            render_width: input.render[0],
            render_height: input.render[1],
            output_width: OUTPUT[0],
            output_height: OUTPUT[1],
            quality: input.quality,
            motion_scale_x: 1.0,
            motion_scale_y: 1.0,
            camera_motion_included: 1,
            depth_inverted: 1,
            near_plane: 0.1,
            far_plane: 10000.0,
            vertical_fov: 1.0,
            aspect_ratio: 1600.0 / 900.0,
            camera_up: [0.0, 1.0, 0.0],
            camera_right: [1.0, 0.0, 0.0],
            camera_forward: [0.0, 0.0, 1.0],
            ..unsafe { std::mem::zeroed() }
        };
        for diagonal in [0, 5, 10, 15] {
            frame.camera_view_to_clip[diagonal] = 1.0;
            frame.clip_to_camera_view[diagonal] = 1.0;
            frame.clip_to_prev_clip[diagonal] = 1.0;
            frame.prev_clip_to_clip[diagonal] = 1.0;
        }
        frame
    }
}

fn enabled(variable: &str) -> bool {
    std::env::var_os(variable).is_some_and(|value| value.to_string_lossy().starts_with('1'))
}

struct Timer {
    disjoint: ID3D11Query,
    begin: ID3D11Query,
    end: ID3D11Query,
}

impl Timer {
    fn create(device: &ID3D11Device) -> Option<Self> {
        let query = |kind| {
            let mut result = None;
            unsafe {
                device.CreateQuery(
                    &D3D11_QUERY_DESC {
                        Query: kind,
                        MiscFlags: 0,
                    },
                    Some(&mut result),
                )
            }
            .ok()?;
            result
        };
        Some(Self {
            disjoint: query(D3D11_QUERY_TIMESTAMP_DISJOINT)?,
            begin: query(D3D11_QUERY_TIMESTAMP)?,
            end: query(D3D11_QUERY_TIMESTAMP)?,
        })
    }

    fn start(&self, context: &ID3D11DeviceContext) {
        unsafe {
            context.Begin(&self.disjoint);
            context.End(&self.begin);
        }
    }

    fn finish(&self, context: &ID3D11DeviceContext) {
        unsafe {
            context.End(&self.end);
            context.End(&self.disjoint);
            context.Flush();
        }
        let get = |query: &ID3D11Query, data: Handle, bytes: u32| unsafe {
            (context.vtable().GetData)(context.as_raw(), query.as_raw(), data, bytes, 0).0
        };
        let mut times = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
        for _ in 0..1000 {
            if get(
                &self.disjoint,
                ptr::from_mut(&mut times).cast(),
                size_of_val(&times) as u32,
            ) != 1
            {
                break;
            }
            unsafe { Sleep(1) };
        }
        let (mut first, mut last) = (0_u64, 0_u64);
        if times.Frequency != 0
            && !times.Disjoint.as_bool()
            && get(&self.begin, ptr::from_mut(&mut first).cast(), 8) == 0
            && get(&self.end, ptr::from_mut(&mut last).cast(), 8) == 0
        {
            eprintln!(
                "colour correction GPU time {:.4} ms",
                last.wrapping_sub(first) as f64 * 1000.0 / times.Frequency as f64
            );
        }
    }
}

fn apply_fidelity(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    surfaces: &Surfaces,
) -> bool {
    let timing = Timer::create(device);
    let mut pass = null_mut();
    let made = unsafe { rsf_colour_fidelity_create(device.as_raw(), &mut pass) } != 0;
    if let Some(timing) = &timing {
        timing.start(context);
    }
    let drawn = unsafe {
        rsf_colour_fidelity_run(
            pass,
            context.as_raw(),
            surfaces.color.as_raw(),
            surfaces.output.as_raw(),
            surfaces.depth.as_raw(),
            1,
            [0.0; 2].as_ptr(),
        )
    } != 0;
    if let Some(timing) = timing {
        timing.finish(context);
    }
    unsafe { rsf_colour_fidelity_destroy(pass) };
    made && drawn
}

fn run(args: &[*const c_char]) -> ProbeResult<bool> {
    let inputs = Inputs::parse(args)?;
    let prefix = args[3];
    if unsafe { rsf_probe_capture_open(prefix) } == 0 {
        return Err("could not initialize RenderDoc".into());
    }
    let (device, context) = native_device()?;
    let dev = device.as_raw();
    let ctx = context.as_raw();
    if unsafe { rsf_probe_load(args[1], dev) } == 0 {
        return Err("could not initialize Streamline or set the device".into());
    }
    let session = Session;
    let surfaces = Surfaces::create(&device, &inputs)?;
    // Shut down Streamline before retiring the resources it was given.
    let _session = session;
    let mut frame = surfaces.frame(&inputs);
    let [width, height] = inputs.render;

    let mut ok = true;
    let mut transport = null_mut();
    if enabled("RSF_PROBE_TRANSPORT") {
        let mut encoded = null_mut();
        ok = unsafe {
            rsf_colour_transport_create(dev, &mut transport) != 0
                && rsf_colour_transport_encode(
                    transport,
                    ctx,
                    surfaces.color.as_raw(),
                    surfaces.exposure(),
                    width,
                    height,
                    &mut encoded,
                ) != 0
        };
        frame.color_in = encoded;
        frame.color_encoded = 1;
        frame.exposure = null_mut();
        eprintln!("colour transport: encoded input, DLSS HDR input off");
    }
    if ok {
        for i in 0..60 {
            if i == 59 {
                unsafe { rsf_probe_capture_start(dev) };
            }
            frame.reset = u32::from(i == 0);
            frame.frame_index = i + 1;
            let result = unsafe { rsf_dlss_evaluate(ctx, &frame) };
            if result != 0 {
                eprintln!("evaluate refused {result}");
                ok = false;
                break;
            }
        }
    }
    ok &= unsafe { rsf_probe_capture_end(dev) } != 0;
    if !transport.is_null() {
        ok &= unsafe {
            rsf_colour_transport_decode(
                transport,
                ctx,
                surfaces.output.as_raw(),
                surfaces.exposure(),
                OUTPUT[0],
                OUTPUT[1],
                surfaces.color.as_raw(),
                width,
                height,
                [0.0; 2].as_ptr(),
            )
        } != 0;
        unsafe { rsf_colour_transport_destroy(transport) };
    }
    if enabled("RSF_PROBE_COLOUR_FIDELITY") {
        ok &= apply_fidelity(&device, &context, &surfaces);
    }
    ok &= unsafe { rsf_probe_dump(dev, ctx, surfaces.output.as_raw(), prefix) } == 0;
    Ok(ok)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_exposure_probe(argc: i32, argv: *const *const c_char) -> i32 {
    if !(5..=10).contains(&argc) || argc == 9 || argv.is_null() {
        println!(
            "usage: exposure-probe interposer input.bin output-prefix exposure (0=auto) [quality: 0=DLAA,1=Quality,3=Performance,4=Ultra] [input DXGI format: 26=R11,10=FP16,2=FP32] [output format:10=FP16,2=FP32] [render width height]"
        );
        return 1;
    }
    let args = unsafe { slice::from_raw_parts(argv, argc as usize) };
    match run(args) {
        Ok(ok) => i32::from(!ok),
        Err(error) => {
            eprintln!("exposure probe: {error}");
            1
        }
    }
}
