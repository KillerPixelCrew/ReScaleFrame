// SPDX-License-Identifier: MIT
//! Explicit hardware SDK fixture. The vendor root contains streamline, fidelityfx and xess.
use super::{Frame, Setup, Status};
use crate::d3d11_output::SavedBindings;
use crate::graphics_d3d11::{rsf_d3d11_state_restore, rsf_d3d11_state_save};
use crate::sdk_module::SdkModule;
use crate::sys::{RSF_FRAME_ASSEMBLY_ABI_VERSION, rsf_pipeline_camera_frame as Camera};
use rsf_game_sys::RSF_DLSS_PIPELINE_ABI_VERSION;
use std::ffi::{CStr, CString, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::Instant;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::{
    Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
    Direct3D11::*,
    Dxgi::{Common::*, CreateDXGIFactory, IDXGIFactory},
};
use windows::core::Interface;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

const DLSS: u32 = 1;
const FSR3: u32 = 3;
const FSR4: u32 = 4;
const XESS: u32 = 5;
const SWITCHES: [u32; 5] = [FSR4, XESS, FSR3, DLSS, FSR4];
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
static ORIGIN: OnceLock<Instant> = OnceLock::new();

struct Runtime {
    _dll: SdkModule,
    start: unsafe extern "C" fn(*mut c_void, *const Setup, u32) -> i32,
    frame: unsafe extern "C" fn(*mut c_void, *const Frame) -> i32,
    select: unsafe extern "C" fn(u32, *mut u32, *mut u32) -> i32,
    status: unsafe extern "C" fn(*mut Status) -> i32,
    stop: unsafe extern "C" fn() -> i32,
}

impl Runtime {
    fn open() -> Option<Self> {
        let path = std::env::current_exe()
            .ok()?
            .with_file_name("ReScaleFrame.Runtime.dll");
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let dll = SdkModule::adopt_system(unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        })?;
        macro_rules! import {
            ($name:expr) => {{
                let address = dll.symbol($name)?;
                // Each slot follows the matching public dlss_pipeline.h signature.
                // The target is the field the import fills, so it differs per use.
                #[allow(clippy::missing_transmute_annotations)]
                let function = unsafe { std::mem::transmute(address) };
                function
            }};
        }
        Some(Self {
            start: import!(c"rsf_dlss_pipeline_start_backend"),
            frame: import!(c"rsf_dlss_pipeline_on_frame"),
            select: import!(c"rsf_dlss_pipeline_select_backend"),
            status: import!(c"rsf_dlss_pipeline_get_status"),
            stop: import!(c"rsf_dlss_pipeline_stop"),
            _dll: dll,
        })
    }
}

macro_rules! check {
    ($failures:expr, $condition:expr) => {
        if !$condition {
            println!("FAIL {}:{}: {}", file!(), line!(), stringify!($condition));
            *$failures += 1;
        }
    };
}

unsafe extern "C" fn log_line(_: *mut c_void, message: *const c_char) {
    let message = unsafe { CStr::from_ptr(message) };
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64();
    eprintln!("[{elapsed:8.3}] {}", message.to_string_lossy());
}

fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

struct Rig {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    _textures: [ID3D11Texture2D; 3],
    _exposure: Option<ID3D11Texture2D>,
    camera: Camera,
    frame: Frame,
    serial: u32,
}

