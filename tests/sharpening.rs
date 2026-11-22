// SPDX-License-Identifier: MIT
use super::{Device, Handle, TestResult, check, near, run_warp};
use crate::d3d11_sharpening::{
    Frame, rsf_core_sharpen11_count, rsf_core_sharpen11_destroy, rsf_core_sharpen11_run,
    rsf_sharpen11_pack,
};
use crate::sharpening_policy::defaults;
use crate::sys::rsf_sharpening_options as Options;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT,
};
use windows::core::Interface;

const WIDTH: usize = 33;
const HEIGHT: usize = 17;
const COUNT: usize = WIDTH * HEIGHT;
const SIZE: [u32; 2] = [WIDTH as u32, HEIGHT as u32];
const FILTERS: [u32; 3] = [1, 2, 3];
const DEPTH_FILTERS: [u32; 2] = [2, 3];

unsafe extern "C" {
    fn rsf_sharpen11_adaptive(request: &Options, base: f32, motion: f32) -> f32;
}

fn frame() -> Frame {
    Frame {
        depth_linear: 0,
        depth_inverted: 1,
        hdr: 1,
        near_plane: 0.0,
        far_plane: 0.0,
        exposure: 1.0,
        motion_x: 1.0,
        motion_y: 1.0,
    }
}

fn constants(request: Options, frame: &Frame, mode: u32, exposure: u32) -> [u32; 32] {
    let extent = [SIZE[0], SIZE[1], 11, 7, 19, 9];
    let mut words = [0; 32];
    unsafe {
        rsf_sharpen11_pack(
            &request,
            frame,
            extent.as_ptr(),
            exposure,
            mode,
            words.as_mut_ptr(),
        )
    };
    words
}

fn check_constants() -> TestResult {
    let options = defaults();
    check(
        options.mode == 0
            && options.sharpness == 0.4
            && options.mas == 0
            && options.motion_sharpness == 0.2
            && options.motion_threshold == 0.0
            && options.motion_scale_limit == 10.0
            && options.depth_scale == 35.0
            && options.depth_bias == 0.001
            && options.contrast_enabled == 0
            && options.contrast == 0.0
            && options.clamp_output == 0
            && options.debug == 0,
        "default sharpening options",
    )?;
    let mut mas = Options {
        mas: 1,
        motion_threshold: 2.0,
        ..options
    };
    let adaptive =
        |request: &Options, motion| unsafe { rsf_sharpen11_adaptive(request, 0.4, motion) };
    near(adaptive(&mas, 2.0), 0.4, 0.0, "MAS below threshold")?;
    near(adaptive(&mas, 6.0), 0.5, 1e-6, "MAS ramp")?;
    near(adaptive(&mas, 100.0), 0.6, 1e-6, "MAS ceiling")?;
    mas.motion_sharpness = -0.2;
    near(adaptive(&mas, 100.0), 0.2, 1e-6, "negative MAS")?;
    mas.motion_scale_limit = 1.0;
    near(adaptive(&mas, 100.0), 0.4, 0.0, "degenerate MAS ramp")?;

    let mut facts = frame();
    let invalid = Options {
        sharpness: f32::NAN,
        contrast_enabled: 1,
        contrast: -1.0,
        depth_bias: 2.0,
        ..options
    };
    let words = constants(invalid, &facts, 1, 0);
    check(
        [words[0], words[1], words[19]] == [0.4f32.to_bits(), 0, 1.0f32.to_bits()],
        "sanitize shader constants",
    )?;
    for mode in 0..=4 {
        let words = constants(options, &facts, mode, 0);
        let strength: f32 = if DEPTH_FILTERS.contains(&mode) {
            0.8
        } else {
            0.4
        };
        check(words[0] == strength.to_bits(), "sharpness mapping")?;
        check(
            [
                words[8], words[9], words[24], words[25], words[30], words[17],
            ] == [
                (11.0 / WIDTH as f32).to_bits(),
                (7.0 / HEIGHT as f32).to_bits(),
                (19.0 / WIDTH as f32).to_bits(),
                (9.0 / HEIGHT as f32).to_bits(),
                0,
                1,
            ],
            "frame mapping",
        )?;
    }
    facts.near_plane = 1.0;
    facts.far_plane = 100.0;
    let words = constants(options, &facts, 1, 0);
    check(
        words[30] == 1
            && words[20..23] == [100.0f32.to_bits(), 100.0f32.to_bits(), 99.0f32.to_bits()],
        "depth planes",
    )?;
    facts.exposure = 2.0;
    facts.motion_x = 3.0;
    facts.motion_y = 4.0;
    let all = Options {
        sharpness: 0.7,
        mas: 1,
        motion_sharpness: -0.3,
        motion_threshold: 2.0,
        motion_scale_limit: 11.0,
        contrast_enabled: 1,
        contrast: 1.2,
        depth_scale: 45.0,
        depth_bias: 0.002,
        clamp_output: 1,
        debug: 1,
        ..options
    };
    for mode in FILTERS {
        let words = constants(all, &facts, mode, 1);
        let strength: f32 = if mode == 1 { 0.7 } else { 1.4 };
        check(
            words[0] == strength.to_bits()
                && words[1] == 1.2f32.to_bits()
                && words[2] == 1
                && words[4] == if mode == 1 { 1 } else { 3 }
                && words[7] == (-0.3f32).to_bits()
                && words[10..14]
                    == [
                        3.0f32.to_bits(),
                        4.0f32.to_bits(),
                        2.0f32.to_bits(),
                        11.0f32.to_bits(),
                    ]
                && words[18..20] == [45.0f32.to_bits(), 0.002f32.to_bits()]
                && words[23] == u32::from(mode != 1)
                && words[28..30] == [1, 1]
                && words[31] == 2.0f32.to_bits(),
            "all options map to shader constants",
        )?;
    }
    Ok(())
}

