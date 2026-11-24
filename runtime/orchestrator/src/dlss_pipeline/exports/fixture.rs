// SPDX-License-Identifier: MIT
use super::*;
use crate::graphics_d3d11::{rsf_d3d11_state_restore, rsf_d3d11_state_save};
use crate::sdk_module::SdkModule;
use crate::sys::rsf_d3d11_state;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{Common::*, CreateDXGIFactory, IDXGIFactory};
use windows::core::Interface;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

use rsf_test_support::device::{Device, TestResult};

const SIZE: u32 = 512;

struct Runtime {
    _dll: SdkModule,
    start: unsafe extern "C" fn(Raw, *const Setup) -> i32,
    stop: unsafe extern "C" fn() -> i32,
    output: unsafe extern "C" fn() -> Raw,
    select: unsafe extern "C" fn(u32, *mut u32, *mut u32) -> i32,
    status: unsafe extern "C" fn(*mut Status) -> i32,
    frame: unsafe extern "C" fn(Raw, *const Frame) -> i32,
    sharpen: unsafe extern "C" fn(*const Sharpening) -> i32,
}

impl Runtime {
    fn open() -> TestResult<Self> {
        let path = std::env::current_exe()?.with_file_name("ReScaleFrame.Runtime.dll");
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let dll = SdkModule::adopt_system(unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        })
        .ok_or("could not load the Runtime DLL beside the fixture")?;
        macro_rules! import {
            ($name:expr) => {{
                let address = dll
                    .symbol($name)
                    .ok_or_else(|| format!("missing Runtime export {}", $name.to_string_lossy()))?;
                // The signature is the corresponding public declaration in dlss_pipeline.h.
                // The target is the field the import fills, so it differs per use.
                #[allow(clippy::missing_transmute_annotations)]
                let function = unsafe { std::mem::transmute(address) };
                function
            }};
        }
        Ok(Self {
            start: import!(c"rsf_dlss_pipeline_start"),
            stop: import!(c"rsf_dlss_pipeline_stop"),
            output: import!(c"rsf_dlss_pipeline_output_texture"),
            select: import!(c"rsf_dlss_pipeline_select_backend"),
            status: import!(c"rsf_dlss_pipeline_get_status"),
            frame: import!(c"rsf_dlss_pipeline_on_frame"),
            sharpen: import!(c"rsf_dlss_pipeline_set_sharpening"),
            _dll: dll,
        })
    }
}

unsafe extern "C" fn log(_: Raw, message: *const c_char) {
    eprintln!("{}", unsafe { CStr::from_ptr(message) }.to_string_lossy());
}

fn hardware() -> Option<Device> {
    let factory: IDXGIFactory = unsafe { CreateDXGIFactory() }.ok()?;
    let mut adapters = (0..).map_while(|i| unsafe { factory.EnumAdapters(i) }.ok());
    let adapter = adapters
        .find(|adapter| unsafe { adapter.GetDesc() }.is_ok_and(|desc| desc.VendorId == 0x10de))?;
    let (device, context) = rsf_test_support::d3d11_device_on(
        Some(&adapter),
        D3D_DRIVER_TYPE_UNKNOWN,
        D3D11_CREATE_DEVICE_FLAG(0),
        None,
    )
    .ok()?;
    Some(Device { device, context })
}

fn inputs(device: &Device) -> TestResult<[ID3D11Texture2D; 3]> {
    let bind = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    let make = |format, value| -> TestResult<_> {
        let texture = device.texture(&Device::desc([SIZE; 2], format, bind), None)?;
        let mut view = None;
        unsafe {
            device
                .device
                .CreateUnorderedAccessView(&texture, None, Some(&mut view))
        }?;
        let view = view.ok_or("missing input UAV")?;
        unsafe {
            device
                .context
                .ClearUnorderedAccessViewFloat(&view, &[value, 0.0, 0.0, 1.0])
        };
        Ok(texture)
    };
    Ok([
        make(DXGI_FORMAT_R16G16B16A16_FLOAT, 0.25)?,
        make(DXGI_FORMAT_R32_FLOAT, 0.5)?,
        make(DXGI_FORMAT_R16G16_FLOAT, 0.0)?,
    ])
}