impl Rig {
    fn create(require_nvidia: bool, with_exposure: bool) -> Option<Self> {
        let factory = unsafe { CreateDXGIFactory::<IDXGIFactory>() }.ok()?;
        let mut index = 0;
        let adapter = loop {
            let adapter = unsafe { factory.EnumAdapters(index) }.ok()?;
            if !require_nvidia
                || unsafe { adapter.GetDesc() }.is_ok_and(|desc| desc.VendorId == 0x10de)
            {
                break adapter;
            }
            index += 1;
        };
        let (mut device, mut context) = (None, None);
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .ok()?;
        let (device, context) = (device?, context?);
        let texture = |format, width, height, bind, input| {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
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
            unsafe { device.CreateTexture2D(&desc, input, Some(&mut texture)) }.ok()?;
            texture
        };
        let make_input = |format, red| {
            let source = texture(
                format,
                WIDTH,
                HEIGHT,
                (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
                None,
            )?;
            let mut view = None;
            unsafe { device.CreateUnorderedAccessView(&source, None, Some(&mut view)) }.ok()?;
            unsafe { context.ClearUnorderedAccessViewFloat(&view?, &[red, 0.0, 0.0, 1.0]) };
            Some(source)
        };
        let textures = [
            make_input(DXGI_FORMAT_R16G16B16A16_FLOAT, 0.25)?,
            make_input(DXGI_FORMAT_R32_FLOAT, 0.5)?,
            make_input(DXGI_FORMAT_R16G16_FLOAT, 0.0)?,
        ];
        let exposure = if with_exposure {
            let one = 1.0_f32;
            let data = D3D11_SUBRESOURCE_DATA {
                pSysMem: (&raw const one).cast(),
                SysMemPitch: 4,
                SysMemSlicePitch: 0,
            };
            Some(texture(
                DXGI_FORMAT_R32_FLOAT,
                1,
                1,
                D3D11_BIND_SHADER_RESOURCE.0 as u32,
                Some(&raw const data),
            )?)
        } else {
            None
        };
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
        let mut frame: Frame = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<Frame>() as u32;
        frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
        frame.scene_color = textures[0].as_raw();
        frame.depth = textures[1].as_raw();
        frame.game_motion = textures[2].as_raw();
        frame.exposure = exposure.as_ref().map_or(null_mut(), Interface::as_raw);
        frame.render_width = WIDTH;
        frame.render_height = HEIGHT;
        Some(Self {
            device,
            context,
            _textures: textures,
            _exposure: exposure,
            camera,
            frame,
            serial: 0,
        })
    }

    fn run_frame(&mut self, runtime: &Runtime) -> i32 {
        self.camera.jitter_pixels[0] = if self.serial.is_multiple_of(2) {
            0.25
        } else {
            -0.25
        };
        self.serial += 1;
        let context = self.context.as_raw();
        // Restore engine bindings before the next flush and timing sample.
        let Some(_saved) =
            (unsafe { SavedBindings::new(context, rsf_d3d11_state_save, rsf_d3d11_state_restore) })
        else {
            return -7;
        };
        self.frame.camera = &raw const self.camera;
        unsafe { (runtime.frame)(context, &self.frame) }
    }

    fn present(&self) {
        unsafe { self.context.Flush() };
    }

    fn start(&self, runtime: &Runtime, root: &CStr, backend: u32) -> bool {
        let directory = |suffix: &[u8]| {
            let mut path = root.to_bytes().to_vec();
            path.extend_from_slice(suffix);
            CString::new(path).expect("C string root and literal suffix")
        };
        let streamline = directory(br"\streamline");
        let fidelityfx = directory(br"\fidelityfx");
        let xess = directory(br"\xess");
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<Setup>() as u32;
        setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
        setup.streamline_directory_utf8 = if backend == DLSS {
            streamline.as_ptr()
        } else {
            null()
        };
        setup.output_width = WIDTH;
        setup.output_height = HEIGHT;
        setup.motion.struct_size = size_of_val(&setup.motion) as u32;
        setup.motion.scale_x = 1.0;
        setup.motion.scale_y = 1.0;
        setup.motion.output_scale_x = 1.0;
        setup.motion.output_scale_y = 1.0;
        setup.motion.invalid_value = -1000.0;
        setup.motion.zero_means_unwritten = 1;
        // A zero quality and engine are native quality and custom engine.
        setup.engine_version_utf8 = c"ReScaleFrame synthetic fixture".as_ptr();
        setup.project_id_utf8 = c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr();
        setup.fsr2_directory_utf8 = fidelityfx.as_ptr();
        setup.fsr3_directory_utf8 = fidelityfx.as_ptr();
        setup.fsr4_directory_utf8 = fidelityfx.as_ptr();
        setup.xess_directory_utf8 = xess.as_ptr();
        setup.view_space_to_meters = 1.0;
        setup.log = Some(log_line);
        let result = unsafe { (runtime.start)(self.device.as_raw(), &setup, backend) };
        if result != 0 {
            eprintln!("pipeline start refused: {result}");
        }
        result == 0
    }
}

fn warm_up(rig: &mut Rig, runtime: &Runtime) -> bool {
    for i in 0..12 {
        let result = rig.run_frame(runtime);
        rig.present();
        if result != 0 {
            println!("warm-up frame {i} refused: {result}");
            return false;
        }
    }
    true
}

fn measure_switches(rig: &mut Rig, runtime: &Runtime, baseline: bool, failures: &mut u32) {
    check!(failures, warm_up(rig, runtime));
    let mut first_select = 0.0;
    for backend in SWITCHES {
        let start = Instant::now();
        let result = unsafe { (runtime.select)(backend, null_mut(), null_mut()) };
        let select_ms = milliseconds(start);
        if !baseline {
            check!(failures, result == 0);
        }
        let (mut first_ms, mut later_ms) = (0.0, 0.0_f64);
        if result == 0 {
            for i in 0..if baseline { 8 } else { 12 } {
                let start = Instant::now();
                let frame_result = rig.run_frame(runtime);
                if !baseline {
                    check!(failures, frame_result == 0);
                }
                rig.present();
                let elapsed = milliseconds(start);
                if i == 0 {
                    first_ms = elapsed;
                } else {
                    later_ms = later_ms.max(elapsed);
                }
                if baseline {
                    check!(failures, frame_result == 0);
                }
            }
        }
        if baseline {
            println!(
                "BASELINE select backend {backend} (sync): result {result}, render-thread stall {select_ms:.1} ms; first frame {first_ms:.1} ms, worst later frame {later_ms:.1} ms"
            );
        } else {
            println!(
                "SELECT backend {backend}: select {select_ms:.1} ms, first frame {first_ms:.1} ms, worst later frame {later_ms:.1} ms"
            );
            if first_select == 0.0 {
                first_select = select_ms;
            }
            check!(failures, first_ms < 250.0 && later_ms < 250.0);
        }
    }
    if !baseline {
        println!("SELECT first FSR4 select {first_select:.0} ms");
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_sr_backend_switch_fixture(argc: i32, argv: *const *const c_char) -> i32 {
    if argc < 3 {
        println!(
            "usage: sr_backend_switch_device <vendor root> baseline|select|select+exposure|fsr3-start"
        );
        return 77;
    }
    ORIGIN.get_or_init(Instant::now);
    let root = unsafe { CStr::from_ptr(*argv.add(1)) };
    let mode = unsafe { CStr::from_ptr(*argv.add(2)) }.to_bytes();
    let (mode, exposure) = match mode
        .strip_suffix(b"+exposure")
        .filter(|mode| !mode.is_empty())
    {
        Some(mode) => (mode, true),
        None => (mode, false),
    };
    let alternate_start = mode == b"fsr3-start";
    if !alternate_start && mode != b"baseline" && mode != b"select" {
        println!("unknown mode {}", String::from_utf8_lossy(mode));
        return 2;
    }
    let Some(mut rig) = Rig::create(!alternate_start, exposure) else {
        println!("SKIP: no matching D3D11 adapter");
        return 77;
    };
    let Some(runtime) = Runtime::open() else {
        eprintln!("could not load the Runtime DLL or its D3D11 SR exports");
        return 1;
    };
    if !rig.start(&runtime, root, if alternate_start { FSR3 } else { DLSS }) {
        return if alternate_start { 1 } else { 77 };
    }
    let mut failures = 0;
    let tally = &mut failures;
    if alternate_start {
        let mut pipeline: Status = unsafe { std::mem::zeroed() };
        pipeline.struct_size = size_of::<Status>() as u32;
        check!(tally, (unsafe { (runtime.status)(&mut pipeline) }) == 0);
        check!(
            tally,
            pipeline.backend == FSR3 && pipeline.requested_backend == FSR3
        );
        check!(tally, warm_up(&mut rig, &runtime));
    } else {
        measure_switches(&mut rig, &runtime, mode == b"baseline", tally);
    }
    check!(tally, (unsafe { (runtime.stop)() }) == 0);
    i32::from(failures != 0)
}