struct Sharpen(Handle);

impl Sharpen {
    fn dispatches(&self) -> u64 {
        unsafe { rsf_core_sharpen11_count(self.0) }
    }

    fn run(
        &mut self,
        context: Handle,
        output: Handle,
        depth: Handle,
        options: Options,
        frame: &Frame,
    ) -> TestResult {
        let inputs = [output, ptr::null_mut(), depth, ptr::null_mut()];
        check(
            unsafe { rsf_core_sharpen11_run(&mut self.0, context, &inputs, &options, frame) } != 0,
            "sharpening dispatch refused",
        )
    }
}

impl Drop for Sharpen {
    fn drop(&mut self) {
        unsafe { rsf_core_sharpen11_destroy(self.0) };
    }
}

struct Fixture<'a> {
    warp: &'a Device,
    pass: Sharpen,
    depth: ID3D11Texture2D,
}

impl Fixture<'_> {
    fn run(
        &mut self,
        pixels: &[f32],
        mode: u32,
        sharpness: f32,
        clamp: bool,
        facts: &Frame,
    ) -> TestResult<(ID3D11Texture2D, Vec<f32>)> {
        let desc = Device::desc(
            SIZE,
            DXGI_FORMAT_R32G32B32A32_FLOAT,
            D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS,
        );
        let output = self.warp.initialized(&desc, pixels, SIZE[0] * 16)?;
        let options = Options {
            mode,
            sharpness,
            clamp_output: u32::from(clamp),
            ..defaults()
        };
        self.pass.run(
            self.warp.context.as_raw(),
            output.as_raw(),
            self.depth.as_raw(),
            options,
            facts,
        )?;
        let readback = unsafe { self.warp.read(&output, &desc, 4) }?;
        Ok((output, readback))
    }
}

fn pixels(make_rgb: impl Fn(usize) -> [f32; 3]) -> Vec<f32> {
    (0..COUNT)
        .flat_map(|i| {
            let [r, g, b] = make_rgb(i % WIDTH);
            [r, g, b, i as f32 / COUNT as f32]
        })
        .collect()
}