fn camera() -> Camera {
    let mut camera: Camera = unsafe { std::mem::zeroed() };
    camera.struct_size = size_of::<Camera>() as u32;
    camera.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;
    camera.view_to_clip[0] = 2.0;
    camera.view_to_clip[5] = 2.0;
    camera.view_to_clip[11] = 1.0;
    camera.view_to_clip[14] = 0.1;
    camera.clip_to_view[0] = 0.5;
    camera.clip_to_view[5] = 0.5;
    camera.clip_to_view[11] = 10.0;
    camera.clip_to_view[14] = 1.0;
    for i in [0, 5, 10, 15] {
        camera.clip_to_prev_clip[i] = 1.0;
        camera.prev_clip_to_clip[i] = 1.0;
    }
    camera.camera_forward[2] = 1.0;
    camera.camera_up[1] = 1.0;
    camera.camera_right[0] = 1.0;
    camera.near_plane = 0.1;
    camera.vertical_fov = 1.0;
    camera.aspect_ratio = 1.0;
    camera.depth_inverted = 1;
    camera.has_jitter = 1;
    camera.jitter_pixels[0] = 0.25;
    camera
}

// The fixture generates only small positive normal binary16 values.
fn half(value: f32) -> u16 {
    let bits = value.to_bits();
    let rounded = bits + 0xfff + ((bits >> 13) & 1);
    ((rounded - (112 << 23)) >> 13) as u16
}

fn sharpening(
    runtime: &Runtime,
    device: &Device,
    inputs: &[ID3D11Texture2D; 3],
    frame: &Frame,
) -> TestResult<bool> {
    let context = device.context.as_raw();
    let mut source = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..SIZE {
        for x in 0..SIZE {
            let x = x as f32;
            let base = 0.1
                + 0.3 * x / SIZE as f32
                + if x >= 256.0 { 0.3 } else { 0.0 }
                + 0.01 * (x * 0.7).sin();
            source.extend((0..4).map(|c| {
                half(if c == 3 {
                    0.75
                } else {
                    base * (0.9 + c as f32 * 0.1)
                })
            }));
        }
    }
    unsafe {
        device
            .context
            .UpdateSubresource(&inputs[0], 0, None, source.as_ptr().cast(), SIZE * 8, 0)
    };
    let mut passed = true;
    let mut run = |options: &Sharpening| -> TestResult<Vec<u16>> {
        passed &= unsafe { (runtime.sharpen)(options) } == OK;
        passed &= unsafe { (runtime.frame)(context, frame) } == OK;
        let raw = unsafe { (runtime.output)() };
        let texture =
            unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or("missing SR output")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        unsafe { device.read(texture, &desc, 4) }
    };
    let mut options = sharpening_policy::defaults();
    let off = run(&options)?;
    options.mode = 1; // RCAS
    options.sharpness = 0.0;
    let identical = !off.is_empty() && run(&options)? == off;
    eprintln!("Sharpening Off versus zero strength (byte identical): {identical}");
    options.sharpness = 1.0;
    let mut changed = true;
    for mode in [1, 2, 3, 4] {
        options.mode = mode;
        let filtered = run(&options)?;
        let distinct = !filtered.is_empty() && filtered != off;
        eprintln!("Sharpening production mode {mode} edge: {distinct}");
        changed &= distinct;
    }
    options.mode = 0;
    passed &= unsafe { (runtime.sharpen)(&options) } == OK;
    Ok(passed && identical && changed)
}

