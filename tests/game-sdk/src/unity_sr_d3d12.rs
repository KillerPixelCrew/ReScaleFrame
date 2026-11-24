// SPDX-License-Identifier: MIT
mod gpu;
mod math;
mod rdr2;
mod streamline;

use crate::sys::*;
use gpu::{COPY_SOURCE, Gpu, SHADER_READ};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_FG_INPUTS,
    RSF_GAME_RENDER_SR, rsf_camera_frame, rsf_game_host_services, rsf_game_render_pass,
    rsf_render_host_setup,
};
use rsf_graphics::dxgi_format::half_to_f32;
use rsf_test_support::d3d12::transition;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};
use windows::{
    Win32::Graphics::{Direct3D12::*, Dxgi::Common::*},
    core::Interface,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const ENGINE_WIDTH: u32 = 426;
const ENGINE_HEIGHT: u32 = 240;

fn check(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn enabled(key: &str) -> bool {
    std::env::var_os(key).is_some()
}

unsafe extern "C" {
    // games/rdr2/src/runtime_setup.h, implemented by the linked RDR2 adapter.
    fn rsf_rdr2_fill_runtime_setup(
        ini: *const u16,
        root: *const u16,
        setup: *mut rsf_render_host_setup,
    );
}

unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        eprintln!("{}", unsafe { CStr::from_ptr(message) }.to_string_lossy());
    }
}

struct Inputs {
    color: ID3D12Resource,
    depth: ID3D12Resource,
    motion: ID3D12Resource,
    output: ID3D12Resource,
    exposure: Option<ID3D12Resource>,
    // D3D12 command recording does not retain the upload resources.
    _uploads: Vec<ID3D12Resource>,
}

impl Inputs {
    fn new(gpu: &Gpu, width: u32, height: u32) -> Result<Self> {
        let color = gpu.texture(
            width,
            height,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            D3D12_RESOURCE_FLAG_NONE,
        )?;
        let depth = gpu.texture(
            width,
            height,
            DXGI_FORMAT_R32_FLOAT,
            D3D12_RESOURCE_FLAG_NONE,
        )?;
        let motion = gpu.texture(
            width,
            height,
            DXGI_FORMAT_R16G16_FLOAT,
            D3D12_RESOURCE_FLAG_NONE,
        )?;
        let output = gpu.texture(
            WIDTH,
            HEIGHT,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        )?;
        // Exact little-endian binary16 for 0.25, 0.5, 0.75, 1.0.
        let mut uploads = vec![
            gpu.upload(&color, &[0x00, 0x34, 0x00, 0x38, 0x00, 0x3a, 0x00, 0x3c])?,
            gpu.upload(&depth, &0.5_f32.to_le_bytes())?,
            gpu.upload(&motion, &[0; 4])?,
        ];
        let exposure = if enabled("RSF_SR_EXTERNAL_EXPOSURE") {
            let image = gpu.texture(1, 1, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_FLAG_NONE)?;
            uploads.push(gpu.upload(&image, &1.0_f32.to_le_bytes())?);
            eprintln!("Fixture supplies external 1x1 R32_FLOAT exposure=1.");
            Some(image)
        } else {
            None
        };
        Ok(Self {
            color,
            depth,
            motion,
            output,
            exposure,
            _uploads: uploads,
        })
    }

    fn pass(
        &self,
        gpu: &Gpu,
        width: u32,
        height: u32,
        backend: u32,
    ) -> Result<rsf_game_render_pass> {
        // The SDK record consists of numeric fields and borrowed pointers.
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
        pass.role = RSF_GAME_RENDER_SR;
        pass.camera_valid = 1;
        pass.session_id = 7;
        pass.view_key = 11;
        pass.history_key = 11;
        pass.native_frame = 1;
        pass.resource_generation = 1;
        pass.color_input = self.color.as_raw();
        pass.depth = self.depth.as_raw();
        pass.motion = self.motion.as_raw();
        pass.color_output = self.output.as_raw();
        pass.exposure = self
            .exposure
            .as_ref()
            .map_or(ptr::null_mut(), Interface::as_raw);
        pass.motion_to_uv = [1.0; 2];
        pass.camera.struct_size = size_of::<rsf_camera_frame>() as u32;
        pass.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        pass.camera.render_width = width;
        pass.camera.render_height = height;
        pass.camera.output_width = WIDTH;
        pass.camera.output_height = HEIGHT;
        pass.camera.near_plane = math::NEAR;
        pass.camera.far_plane = math::FAR;
        pass.camera.vertical_fov_radians = math::VERTICAL_FOV;
        pass.camera.depth_inverted = 1;
        pass.camera.frame_time_seconds = 1.0 / 60.0;
        math::set_matrices(&mut pass)?;
        if backend == 0 {
            transition(&gpu.list, &self.color, SHADER_READ, COPY_SOURCE);
            unsafe { gpu.list.CopyResource(&self.output, &self.color) };
            transition(&gpu.list, &self.color, COPY_SOURCE, SHADER_READ);
            pass.color_output = ptr::null_mut();
            pass.role = RSF_GAME_RENDER_FG_INPUTS;
        }
        Ok(pass)
    }
}