fn gpu(warp: &Device) -> TestResult {
    let desc = Device::desc(
        SIZE,
        DXGI_FORMAT_R32_FLOAT,
        D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS,
    );
    let depth = warp.initialized(&desc, &[0.5f32; COUNT], SIZE[0] * 4)?;
    let mut fixture = Fixture {
        warp,
        pass: Sharpen(ptr::null_mut()),
        depth,
    };
    let mut facts = frame();
    facts.near_plane = 1.0;
    facts.far_plane = 100.0;
    let flat = pixels(|_| [10.0; 3]);
    let edge = pixels(|x| {
        let base = if x < WIDTH / 2 { 10.0 } else { 80.0 };
        [base + 0.2 * x as f32; 3]
    });
    for mode in FILTERS {
        let (_, actual) = fixture.run(&flat, mode, 1.0, false, &facts)?;
        check(actual == flat, "HDR flat bit exact with alpha")?;
        let (output, actual) = fixture.run(&edge, mode, 1.0, false, &facts)?;
        let mut changed = false;
        for (got, source) in actual.chunks_exact(4).zip(edge.chunks_exact(4)) {
            check(got[3] == source[3], "HDR edge alpha retained")?;
            for (&got, &source) in got[..3].iter().zip(&source[..3]) {
                check(
                    got.is_finite() && (10.0 - 1e-4..=87.0 + 1e-4).contains(&got),
                    "HDR edge bounded",
                )?;
                changed |= (got - source).abs() > 1e-3;
            }
        }
        check(changed, "HDR edge changed")?;
        fixture.pass.run(
            warp.context.as_raw(),
            output.as_raw(),
            ptr::null_mut(),
            Options {
                mode,
                sharpness: 1.0,
                ..defaults()
            },
            &facts,
        )?;
    }

    let colored = pixels(|x| {
        let ramp = if x < WIDTH / 2 {
            2.0 + (x * x) as f32 * 0.015
        } else {
            8.0 + (x * x) as f32 * 0.02
        };
        [ramp, ramp * 0.53, ramp * 0.19]
    });
    for mode in DEPTH_FILTERS {
        let (_, actual) = fixture.run(&colored, mode, 1.0, false, &facts)?;
        let mut changed = false;
        for (got, source) in actual.chunks_exact(4).zip(colored.chunks_exact(4)) {
            let tolerance = got[0].max(1.0) * 2e-5;
            check(
                got[0].is_finite()
                    && got[3] == source[3]
                    && (got[1] - got[0] * 0.53).abs() < tolerance
                    && (got[2] - got[0] * 0.19).abs() < tolerance,
                "colored HDR retains chroma and alpha",
            )?;
            changed |= (got[0] - source[0]).abs() > 1e-4;
        }
        check(changed, "colored HDR changes brightness")?;
    }
    facts.hdr = 0;
    let oversized = pixels(|_| [2.0, 1.0, 0.5]);
    let clamped = pixels(|_| [1.0, 0.5, 0.25]);
    for mode in DEPTH_FILTERS {
        let (_, actual) = fixture.run(&oversized, mode, 0.4, true, &facts)?;
        check(
            actual == clamped,
            "depth clamp keeps positive channel ratios and alpha",
        )?;
    }
    let white = pixels(|_| [1.0; 3]);
    for mode in FILTERS {
        let (_, actual) = fixture.run(&white, mode, 0.4, false, &facts)?;
        check(actual == white, "SDR white finite and flat")?;
    }
    let before = fixture.pass.dispatches();
    let (_, actual) = fixture.run(&white, 0, 0.4, false, &facts)?;
    check(
        actual
            .iter()
            .map(|x| x.to_bits())
            .eq(white.iter().map(|x| x.to_bits()))
            && before == fixture.pass.dispatches(),
        "Off bit exact and dispatch count unchanged",
    )
}

fn cpu() -> TestResult {
    check_constants()?;
    let mut pass = Sharpen(ptr::null_mut());
    for options in [
        defaults(),
        Options {
            mode: 1,
            sharpness: 0.0,
            ..defaults()
        },
    ] {
        pass.run(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            options,
            &frame(),
        )?;
    }
    check(
        pass.0.is_null() && pass.dispatches() == 0,
        "disabled pass needs no resources",
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_fixture() -> i32 {
    if let Err(error) = cpu() {
        eprintln!("sharpening: {error}");
        return 1;
    }
    run_warp("sharpening", gpu)
}