fn evaluate(runtime: &Runtime, device: &Device) -> TestResult<bool> {
    let initial_output = unsafe { (runtime.output)() };
    if initial_output.is_null() {
        return Err("missing initial SR output".into());
    }
    let inputs = inputs(device)?;
    let context = device.context.as_raw();
    let mut camera = camera();
    let mut frame: Frame = unsafe { std::mem::zeroed() };
    frame.struct_size = size_of::<Frame>() as u32;
    frame.abi_version = ABI;
    frame.scene_color = inputs[0].as_raw();
    frame.depth = inputs[1].as_raw();
    frame.game_motion = inputs[2].as_raw();
    frame.render_width = SIZE;
    frame.render_height = SIZE;
    let mut passed = true;
    for backend in [1, 2, 3, 5, 4, 1] {
        let result = unsafe { (runtime.select)(backend, null_mut(), null_mut()) };
        eprintln!("select {backend}: {result}");
        let mut status: Status = unsafe { std::mem::zeroed() };
        status.struct_size = size_of::<Status>() as u32;
        let status_result = unsafe { (runtime.status)(&mut status) };
        // The compatibility pipeline maps provider refusals to its own result domain.
        if backend == 4
            && result == NOT_SUPPORTED
            && status_result == OK
            && status.last_switch_result == -7
        {
            continue;
        }
        if result != OK {
            passed = false;
            continue;
        }
        if status_result != OK
            || status.backend != backend
            || status.requested_backend != backend
            || [status.output_width, status.output_height] != [SIZE; 2]
            || unsafe { (runtime.output)() } != initial_output
        {
            eprintln!(
                "backend {backend} did not preserve the output lease or report its selection"
            );
            passed = false;
            continue;
        }
        for jitter in [0.25, -0.25, 0.25] {
            camera.jitter_pixels[0] = jitter;
            frame.camera = &camera;
            let mut bindings = rsf_d3d11_state::default();
            let result = unsafe {
                rsf_d3d11_state_save(context, &mut bindings);
                let result = (runtime.frame)(context, &frame);
                rsf_d3d11_state_restore(context, &mut bindings);
                result
            };
            eprintln!("evaluate {backend}: {result}");
            passed &= result == OK;
        }
    }
    if unsafe { (runtime.select)(3, null_mut(), null_mut()) } != OK {
        return Ok(false);
    }
    camera.reset = 1;
    camera.jitter_pixels[0] = 0.25;
    frame.camera = &camera;
    Ok(sharpening(runtime, device, &inputs, &frame)? && passed)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_sr_pipeline(argc: i32, argv: *const *const c_char) -> i32 {
    if argc != 4 || argv.is_null() {
        println!("provide Streamline, FFX and XeSS runtime directories for the hardware fixture");
        return 77;
    }
    let Some(device) = hardware() else {
        return 77;
    };
    let mut setup: Setup = unsafe { std::mem::zeroed() };
    setup.struct_size = size_of::<Setup>() as u32;
    setup.abi_version = ABI;
    setup.streamline_directory_utf8 = unsafe { *argv.add(1) };
    setup.output_width = SIZE;
    setup.output_height = SIZE;
    setup.motion.struct_size = size_of_val(&setup.motion) as u32;
    setup.motion.scale_x = 1.0;
    setup.motion.scale_y = 1.0;
    setup.motion.output_scale_x = 1.0;
    setup.motion.output_scale_y = 1.0;
    setup.motion.invalid_value = -1000.0;
    setup.motion.zero_means_unwritten = 1;
    setup.engine_version_utf8 = c"ReScaleFrame synthetic fixture".as_ptr();
    setup.project_id_utf8 = c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr();
    setup.fsr2_directory_utf8 = unsafe { *argv.add(2) };
    setup.fsr3_directory_utf8 = setup.fsr2_directory_utf8;
    setup.fsr4_directory_utf8 = setup.fsr2_directory_utf8;
    setup.xess_directory_utf8 = unsafe { *argv.add(3) };
    setup.view_space_to_meters = 1.0;
    setup.log = Some(log);
    let runtime = match Runtime::open() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let result = unsafe { (runtime.start)(device.device.as_raw(), &setup) };
    if result != OK {
        eprintln!("pipeline start refused: {result}");
        return 1;
    }
    let passed = evaluate(&runtime, &device).unwrap_or_else(|error| {
        eprintln!("SR pipeline fixture failed: {error}");
        false
    });
    i32::from(!(passed & (unsafe { (runtime.stop)() } == OK)))
}