struct Args {
    backend: u32,
    streamline: CString,
    fidelityfx: CString,
    xess: CString,
    vendor: Option<u32>,
    shared: bool,
}

impl Args {
    fn parse(args: &[std::ffi::OsString]) -> Option<Self> {
        let [backend, streamline, fidelityfx, xess, rest @ ..] = args else {
            return None;
        };
        if rest.len() > 2 {
            return None;
        }
        let vendor = match rest.first() {
            Some(text) => {
                let text = text.to_str()?;
                let text = text.strip_prefix('+').unwrap_or(text);
                Some(
                    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                        u32::from_str_radix(hex, 16).ok()?
                    } else if text.starts_with('0') {
                        u32::from_str_radix(text, 8).ok()?
                    } else {
                        text.parse().ok()?
                    },
                )
            }
            None => None,
        };
        Some(Self {
            backend: backend.to_str()?.parse().ok()?,
            streamline: CString::new(streamline.to_str()?).ok()?,
            fidelityfx: CString::new(fidelityfx.to_str()?).ok()?,
            xess: CString::new(xess.to_str()?).ok()?,
            vendor,
            shared: rest.len() == 2,
        })
    }

    fn setup(&self, gpu: &Gpu, interposer: &CStr, preset: u32) -> rsf_sr12_setup {
        let engine_extents = enabled("RSF_SR_ENGINE_EXTENTS");
        let engine = RSF_FIXTURE_UNITY_ENGINE as u32;
        rsf_sr12_setup {
            struct_size: size_of::<rsf_sr12_setup>() as u32,
            abi_version: if engine_extents { 2 } else { 1 },
            device: gpu.device.as_raw(),
            backend: self.backend,
            quality: 1,
            output_width: WIDTH,
            output_height: HEIGHT,
            inverted_depth: 1,
            fsr2_directory_utf8: self.fidelityfx.as_ptr(),
            fsr3_directory_utf8: self.fidelityfx.as_ptr(),
            fsr4_directory_utf8: self.fidelityfx.as_ptr(),
            xess_directory_utf8: self.xess.as_ptr(),
            dlss: rsf_dlss_setup {
                struct_size: size_of::<rsf_dlss_setup>() as u32,
                abi_version: RSF_DLSS_ABI_VERSION,
                interposer_path_utf8: interposer.as_ptr(),
                plugin_directory_utf8: self.streamline.as_ptr(),
                engine,
                engine_version_utf8: RSF_UNITY_ENGINE_VERSION.as_ptr().cast(),
                project_id_utf8: RSF_UNITY_PROJECT_ID.as_ptr().cast(),
                require_signature: 1,
                log: Some(log),
                preset,
                ..Default::default()
            },
            log: Some(log),
            render_width: if engine_extents { ENGINE_WIDTH } else { 0 },
            render_height: if engine_extents { ENGINE_HEIGHT } else { 0 },
            engine_input_extents: u32::from(engine_extents),
            ..Default::default()
        }
    }
}

struct Shared(*mut rsf_streamline_host);

