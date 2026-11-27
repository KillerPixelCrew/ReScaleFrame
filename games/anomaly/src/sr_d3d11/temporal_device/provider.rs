// SPDX-License-Identifier: MIT
use super::*;
use crate::runtime_module::{Module, rsf_anomaly_runtime_module};
use crate::sr_pass::build_pass;
use crate::sr_stage::math::{JitterSequence, next_jitter};
use crate::sr_temporal::{DLSS, FSR3, FSR4, XESS, fg_fixture::Temporal};
use rsf_game_sys::{RSF_OVERLAY_SR_INPUT_DEPTH, RSF_OVERLAY_SR_INPUT_JITTER};
use std::ffi::{CString, OsStr};
use std::path::{Path, PathBuf};
use std::time::Instant;
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
use windows_sys::core::w;

const OUTPUT: [u32; 2] = [960, 540];
const FRAMES: u32 = 24;

struct Scene {
    size: [u32; 2],
    texture: ID3D11Texture2D,
    colour: ID3D11ShaderResourceView,
    motion: ID3D11ShaderResourceView,
    position: ID3D11ShaderResourceView,
    distort: ID3D11ShaderResourceView,
    depth: Depth,
}

impl Scene {
    fn new(d: &Device, size: [u32; 2]) -> Self {
        let [w, h] = size;
        let n = texels(size);
        let texture = d.texture(size, HDR, SRV | RTV).unwrap();
        let colour = d.srv(&texture, None).unwrap();
        let mut motion = vec![0u16; n * 4];
        for pixel in motion[..(w * (h / 2) * 4) as usize].chunks_exact_mut(4) {
            pixel[2] = half(1.);
        }
        let motion = d
            .data_texture(size, HDR, SRV | RTV, &motion, w * 8)
            .unwrap();
        let motion = d.srv(&motion, None).unwrap();
        let position = d
            .data_texture(size, HDR, SRV, &vec![half(2.); n * 4], w * 8)
            .unwrap();
        let position = d.srv(&position, None).unwrap();
        let distort = d
            .data_texture(size, UNORM, SRV, &vec![0x8080_8080u32; n], w * 4)
            .unwrap();
        let distort = d.srv(&distort, None).unwrap();
        let depths: Vec<_> = (0..w * h)
            .map(|i| {
                d24(if i % w < w / 8 && i / w > h * 3 / 4 {
                    0.01
                } else {
                    0.9
                })
            })
            .collect();
        let depth = Depth::new(d, size, &depths);
        Self {
            size,
            texture,
            colour,
            motion,
            position,
            distort,
            depth,
        }
    }

    fn render(&self, d: &Device, jitter: &Jitter) {
        let [w, h] = self.size;
        let heights: Vec<_> = (0..w * h)
            .flat_map(|i| {
                let u = (f64::from(i % w) + 0.5 - f64::from(jitter.current_x)) / f64::from(w);
                let v = (f64::from(i / w) + 0.5 - f64::from(jitter.current_y)) / f64::from(h);
                let c = half(tonemap(radiance(u, v)) as f32);
                [c, c, c, half(1.)]
            })
            .collect();
        unsafe {
            d.context
                .UpdateSubresource(&self.texture, 0, None, heights.as_ptr().cast(), w * 8, 0)
        };
    }

    fn prepare(&self, d: &Device, pass: &Pass, targets: &TemporalTargets) {
        super::prepare(
            d,
            pass,
            self.size,
            targets,
            [&self.colour, &self.depth.srv, &self.motion],
            true,
        );
    }

    fn input(
        &self,
        targets: &TemporalTargets,
        current: Camera,
        previous: Camera,
        jitter: Jitter,
        auto_exposure: bool,
    ) -> PassInputs {
        let views = targets.view();
        let mut input = frame_inputs(self.size, OUTPUT, current, previous, jitter);
        input.color = views.colour;
        input.color_output = views.output;
        input.motion = views.motion;
        input.depth = self.depth.texture.as_raw();
        input.exposure = if auto_exposure {
            null_mut()
        } else {
            views.exposure
        };
        input
    }

    fn views(&self, output: &Outputs, reconstruction: *mut c_void) -> DrawViews {
        output.reinsert(
            self.size,
            OUTPUT,
            [
                self.colour.as_raw(),
                self.position.as_raw(),
                self.distort.as_raw(),
                self.depth.srv.as_raw(),
                reconstruction,
            ],
        )
    }
}

fn radiance(u: f64, v: f64) -> f64 {
    0.15 + 0.35 * (0.5 + 0.5 * (u * 9.).sin()) * (0.5 + 0.5 * (v * 7.).cos()) + 0.6 * u * v
}

