// SPDX-License-Identifier: MIT
//! Synthetic Runtime receipts for the same temporal driver used by the plugin.
use super::*;
use crate::sr_pass::PassError;
use crate::sr_stage_abi::{
    rsf_anomaly_sr_camera, rsf_anomaly_sr_jitter_state, rsf_anomaly_sr_pass,
};
use rsf_game_sys::{RSF_GAME_RENDER_RESET, rsf_game_render_pass};

const OUTPUT: [u32; 2] = [1920, 1080];
const RESIZED: [u32; 2] = [2560, 1440];
const QUALITY: [u32; 2] = [1280, 720];
const BALANCED: [u32; 2] = [1129, 635];
const BALANCED_RESIZED: [u32; 2] = [1506, 847];

#[derive(Default)]
struct Fake {
    setup: rsf_dlss_pipeline_setup,
    device: usize,
    starts: u32,
    stops: u32,
    selects: u32,
    qualities: u32,
    presets: u32,
    resizes: u32,
    evaluates: u32,
    backend: u32,
    quality: u32,
    preset: u32,
    output: [u32; 2],
    render: [u32; 2],
    running: bool,
    refuse_fsr4: bool,
    start_result: i32,
    evaluate_result: i32,
    flags: u32,
    frame: u64,
    session: u64,
}

impl Fake {
    fn plan(&mut self) {
        let ratio = [1.0, 1.5, 1.7, 2.0, 3.0][self.quality as usize];
        self.render = self
            .output
            .map(|size| (f64::from(size) / ratio + 0.5) as u32);
    }

    fn reset(&mut self) {
        *self = Self {
            refuse_fsr4: true,
            ..Self::default()
        };
    }
}