impl Shared {
    fn adopt(args: &Args, gpu: &Gpu, setup: &rsf_sr12_setup) -> Result<Self> {
        let registration = rsf_streamline_host_setup {
            struct_size: size_of::<rsf_streamline_host_setup>() as u32,
            abi_version: RSF_STREAMLINE_HOST_ABI_VERSION,
            runtime_directory_utf8: args.streamline.as_ptr(),
            dxgi_adapter: gpu
                .adapter
                .as_ref()
                .map_or(ptr::null_mut(), Interface::as_raw),
            engine_type: setup.dlss.engine,
            engine_version_utf8: setup.dlss.engine_version_utf8,
            project_id_utf8: setup.dlss.project_id_utf8,
            require_signature: 1,
            development_runtime: 0,
            log: Some(log),
            log_user: ptr::null_mut(),
            profile: RSF_SL_PROFILE_DLSS_FG,
            load_dlss_sr: 1,
        };
        let mut raw = ptr::null_mut();
        check(
            unsafe {
                rsf_streamline_host_adopt(
                    &registration,
                    gpu.device.as_raw(),
                    gpu.queue.as_raw(),
                    &mut raw,
                ) == 0
                    && !raw.is_null()
            },
            "Streamline host adoption failed",
        )?;
        let shared = Self(raw);
        check(
            unsafe {
                rsf_streamline_host_acquire(raw, 1) == 0 && rsf_streamline_host_sleep(raw, 1) == 0
            },
            "Streamline source acquisition or pacing failed",
        )?;
        shared.markers([
            RSF_FIXTURE_UNITY_SIM_START,
            RSF_FIXTURE_UNITY_SIM_END,
            RSF_FIXTURE_UNITY_SUBMIT_START,
        ])?;
        Ok(shared)
    }

    fn markers(&self, markers: [i32; 3]) -> Result<()> {
        for marker in markers {
            check(
                unsafe { rsf_streamline_host_marker(self.0, 1, marker as u32, 0) } == 0,
                "Streamline marker refused",
            )?;
        }
        Ok(())
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        unsafe { rsf_streamline_host_destroy(self.0) };
    }
}

struct SuperResolution(*mut rsf_sr12);
impl Drop for SuperResolution {
    fn drop(&mut self) {
        unsafe { rsf_sr12_destroy(self.0) };
    }
}

fn abi_probe() -> Result<()> {
    const {
        assert!(size_of::<rsf_camera_frame>() == 384);
        assert!(size_of::<rsf_unity_packet>() == 520);
        assert!(size_of::<rsf_unity_native_api>() == 72);
    }
    let mut invalid = ptr::without_provenance_mut(1);
    check(
        unsafe {
            rsf_sr12_create(ptr::null(), &mut invalid) == RSF_FIXTURE_UNITY_INVALID
                && invalid.is_null()
                && rsf_sr12_evaluate(ptr::null_mut(), ptr::null_mut(), ptr::null(), 0)
                    == RSF_FIXTURE_UNITY_INVALID
        },
        "Runtime SR12 null argument contract failed",
    )
}