fn next(sequence: &mut JitterSequence, state: Jitter, [w, h]: [u32; 2]) -> Jitter {
    let sample = sequence.next([w, h, OUTPUT[0], OUTPUT[1]], 0, true);
    next_jitter(&state, false, true, &sample, w, h, sequence.reset)
}

struct Dump(PathBuf);
impl Dump {
    fn path(&self, suffix: &str) -> PathBuf {
        let mut path = self.0.clone().into_os_string();
        path.push(suffix);
        path.into()
    }
}
impl Drop for Dump {
    fn drop(&mut self) {
        for suffix in [
            "_input",
            "_output",
            "_motion",
            "_motion_decoded",
            "_motion_submitted",
            "_depth",
            "_exposure",
        ] {
            for extension in [".bin", "_raw.json", ".png", ".json", ".tga"] {
                let _ = std::fs::remove_file(self.path(&format!("{suffix}{extension}")));
            }
        }
        let _ = std::fs::remove_file(self.path("_frame.json"));
    }
}

fn capture(
    d: &Device,
    pass: &Pass,
    driver: &Temporal,
    scene: &Scene,
    targets: &TemporalTargets,
    camera: Camera,
    jitter: Jitter,
) {
    let prefix =
        Dump(std::env::temp_dir().join(format!("rsf_anomaly_sr_motion_{}", std::process::id())));
    let path = CString::new(prefix.0.to_string_lossy().as_bytes()).unwrap();
    assert!(driver.dump(&path));
    scene.render(d, &jitter);
    scene.prepare(d, pass, targets);
    let previous = super::camera(view_at(0.5), camera.projection);
    let mut input = scene.input(targets, camera, previous, jitter, false);
    assert!(driver.evaluate_with_sequence(d.context.as_raw(), &mut input, false));
    let built = build_pass(&input).unwrap_or_else(|error| panic!("dump frame: {}", error.name()));
    let bytes = std::fs::read(prefix.path("_motion_submitted.bin")).unwrap();
    let format = std::fs::read(prefix.path("_motion_submitted_raw.json")).unwrap();
    assert!(
        format
            .windows(b"\"format\":34".len())
            .any(|part| part == b"\"format\":34")
    );
    assert_eq!(bytes.len(), texels(scene.size) * 4);
    let [w, h] = scene.size;
    let (mut zero, mut camera) = (0, 0);
    for (i, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let [x, y] = [i as u32 % w, i as u32 / w];
        let [mx, my] = [
            pixel[..2].try_into().unwrap(),
            pixel[2..].try_into().unwrap(),
        ]
        .map(u16::from_le_bytes)
        .map(half_to_float)
        .map(f64::from);
        assert!(
            mx.is_finite() && my.is_finite() && mx.abs() <= 500. && my.abs() <= 500.,
            "sentinel at {x},{y}"
        );
        if y < h / 2 || (x < w / 8 && y > h * 3 / 4) {
            assert_eq!([mx, my], [0., 0.], "valid or first-person zero at {x},{y}");
            zero += 1;
        } else {
            let [ex, ey] = camera_motion(
                [x, y],
                scene.size,
                f64::from(d24(0.9)) / 16_777_215.,
                &built.camera.clip_to_previous_clip,
            );
            assert!(
                (mx - ex).abs() <= 0.02 + 2e-3 * ex.abs()
                    && (my - ey).abs() <= 0.02
                    && ex.abs() > 0.1,
                "reconstructed unwritten motion at {x},{y}: got [{mx}, {my}], expected [{ex}, {ey}]"
            );
            camera += 1;
        }
    }
    println!("DLSS submitted motion: {zero} zero, {camera} reconstructed, 0 sentinels");
    assert!(zero > 0 && camera > 0);
}

fn compare(d: &Device, outputs: &Outputs, targets: &TemporalTargets) -> (f64, f64) {
    let colour = read::<u8>(d, &outputs.colour, 4);
    let raw = targets.view().output;
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
    let linear = read::<u16>(d, texture, 4);
    let [ow, oh] = OUTPUT;
    assert_eq!(
        (colour.len(), linear.len()),
        (texels(OUTPUT) * 4, texels(OUTPUT) * 4)
    );
    let (mut sum, mut worst, mut samples) = (0f64, 0f64, 0);
    for y in (oh / 8..oh * 7 / 8).step_by(7) {
        for x in (ow / 8..ow * 7 / 8).step_by(7) {
            let i = ((y * ow + x) * 4) as usize;
            let u = (f64::from(x) + 0.5) / f64::from(ow);
            let v = (f64::from(y) + 0.5) / f64::from(oh);
            let error = (f64::from(colour[i]) - 255. * tonemap(radiance(u, v)).min(1.)).abs();
            sum += error;
            worst = worst.max(error);
            samples += 1;
            assert!(
                half_to_float(linear[i]).is_finite(),
                "non-finite linear output at {x},{y}"
            );
        }
    }
    assert!(samples > 0);
    (sum / f64::from(samples), worst)
}