thread_local! {
    static FAKE: std::cell::RefCell<Fake> = std::cell::RefCell::new(Fake::default());
    static LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn table() -> rsf_d3d11_sr_api {
    unsafe extern "C" fn start(device: Raw, setup: *const rsf_dlss_pipeline_setup) -> i32 {
        let setup = unsafe { *setup };
        FAKE.with_borrow_mut(|f| {
            f.starts += 1;
            f.setup = setup;
            if f.start_result != OK {
                return f.start_result;
            }
            f.device = device.addr();
            f.running = true;
            f.backend = DLSS;
            f.quality = setup.quality;
            f.output = [setup.output_width, setup.output_height];
            f.plan();
            OK
        })
    }
    unsafe extern "C" fn evaluate(_: Raw, pass: *const rsf_game_render_pass) -> i32 {
        let pass = unsafe { &*pass };
        FAKE.with_borrow_mut(|f| {
            f.evaluates += 1;
            f.flags = pass.flags;
            f.frame = pass.native_frame;
            f.session = pass.session_id;
            f.evaluate_result
        })
    }
    unsafe extern "C" fn reset() -> i32 {
        OK
    }
    unsafe extern "C" fn stop() -> i32 {
        FAKE.with_borrow_mut(|f| {
            f.stops += 1;
            f.running = false;
        });
        OK
    }
    unsafe extern "C" fn status(out: *mut rsf_dlss_pipeline_status) -> i32 {
        let status = FAKE.with_borrow(|f| rsf_dlss_pipeline_status {
            running: u32::from(f.running),
            dlss_supported: u32::from(f.running),
            output_width: f.output[0],
            output_height: f.output[1],
            render_width: f.render[0],
            render_height: f.render[1],
            backend: f.backend,
            requested_backend: f.backend,
            ..fresh_pipeline_status()
        });
        unsafe { out.write(status) };
        OK
    }
    unsafe extern "C" fn select(value: u32, _: *mut u32, _: *mut u32) -> i32 {
        FAKE.with_borrow_mut(|f| {
            f.selects += 1;
            if value == FSR4 && f.refuse_fsr4 {
                return NOT_SUPPORTED;
            }
            f.backend = value;
            OK
        })
    }
    unsafe extern "C" fn quality(value: u32, _: *mut u32, _: *mut u32) -> i32 {
        FAKE.with_borrow_mut(|f| {
            f.qualities += 1;
            f.quality = value;
            f.plan();
        });
        OK
    }
    unsafe extern "C" fn preset(value: u32) -> i32 {
        FAKE.with_borrow_mut(|f| {
            f.presets += 1;
            f.preset = value;
        });
        OK
    }
    unsafe extern "C" fn resize(w: u32, h: u32, _: *mut u32, _: *mut u32) -> i32 {
        FAKE.with_borrow_mut(|f| {
            f.resizes += 1;
            f.output = [w, h];
            f.plan();
        });
        OK
    }
    unsafe extern "C" fn output() -> Raw {
        null_mut()
    }
    unsafe extern "C" fn dump(_: *const c_char) -> i32 {
        OK
    }
    unsafe extern "C" fn sharpening(_: *const c_void) -> i32 {
        OK
    }

    rsf_d3d11_sr_api {
        struct_size: size_of::<rsf_d3d11_sr_api>() as u32,
        abi_version: RSF_D3D11_SR_API_VERSION,
        pipeline_abi_version: RSF_DLSS_PIPELINE_ABI_VERSION,
        start: Some(start),
        evaluate: Some(evaluate),
        reset: Some(reset),
        stop: Some(stop),
        status: Some(status),
        select_backend: Some(select),
        set_quality: Some(quality),
        set_preset: Some(preset),
        resize_output: Some(resize),
        output_texture: Some(output),
        request_dump: Some(dump),
        set_sharpening: Some(sharpening),
    }
}

unsafe extern "C" fn collect(_: Raw, text: *const c_char) {
    let line = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    LINES.with_borrow_mut(|lines| lines.push(line));
}

fn driver(stage: u32, streamline: &CStr, logging: bool) -> Box<Driver> {
    let driver = unsafe { Box::from_raw(rsf_anomaly_sr_create()) };
    let options = rsf_anomaly_sr_driver_config {
        stage,
        first_person_zero: true,
        initial: rsf_anomaly_sr_settings {
            backend: DLSS,
            ..DEFAULT_SETTINGS
        },
        streamline: streamline.as_ptr(),
        fidelityfx: c"C:\\ffx".as_ptr(),
        xess: c"C:\\xess".as_ptr(),
        ..Default::default()
    };
    unsafe {
        rsf_anomaly_sr_configure(&driver, &options, logging.then_some(collect), null_mut());
        assert!(rsf_anomaly_sr_bind_table(
            &driver,
            &table(),
            rsf_anomaly_sr_session_id()
        ));
    }
    driver
}

fn boundary(driver: &Driver, device: Raw, [w, h]: [u32; 2]) {
    unsafe { rsf_anomaly_sr_boundary(driver, device, w, h) };
}

fn plan(driver: &Driver, [w, h]: [u32; 2]) -> Option<[u32; 2]> {
    let (mut rw, mut rh) = (0, 0);
    unsafe { rsf_anomaly_sr_plan(driver, w, h, &mut rw, &mut rh) }.then_some([rw, rh])
}

fn request(driver: &Driver, enabled: bool, backend: u32, quality: u32, preset: u32) {
    rsf_anomaly_sr_request(
        driver,
        &Settings {
            enabled,
            backend,
            quality,
            preset,
        },
    );
}

fn frame([rw, rh]: [u32; 2], index: u64, sequence: bool) -> PassInputs {
    let h = 1.0 / 0.6_f32.tan();
    let w = h / (16.0 / 9.0);
    let q = 900.0 / (900.0 - 0.2);
    let camera = rsf_anomaly_sr_camera {
        view: [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
        projection: [
            w,
            0.,
            0.,
            0.,
            0.,
            h,
            0.,
            0.,
            0.,
            0.,
            q,
            1.,
            0.,
            0.,
            -q * 0.2,
            0.,
        ],
        valid: true,
    };
    let [color, color_output, depth, motion] = [1, 2, 3, 4].map(ptr::without_provenance_mut);
    PassInputs {
        frame: rsf_anomaly_sr_pass {
            native_frame: index,
            render_width: rw,
            render_height: rh,
            output_width: OUTPUT[0],
            output_height: OUTPUT[1],
            current: camera,
            previous: camera,
            jitter: rsf_anomaly_sr_jitter_state {
                applied: true,
                render_width: rw,
                render_height: rh,
                current_x: 0.25,
                reset: sequence,
                ..Default::default()
            },
            ..Default::default()
        },
        color,
        color_output,
        depth,
        motion,
        ..Default::default()
    }
}

fn evaluate(driver: &Driver, input: &mut PassInputs, facts: ResetFacts) -> bool {
    unsafe { rsf_anomaly_sr_evaluate(driver, null_mut(), input, &facts, null_mut(), null_mut()) }
}

fn accept(driver: &Driver, input: &mut PassInputs, facts: ResetFacts, reset: bool) {
    assert!(evaluate(driver, input, facts));
    FAKE.with_borrow(|f| assert_eq!(f.flags & RSF_GAME_RENDER_RESET != 0, reset));
}

fn report(driver: &Driver) -> rsf_overlay_sr_report {
    let mut report = rsf_overlay_sr_report::default();
    rsf_anomaly_sr_overlay_report(driver, &mut report);
    report
}

fn reason(report: &rsf_overlay_sr_report) -> &str {
    unsafe { CStr::from_ptr(report.reason.as_ptr()) }
        .to_str()
        .unwrap()
}

fn stop(driver: &Driver) {
    unsafe { rsf_anomaly_sr_stop(driver) };
}

fn logged(fragment: &str) -> bool {
    LINES.with_borrow(|lines| lines.iter().any(|line| line.contains(fragment)))
}

fn periodic(driver: &Driver) {
    LINES.with_borrow_mut(Vec::clear);
    rsf_anomaly_sr_report(driver, false, true, Some(collect), null_mut());
    LINES.with_borrow(|lines| assert_eq!(lines.len(), 2));
}

pub(super) fn lifecycle() {
    FAKE.with_borrow_mut(Fake::reset);
    let driver = driver(5, c"C:\\sl", false);
    let [a, b] = [0x1000, 0x1100].map(ptr::without_provenance_mut);
    boundary(&driver, a, OUTPUT);
    assert!(rsf_anomaly_sr_running(&driver));
    FAKE.with_borrow(|f| {
        assert_eq!(
            (f.starts, f.device, f.selects, f.presets),
            (1, a.addr(), 0, 0)
        );
        let s = &f.setup;
        assert_eq!(s.struct_size as usize, size_of::<rsf_dlss_pipeline_setup>());
        assert_eq!(
            (s.abi_version, s.engine, s.standard_depth),
            (RSF_DLSS_PIPELINE_ABI_VERSION, CUSTOM_ENGINE, 1)
        );
        assert_eq!(s.view_space_to_meters, 1.0);
        for (value, expected) in [
            (s.project_id_utf8, RSF_ANOMALY_SR_PROJECT_ID),
            (s.engine_version_utf8, RSF_ANOMALY_SR_ENGINE_VERSION),
            (s.streamline_directory_utf8, b"C:\\sl\0".as_slice()),
            (s.fsr3_directory_utf8, b"C:\\ffx\0".as_slice()),
            (s.fsr4_directory_utf8, b"C:\\ffx\0".as_slice()),
            (s.fsr2_directory_utf8, b"\0".as_slice()),
            (s.xess_directory_utf8, b"C:\\xess\0".as_slice()),
        ] {
            assert_eq!(
                unsafe { CStr::from_ptr(value) }.to_bytes_with_nul(),
                expected
            );
        }
        assert_eq!((s.output_width, s.quality), (1920, 1));
        assert_eq!(
            (
                s.motion.validity_enabled,
                s.motion.validity_channel,
                s.motion.zero_means_unwritten
            ),
            (1, 2, 0)
        );
        assert_eq!(s.motion.scale_x, 1.0);
    });
    assert_eq!(plan(&driver, OUTPUT), Some(QUALITY));
    assert_eq!(plan(&driver, RESIZED), None);
    boundary(&driver, a, OUTPUT);
    FAKE.with_borrow(|f| assert_eq!((f.starts, f.selects), (1, 0)));

    accept(
        &driver,
        &mut frame(QUALITY, 1, false),
        ResetFacts::default(),
        true,
    );
    FAKE.with_borrow(|f| {
        assert_eq!(f.frame, 1);
        assert_ne!(f.session, 0);
    });
    accept(
        &driver,
        &mut frame(QUALITY, 2, false),
        ResetFacts::default(),
        false,
    );
    accept(
        &driver,
        &mut frame(QUALITY, 3, true),
        ResetFacts {
            sequence: true,
            ..Default::default()
        },
        true,
    );
    accept(
        &driver,
        &mut frame(QUALITY, 4, false),
        ResetFacts {
            cut: true,
            ..Default::default()
        },
        true,
    );
    let c = driver.published().counters;
    assert_eq!(c.evaluated[DLSS as usize], 4);
    for reset in [ResetReason::First, ResetReason::Sequence, ResetReason::Cut] {
        assert_eq!(c.resets[reset as usize], 1);
    }
    let view = report(&driver);
    assert_eq!(
        (
            view.effective_backend,
            view.render_width,
            view.effective_quality
        ),
        (DLSS, 1280, 1)
    );
    assert_eq!(view.inputs & RSF_OVERLAY_SR_INPUT_EXPOSURE, 0);
    assert!(reason(&view).is_empty());

    request(&driver, true, XESS, 2, 4);
    FAKE.with_borrow(|f| assert_eq!(f.selects, 0));
    boundary(&driver, a, OUTPUT);
    FAKE.with_borrow(|f| assert_eq!((f.selects, f.qualities), (0, 1)));
    assert_eq!(driver.published().status.pending_backend, XESS);
    let view = report(&driver);
    assert_eq!((view.effective_quality, view.render_width), (1, 1280));
    boundary(&driver, a, OUTPUT);
    FAKE.with_borrow(|f| {
        assert_eq!(
            (
                f.selects,
                f.backend,
                f.qualities,
                f.quality,
                f.presets,
                f.preset
            ),
            (1, XESS, 1, 2, 1, 4)
        )
    });
    assert_eq!(plan(&driver, OUTPUT), Some(BALANCED));
    accept(
        &driver,
        &mut frame(BALANCED, 5, false),
        ResetFacts::default(),
        true,
    );
    let c = driver.published().counters;
    assert_eq!(c.evaluated[XESS as usize], 1);
    assert_eq!(c.resets[ResetReason::Settings as usize], 1);

    request(&driver, true, FSR4, 2, 4);
    for _ in 0..3 {
        boundary(&driver, a, OUTPUT);
    }
    FAKE.with_borrow(|f| assert_eq!((f.selects, f.backend), (2, XESS)));
    assert_eq!(driver.published().counters.select_failures, 1);
    assert!(evaluate(
        &driver,
        &mut frame(BALANCED, 6, false),
        ResetFacts::default()
    ));
    let view = report(&driver);
    assert_eq!(view.effective_backend, XESS);
    assert!(reason(&view).contains("FSR4"));

    request(&driver, false, XESS, 2, 4);
    boundary(&driver, a, OUTPUT);
    assert!(!rsf_anomaly_sr_enabled(&driver) && rsf_anomaly_sr_running(&driver));
    FAKE.with_borrow(|f| assert_eq!(f.selects, 2));
    request(&driver, true, XESS, 2, 4);
    boundary(&driver, a, OUTPUT);
    assert!(rsf_anomaly_sr_enabled(&driver));
    boundary(&driver, a, RESIZED);
    FAKE.with_borrow(|f| assert_eq!(f.resizes, 1));
    assert_eq!(plan(&driver, RESIZED), Some(BALANCED_RESIZED));

    FAKE.with_borrow_mut(|f| f.evaluate_result = -9);
    assert!(!evaluate(
        &driver,
        &mut frame(BALANCED_RESIZED, 7, false),
        ResetFacts::default()
    ));
    let c = driver.published().counters;
    assert_eq!(
        (c.results[9], c.refused[Refusal::Pipeline as usize]),
        (1, 1)
    );
    let view = report(&driver);
    assert_eq!(view.effective_backend, 0);
    assert!(reason(&view).contains("refused"));
    let before = FAKE.with_borrow_mut(|f| {
        f.evaluate_result = OK;
        f.evaluates
    });
    let mut input = frame(BALANCED_RESIZED, 8, false);
    input.frame.jitter.applied = false;
    assert!(!evaluate(&driver, &mut input, ResetFacts::default()));
    FAKE.with_borrow(|f| assert_eq!(f.evaluates, before));
    assert_eq!(
        driver.published().counters.pass_errors[PassError::Jitter as usize],
        1
    );

    boundary(&driver, b, RESIZED);
    FAKE.with_borrow(|f| {
        assert_eq!(
            (f.stops, f.starts, f.device, f.backend, f.quality),
            (1, 2, b.addr(), XESS, 2)
        )
    });
    accept(
        &driver,
        &mut frame(BALANCED_RESIZED, 9, false),
        ResetFacts::default(),
        true,
    );
    assert_eq!(
        driver.published().counters.resets[ResetReason::First as usize],
        2
    );
    stop(&driver);
    FAKE.with_borrow(|f| assert_eq!(f.stops, 2));
    assert!(!rsf_anomaly_sr_running(&driver));
    assert_eq!(plan(&driver, RESIZED), None);
    assert!(!evaluate(
        &driver,
        &mut frame(BALANCED_RESIZED, 10, false),
        ResetFacts::default()
    ));
    assert_eq!(
        driver.published().counters.refused[Refusal::NotRunning as usize],
        1
    );
    periodic(&driver);
    assert!(logged("evaluated dlss=4 fsr3=0 fsr4=0 xess=3"));
    assert!(logged("pipeline bound=1 running=0"));

    let inputs_only = self::driver(4, c"C:\\sl", false);
    let starts = FAKE.with_borrow(|f| f.starts);
    boundary(&inputs_only, a, OUTPUT);
    FAKE.with_borrow(|f| assert_eq!(f.starts, starts));
    assert!(!rsf_anomaly_sr_running(&inputs_only));
}

pub(super) fn refused_start() {
    FAKE.with_borrow_mut(|f| {
        f.reset();
        f.start_result = STREAMLINE_FAILED;
    });
    LINES.with_borrow_mut(Vec::clear);
    let directory = c"D:\\Anomaly 1.5.3\\bin\\ReScaleFrame\\streamline";
    let driver = driver(5, directory, true);
    let device = ptr::without_provenance_mut(0x1000);
    boundary(&driver, device, OUTPUT);
    FAKE.with_borrow(|f| assert_eq!(f.starts, 1));
    assert!(!rsf_anomaly_sr_running(&driver));
    assert_eq!(driver.published().counters.start_failures, 1);
    assert!(logged("did not start (-5, Streamline failed)"));
    LINES.with_borrow(|lines| {
        assert!(lines.iter().any(|line| {
            line.contains("SR is OFF")
                && line.contains(directory.to_str().unwrap())
                && line.contains("SRStreamlineDir")
        }))
    });
    let view = report(&driver);
    assert_eq!(view.effective_backend, 0);
    let details = reason(&view);
    assert!(
        details.contains("SR off")
            && details.contains("Streamline")
            && details.contains("SRStreamlineDir")
    );
    assert!(details.len() < view.reason.len() - 1);
    assert!(start_refusal(NOT_SUPPORTED).contains("selected SR provider"));
    assert!(start_refusal(-7).contains("resource failed"));
    for result in [NOT_SUPPORTED, STREAMLINE_FAILED, -1, -7] {
        assert!(start_refusal(result).len() < view.reason.len());
    }
    boundary(&driver, device, OUTPUT);
    FAKE.with_borrow_mut(|f| {
        assert_eq!(f.starts, 1);
        f.start_result = OK;
    });
    boundary(&driver, device, RESIZED);
    FAKE.with_borrow(|f| assert_eq!((f.starts, f.setup.output_width), (2, 2560)));
    assert!(rsf_anomaly_sr_running(&driver));
    stop(&driver);
}

pub(super) fn announce() {
    FAKE.with_borrow_mut(|f| {
        f.reset();
        f.refuse_fsr4 = false;
    });
    LINES.with_borrow_mut(Vec::clear);
    let driver = driver(5, c"C:\\sl", true);
    let a = ptr::without_provenance_mut(0x1000);
    boundary(&driver, a, OUTPUT);
    assert!(evaluate(
        &driver,
        &mut frame(QUALITY, 1, false),
        ResetFacts::default()
    ));
    let pending = |driver: &Driver| driver.published().status.pending_backend;
    let applied = |driver: &Driver| driver.published().status.applied.backend;
    let selects = || FAKE.with_borrow(|f| f.selects);
    let want = |driver: &Driver, backend| {
        request(driver, true, backend, 1, 0);
        boundary(driver, a, OUTPUT);
    };
    want(&driver, FSR4);
    assert_eq!(
        (selects(), applied(&driver), pending(&driver)),
        (0, DLSS, FSR4)
    );
    assert_eq!(driver.published().counters.select_announced, 1);
    accept(
        &driver,
        &mut frame(QUALITY, 2, false),
        ResetFacts::default(),
        false,
    );
    let view = report(&driver);
    assert_eq!(view.effective_backend, DLSS);
    let details = reason(&view);
    assert!(
        details.contains("Preparing FSR4")
            && details.contains("pauses")
            && details.len() < view.reason.len() - 1
    );
    boundary(&driver, a, OUTPUT);
    assert_eq!(
        (selects(), applied(&driver), pending(&driver)),
        (1, FSR4, 0)
    );
    FAKE.with_borrow(|f| assert_eq!(f.backend, FSR4));
    accept(
        &driver,
        &mut frame(QUALITY, 3, false),
        ResetFacts::default(),
        true,
    );
    let view = report(&driver);
    assert_eq!(view.effective_backend, FSR4);
    assert!(reason(&view).is_empty());
    want(&driver, DLSS);
    assert_eq!((selects(), pending(&driver)), (2, 0));
    want(&driver, FSR4);
    assert_eq!((selects(), pending(&driver)), (3, 0));
    assert_eq!(driver.published().counters.select_announced, 1);

    want(&driver, XESS);
    assert_eq!((selects(), pending(&driver)), (3, XESS));
    want(&driver, FSR4);
    assert_eq!(
        (selects(), pending(&driver), applied(&driver)),
        (3, 0, FSR4)
    );
    FAKE.with_borrow_mut(|f| f.refuse_fsr4 = true);
    want(&driver, DLSS);
    let other = self::driver(5, c"C:\\sl", false);
    FAKE.with_borrow_mut(|f| f.backend = DLSS);
    boundary(&other, a, OUTPUT);
    want(&other, FSR4);
    boundary(&other, a, OUTPUT);
    let asked = selects();
    boundary(&other, a, OUTPUT);
    boundary(&other, a, OUTPUT);
    assert_eq!(
        (
            selects(),
            other.published().counters.select_failures,
            pending(&other)
        ),
        (asked, 1, 0)
    );
    assert!(logged(
        "preparing FSR4. The first use makes the vendor build its pipelines"
    ));
    assert!(logged("FSR4 selected, the boundary took"));

    want(&other, DLSS);
    FAKE.with_borrow_mut(|f| f.refuse_fsr4 = false);
    want(&other, FSR4);
    boundary(&other, a, OUTPUT);
    assert_eq!((selects(), applied(&other)), (asked + 1, FSR4));
    want(&driver, XESS);
    assert_eq!(pending(&driver), XESS);
    stop(&driver);
    assert!(!rsf_anomaly_sr_running(&driver));
    assert_eq!(pending(&driver), 0);
    periodic(&other);
    assert!(logged("announced=") && logged("longest_select_ms="));
}