fn run(args: &Args) -> Result<i32> {
    let Some(gpu) = Gpu::open(args.vendor)? else {
        return Ok(77);
    };
    let preset = if let Some(text) = std::env::var_os("RSF_DLSS_TEST_PRESET") {
        let Some(preset) = text.to_str().and_then(|value| value.parse().ok()) else {
            return Ok(2);
        };
        if unsafe { rsf_fixture_unity_dlss_preset_valid(preset) } == 0 {
            return Ok(2);
        }
        eprintln!("Requested DLSS model preset {preset}");
        preset
    } else {
        0
    };
    let mut interposer = args.streamline.as_bytes().to_vec();
    interposer.extend_from_slice(b"\\sl.interposer.dll");
    let interposer = CString::new(interposer)?;
    let mut setup = args.setup(&gpu, &interposer, preset);
    let shared = if args.shared {
        let shared = Shared::adopt(args, &gpu, &setup)?;
        setup.streamline_host = shared.0.cast();
        Some(shared)
    } else {
        None
    };
    let mut sr = SuperResolution(ptr::null_mut());
    // The SDK host record contains nullable callbacks, borrowed pointers and numeric fields.
    let mut host: rsf_game_host_services = unsafe { std::mem::zeroed() };
    let host_ini = std::env::var_os("RSF_RDR2_HOST_INI");
    let host_root = std::env::var_os("RSF_RDR2_HOST_ROOT");
    let host_ini = if let (Some(ini), Some(root)) = (host_ini, host_root) {
        let ini = crate::wide(&ini);
        let root = crate::wide(&root);
        if ini.len() > 32768 || root.len() > 32768 {
            return Ok(2);
        }
        Some((ini, root))
    } else {
        None
    };
    let created = if let Some((ini, root)) = &host_ini {
        host.struct_size = size_of::<rsf_game_host_services>() as u32;
        host.abi_version = RSF_GAME_ABI_VERSION;
        host.session_id = 7;
        host.log = Some(log);
        let mut profile = rsf_render_host_setup::default();
        unsafe {
            rsf_rdr2_fill_runtime_setup(ini.as_ptr(), root.as_ptr(), &mut profile);
            rsf_render_host_create(&profile, &mut host)
        }
    } else {
        unsafe { rsf_sr12_create(&setup, &mut sr.0) }
    };
    if created != 0 {
        eprintln!("Provider {} preparation refused {created}", args.backend);
        return Ok(77);
    }
    let (mut width, mut height) = (ENGINE_WIDTH, ENGINE_HEIGHT);
    if !sr.0.is_null() {
        unsafe { rsf_sr12_plan(sr.0, &mut width, &mut height) };
    }
    check(
        setup.abi_version != 2 || (width, height) == (setup.render_width, setup.render_height),
        "engine input extents changed",
    )?;
    let inputs = Inputs::new(&gpu, width, height)?;
    let mut pass = inputs.pass(&gpu, width, height, args.backend)?;
    if host.process_latency.is_some() && enabled("RSF_RDR2_COLD_FG") {
        rdr2::cold_generation(&gpu, &host, pass)?;
        return Ok(0);
    }
    let evaluated = unsafe {
        if let Some(upscale) = host.process_upscale {
            upscale(host.user, gpu.list.as_raw(), &pass)
        } else {
            rsf_sr12_evaluate(sr.0, gpu.list.as_raw(), &pass, 0)
        }
    };
    eprintln!(
        "Provider {}: {width}x{height} -> {WIDTH}x{HEIGHT}, evaluate={evaluated}",
        args.backend
    );
    if evaluated == 0 {
        rdr2::record_scene(&gpu, &host, &pass, &inputs.output);
        rdr2::fill_slots(&gpu, &host, &mut pass)?;
    }
    if let Some(shared) = &shared {
        // FG contracts on viewport zero independently of SR while borrowing the same token.
        let token = unsafe { rsf_streamline_host_token(shared.0, 1) };
        streamline::set_shared_constants(shared.0, token, &pass)?;
        eprintln!("PASS: independent SR/FG constants accepted on the same source token.");
    }

    let readback = gpu.readback(&inputs.output)?;
    gpu.copy_to_readback(&inputs.output, &readback);
    gpu.submit(&host, 1)?;
    gpu.wait(1)?;
    if evaluated == 0 {
        rdr2::reuse_slot(&gpu, &host, &mut pass, &inputs.output, &readback)?;
        rdr2::native_present(&gpu, &host)?;
        if args.backend == RSF_SR_DLSS
            && enabled("RSF_RDR2_HOST_SWITCHES")
            && let Some((ini, _)) = &host_ini
        {
            rdr2::live_switches(&gpu, &host, &mut pass, &inputs.output, &readback, ini)?;
        }
    }
    let [r, g, b] = readback.center_rgb()?.map(half_to_f32);
    eprintln!("GPU output center RGB {r:.4} {g:.4} {b:.4}");
    drop(sr);
    if let Some(shared) = shared {
        shared.markers([
            RSF_FIXTURE_UNITY_SUBMIT_END,
            RSF_FIXTURE_UNITY_PRESENT_START,
            RSF_FIXTURE_UNITY_PRESENT_END,
        ])?;
    }
    gpu.validate()?;
    check(
        evaluated == 0
            && [r, g, b].into_iter().all(f32::is_finite)
            && r > 0.1
            && g > 0.2
            && b > 0.3,
        "provider refused evaluation or GPU output did not meet RGB thresholds",
    )?;
    Ok(0)
}

// The C++ linker keeps its standard CRT startup and calls this C ABI main.
#[cfg_attr(not(test), unsafe(export_name = "main"))]
extern "C" fn fixture_main(_argc: i32, _argv: *mut *mut c_char) -> i32 {
    crate::run(|| {
        let check_fixture = || -> Result<i32> {
            abi_probe()?;
            let args: Vec<_> = std::env::args_os().skip(1).collect();
            if args.is_empty() {
                math::check_portable_math()?;
                println!("PASS: native ABI and null/short setup refusal.");
                println!("PASS: binary16 and reverse-Z matrix comparison.");
                return Ok(0);
            }
            let Some(args) = Args::parse(&args) else {
                eprintln!(
                    "Usage: test backend streamline-directory fidelityfx-directory xess-directory [vendor-id] [shared]"
                );
                return Ok(2);
            };
            run(&args)
        };
        match check_fixture() {
            Ok(code) => code,
            Err(error) => {
                eprintln!("D3D12 fixture failed: {error}");
                1
            }
        }
    })
}