pub(super) fn run(root: &OsStr) -> bool {
    let Some(d) = Device::hardware(0x10de, false) else {
        println!("SKIP provider: no NVIDIA D3D11 adapter for the DLSS-first fixture");
        return false;
    };
    let mut runtime = rsf_anomaly_runtime_module();
    let _lease = if runtime.is_null() {
        runtime = unsafe { LoadLibraryW(w!("ReScaleFrame.Runtime.dll")) };
        Some(unsafe { Module::from_owned(runtime) }.expect("fixture Runtime module"))
    } else {
        None
    };
    assert_eq!(runtime, rsf_anomaly_runtime_module());
    let dirs = ["streamline", "fidelityfx", "xess"].map(|dir| {
        let path = Path::new(root).join(dir);
        CString::new(path.to_string_lossy().as_bytes()).unwrap()
    });
    let driver = Temporal::bind(
        runtime,
        dirs.each_ref().map(|dir| dir.as_c_str()),
        Some(log),
    )
    .expect("bind SR Runtime");
    let device = d.device.as_raw();
    let context = d.context.as_raw();
    driver.boundary(device, OUTPUT);
    assert!(driver.running().0);
    let pass = make_pass(&d);
    let outputs = Outputs::new(&d, OUTPUT);
    let proj = projection(OUTPUT[0] as f32 / OUTPUT[1] as f32);
    let camera = camera(view_at(0.), proj);
    let mut providers = 0;
    for (backend, name, auto_exposure, dump) in [
        (DLSS, "DLSS", false, false),
        (DLSS, "DLSS auto exposure", true, false),
        (FSR3, "FSR3", false, false),
        (FSR4, "FSR4", false, false),
        (XESS, "XeSS", false, false),
        (DLSS, "DLSS again", false, true),
    ] {
        driver.request(backend);
        driver.boundary(device, OUTPUT);
        let start = Instant::now();
        for _ in 0..4 {
            if driver.selection().0 == 0 {
                break;
            }
            driver.boundary(device, OUTPUT);
        }
        let (pending, effective, result) = driver.selection();
        assert_eq!(pending, 0);
        println!("{name}: selected after {} ms", start.elapsed().as_millis());
        if effective != backend {
            println!("{name}: refused (select result {result})");
            assert_eq!(backend, FSR4, "only FSR4 may be unavailable on this GPU");
            continue;
        }
        let size = driver.plan(OUTPUT).unwrap();
        let targets =
            TemporalTargets::new(device, extent(size), extent(OUTPUT), driver.output_format())
                .unwrap();
        let scene = Scene::new(&d, size);
        let mut sequence = JitterSequence::new();
        let mut state = Jitter::default();
        let mut evaluated = 0;
        for _ in 0..FRAMES {
            state = next(&mut sequence, state, size);
            scene.render(&d, &state);
            scene.prepare(&d, &pass, &targets);
            let mut input = scene.input(&targets, camera, camera, state, auto_exposure);
            if driver.evaluate_with_sequence(context, &mut input, state.reset) {
                evaluated += 1;
                let views = scene.views(&outputs, targets.view().output_srv);
                assert!(pass.run(context, REINSERT, &views));
            }
        }
        let (mean, worst) = compare(&d, &outputs, &targets);
        if dump {
            state = next(&mut sequence, state, size);
            capture(&d, &pass, &driver, &scene, &targets, camera, state);
        }
        println!(
            "{name}: {evaluated}/{FRAMES} frames, mean {mean:.2}, worst {worst:.1} UNORM8 codes"
        );
        assert!(evaluated == FRAMES && mean < 3. && worst < 24.);
        providers += 1;
    }
    let report = driver.summary();
    assert_eq!(report.effective_backend, DLSS);
    assert!(report.render_width > 0 && report.output_width == OUTPUT[0]);
    let required = RSF_OVERLAY_SR_INPUT_JITTER | RSF_OVERLAY_SR_INPUT_DEPTH;
    assert_eq!(report.inputs & required, required);
    assert_eq!(report.reason[0], 0);
    assert!(providers >= 1);
    driver.stop();
    assert_eq!(driver.running(), (false, 0));
    true
}
