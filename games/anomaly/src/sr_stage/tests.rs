// SPDX-License-Identifier: MIT
use super::*;
use std::cell::RefCell;

// A fake leaf table: each test thread records the calls its stage makes, in order.
#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    ini: Vec<(&'static str, &'static str)>,
    scope: bool,
    generation: u64,
    eye_x: f32,
    window_open: bool,
    suppress_swap: bool,
    jittered: bool,
    refused: bool,
    driver_enabled: bool,
    temporal_ready: bool,
    targets: i32,
    motion: rsf_anomaly_sr_motion_target,
    engine_frame: u32,
    evaluate: bool,
    reinsert: bool,
    hooks_created: bool,
    passes: Vec<rsf_anomaly_sr_pass>,
    logs: Vec<String>,
}

thread_local! {
    static FAKE: RefCell<Fake> = RefCell::new(Fake::default());
}

fn fake<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
    FAKE.with(|state| f(&mut state.borrow_mut()))
}
fn record(call: impl Into<String>) {
    let call = call.into();
    fake(|f| f.calls.push(call));
}
fn calls() -> Vec<String> {
    fake(|f| std::mem::take(&mut f.calls))
}
fn position(calls: &[String], name: &str) -> usize {
    calls
        .iter()
        .position(|c| c == name)
        .unwrap_or_else(|| panic!("{name} missing from {calls:?}"))
}

const CONTEXT: *mut c_void = 0x1000 as *mut c_void;

unsafe extern "C" fn frame_facts(
    _: *mut c_void,
    world: bool,
    temporal: bool,
    out: *mut rsf_anomaly_sr_frame_facts,
) {
    let (scope, generation) = fake(|f| (f.scope, f.generation));
    let facts = unsafe { &mut *out };
    *facts = rsf_anomaly_sr_frame_facts {
        device_width: 1920,
        device_height: 1080,
        scope,
        generation,
        ..Default::default()
    };
    record(format!("facts world={world} temporal={temporal}"));
}
unsafe extern "C" fn controller(out: *mut rsf_anomaly_sr_controller) {
    let (open, jittered, refused) = fake(|f| (f.window_open, f.jittered, f.refused));
    unsafe {
        out.write(rsf_anomaly_sr_controller {
            window_open: open,
            refused,
            jitter_applied: jittered,
            stage: 2,
            render_width: 1280,
            render_height: 720,
            twin_generation: 3,
            refusal: if refused {
                c"layout".as_ptr()
            } else {
                std::ptr::null()
            },
            ..Default::default()
        })
    };
}
unsafe extern "C" fn prepare(facts: *const rsf_anomaly_sr_frame_facts) -> bool {
    let facts = unsafe { &*facts };
    record(format!(
        "prepare stage={} jitter={} render={}x{}",
        facts.stage, facts.jitter, facts.render_width, facts.render_height
    ));
    let open = !facts.scope && facts.stage == 2 && !fake(|f| f.suppress_swap);
    fake(|f| {
        f.window_open = open;
        f.jittered = open && facts.jitter;
    });
    open
}
extern "C" fn geometry_active() -> bool {
    true
}
unsafe extern "C" fn read_camera(out: *mut rsf_anomaly_sr_camera) -> bool {
    let camera = unsafe { &mut *out };
    let x = fake(|f| f.eye_x);
    camera.view = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, 0.0, 0.0, 1.0,
    ];
    camera.projection = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.001, 1.0, 0.0, 0.0, -0.2, 0.0,
    ];
    record("camera");
    true
}
unsafe extern "C" fn now(counter: *mut i64, frequency: *mut i64) {
    unsafe {
        counter.write(1000);
        frequency.write(10_000);
    }
}
extern "C" fn seam(cookie: *mut c_void) -> bool {
    record("controller seam");
    if !fake(|f| f.window_open) {
        return false;
    }
    fake(|f| f.window_open = false);
    let seam = rsf_anomaly_sr_seam {
        twin_surface: CONTEXT,
        engine_surface: CONTEXT,
        render_width: 1280,
        render_height: 720,
        output_width: 1920,
        output_height: 1080,
    };
    // The real controller reaches the Rust seam pass through the host exactly like this.
    unsafe { rsf_anomaly_sr_stage_upscale(cookie, &seam) }
}
extern "C" fn context() -> *mut c_void {
    CONTEXT
}
extern "C" fn bilinear() -> bool {
    record("bilinear");
    true
}
extern "C" fn temporal_ready() -> bool {
    fake(|f| f.temporal_ready)
}
extern "C" fn temporal_targets(_: u32) -> i32 {
    fake(|f| f.targets)
}
unsafe extern "C" fn motion_target(out: *mut rsf_anomaly_sr_motion_target) -> bool {
    let target = fake(|f| f.motion);
    unsafe { out.write(target) };
    !target.texture.is_null()
}
extern "C" fn motion_view(_: *mut c_void) -> bool {
    true
}
extern "C" fn engine_frame() -> u32 {
    fake(|f| f.engine_frame)
}
extern "C" fn seam_prepare(motion: bool, first_person_zero: bool) -> bool {
    record(format!(
        "seam prepare motion={motion} fp_zero={first_person_zero}"
    ));
    true
}
extern "C" fn driver_enabled() -> bool {
    fake(|f| f.driver_enabled)
}
extern "C" fn driver_count(reason: u8) {
    record(format!("refused {reason}"));
}
extern "C" fn driver_count_bilinear() {
    record("count bilinear");
}
extern "C" fn driver_count_prepared(missing: bool) {
    record(format!("prepared missing={missing}"));
}
unsafe extern "C" fn driver_evaluate(pass: *const rsf_anomaly_sr_pass) -> bool {
    let pass = unsafe { *pass };
    record("evaluate");
    fake(|f| {
        f.passes.push(pass);
        f.evaluate
    })
}
extern "C" fn reinsert() -> bool {
    record("reinsert");
    fake(|f| f.reinsert)
}
extern "C" fn controller_frame_end() {
    record("controller frame_end");
}
unsafe extern "C" fn device_size(width: *mut u32, height: *mut u32) {
    unsafe {
        width.write(1920);
        height.write(1080);
    }
}
extern "C" fn driver_boundary(width: u32, height: u32) {
    record(format!("boundary {width}x{height}"));
}
extern "C" fn set_swap_enabled(enabled: bool) {
    record(format!("swap {enabled}"));
}
extern "C" fn hooks_created() -> bool {
    fake(|f| f.hooks_created)
}
extern "C" fn install_hooks() -> bool {
    record("install hooks");
    fake(|f| f.hooks_created = true);
    true
}
extern "C" fn install_position_slot() -> bool {
    true
}
extern "C" fn set_rm_near(on: bool) {
    record(format!("rm_near {on}"));
}
extern "C" fn remove_hooks() -> bool {
    record("remove hooks");
    true
}
extern "C" fn shutdown() {
    record("controller shutdown");
}
extern "C" fn driver_stop() {
    record("driver stop");
}
extern "C" fn host_reset() {
    record("host reset");
}
extern "C" fn driver_bind() -> bool {
    record("driver bind");
    true
}
unsafe extern "C" fn set_engine(_: *mut c_void, _: *const c_void, temporal: bool) {
    record(format!("set engine temporal={temporal}"));
}
unsafe extern "C" fn ini_int(_: *const c_void, key: *const c_char, fallback: i32) -> i32 {
    let key = unsafe { CStr::from_ptr(key) }.to_str().unwrap();
    fake(|f| {
        f.ini
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(fallback, |(_, v)| v.parse().unwrap_or(0))
    })
}
unsafe extern "C" fn ini_text(
    _: *const c_void,
    key: *const c_char,
    fallback: *const c_char,
    out: *mut u16,
    capacity: u32,
) -> u32 {
    let key = unsafe { CStr::from_ptr(key) }.to_str().unwrap();
    let fallback = unsafe { CStr::from_ptr(fallback) }
        .to_str()
        .unwrap()
        .to_owned();
    let value = fake(|f| {
        f.ini
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(fallback, |(_, v)| (*v).to_owned())
    });
    let wide: Vec<u16> = value.encode_utf16().take(capacity as usize - 1).collect();
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), out, wide.len());
        out.add(wide.len()).write(0);
    }
    wide.len() as u32
}
unsafe extern "C" fn persist_int(key: *const c_char, value: i32) {
    record(format!(
        "save {}={value}",
        unsafe { CStr::from_ptr(key) }.to_string_lossy()
    ));
}
unsafe extern "C" fn persist_text(key: *const c_char, value: *const c_char) {
    let (key, value) = unsafe { (CStr::from_ptr(key), CStr::from_ptr(value)) };
    record(format!(
        "save {}={}",
        key.to_string_lossy(),
        value.to_string_lossy()
    ));
}
extern "C" fn apply_colour_transport(transport: bool) {
    record(format!("transport {transport}"));
}
unsafe extern "C" fn vendor_file(directory: u32, _: *const c_char) -> bool {
    directory == 2
}
unsafe extern "C" fn log(_: *mut c_void, text: *const c_char) {
    let text = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    fake(|f| f.logs.push(text));
}

fn platform() -> Box<Platform> {
    Box::new(Platform {
        frame_facts: Some(frame_facts),
        engine_frame: Some(engine_frame),
        device_size: Some(device_size),
        read_camera: Some(read_camera),
        context: Some(context),
        device: Some(context),
        now: Some(now),
        geometry_active: Some(geometry_active),
        prepare: Some(prepare),
        seam: Some(seam),
        frame_end: Some(controller_frame_end),
        shutdown: Some(shutdown),
        set_swap_enabled: Some(set_swap_enabled),
        controller: Some(controller),
        bilinear: Some(bilinear),
        temporal_ready: Some(temporal_ready),
        temporal_targets: Some(temporal_targets),
        motion_target: Some(motion_target),
        motion_view: Some(motion_view),
        seam_prepare: Some(seam_prepare),
        reinsert: Some(reinsert),
        driver_bind: Some(driver_bind),
        driver_enabled: Some(driver_enabled),
        driver_boundary: Some(driver_boundary),
        driver_evaluate: Some(driver_evaluate),
        driver_count: Some(driver_count),
        driver_count_bilinear: Some(driver_count_bilinear),
        driver_count_prepared: Some(driver_count_prepared),
        driver_stop: Some(driver_stop),
        hooks_created: Some(hooks_created),
        set_engine: Some(set_engine),
        install_hooks: Some(install_hooks),
        install_position_slot: Some(install_position_slot),
        set_rm_near: Some(set_rm_near),
        remove_hooks: Some(remove_hooks),
        host_reset: Some(host_reset),
        ini_int: Some(ini_int),
        ini_text: Some(ini_text),
        persist_int: Some(persist_int),
        persist_text: Some(persist_text),
        apply_colour_transport: Some(apply_colour_transport),
        vendor_file: Some(vendor_file),
        ..Default::default()
    })
}

fn installed(stage: u32, table: &Platform) -> Stage {
    let driver = Stage::new();
    driver.register(table);
    // Each instance stands for a fresh process: no trampolines exist yet.
    fake(|f| f.hooks_created = false);
    let config = Config {
        stage,
        ..default_config()
    };
    assert!(driver.configure(&config, Some(log), null_mut()));
    assert!(driver.install(0x10000 as *mut c_void, 64, 64, 0x20 as *const c_void));
    assert!(driver.active());
    calls();
    driver
}

#[test]
fn fg_borrows_stock_depth_only_after_the_matching_unswapped_sr_off_prepare() {
    let table = platform();
    let driver = installed(5, &table);
    fake(|f| {
        f.suppress_swap = true;
        f.engine_frame = 7;
        f.scope = true;
    });
    driver.prepare(null_mut());
    assert!(driver.owns_depth(7));

    fake(|f| f.scope = false);
    driver.prepare(null_mut());
    assert!(!driver.owns_depth(7));
    assert!(driver.owns_depth(6));
    assert!(driver.owns_depth(8));

    // The next frame must prove its own eligibility even if the engine repeats the ID.
    driver.frame_end();
    assert!(driver.owns_depth(7));

    fake(|f| f.driver_enabled = true);
    driver.prepare(null_mut());
    assert!(driver.owns_depth(7));

    fake(|f| {
        f.driver_enabled = false;
        f.suppress_swap = false;
    });
    driver.prepare(null_mut());
    assert!(driver.owns_depth(7));
}

fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn wcstod_matches_the_crt_prefix_rules() {
    assert_eq!(wcstod(&wide_text("0.67")), (0.67, 4));
    assert_eq!(wcstod(&wide_text("  -1.5e1x")), (-15.0, 8));
    assert_eq!(wcstod(&wide_text("1e")), (1.0, 1));
    assert_eq!(wcstod(&wide_text("0x1p-1")), (0.5, 6));
    assert_eq!(wcstod(&wide_text("0x")), (0.0, 1));
    assert_eq!(wcstod(&wide_text("INF")).0, f64::INFINITY);
    assert!(wcstod(&wide_text("nan(1)")).0.is_nan());
    assert_eq!(wcstod(&wide_text("x1")).1, 0);
    assert_eq!(wcstod(&wide_text(".")).1, 0);
}

#[test]
fn options_parse_like_the_native_reader() {
    let table = platform();
    let driver = Stage::new();
    driver.register(&*table);
    fake(|f| {
        f.ini = vec![
            ("SRStage", "3"),
            ("SRScale", "0x1p-1"),
            ("SRMipBias", "  OFF "),
            ("SRExposure", "AUTO"),
            ("SRCutDistance", "5000"),
            ("SRCutAngle", "30deg"),
            ("SRStageCaptureCount", "99"),
            ("SRStageCaptureInterval", "0"),
            ("SRBackend", "2"),
            ("SRQuality", "4"),
        ]
    });
    let ini = std::ptr::dangling::<c_void>();
    let options = driver.read_options(ini);
    assert_eq!(options.stage, 3);
    assert_eq!((options.scale, options.scale_defaulted), (0.5, false));
    assert!(!options.mip_bias && options.exposure_auto);
    assert_eq!((options.cut_distance, options.cut_degrees), (1000.0, 30.0));
    assert_eq!((options.capture_count, options.capture_interval), (32, 1));
    // A backend the adapter cannot run keeps the default; a valid quality is kept.
    assert_eq!(
        (options.settings.backend, options.settings.quality),
        (BACKEND_DLSS, 4)
    );
    fake(|f| f.ini = vec![("SRScale", "0.67x"), ("SRMipBias", "-5")]);
    assert_eq!(driver.read_scale(ini), (DEFAULT_SCALE, true));
    assert_eq!(driver.read_mip_bias(ini), (-2.0, true));
}

#[test]
fn quiesce_releases_hooks_before_state_and_the_pipeline_before_resources() {
    let table = platform();
    let driver = installed(5, &table);
    assert!(driver.quiesce());
    let order = calls();
    let at = |name| position(&order, name);
    assert!(at("remove hooks") < at("controller shutdown"));
    assert!(at("controller shutdown") < at("driver stop"));
    assert!(at("driver stop") < at("host reset"));
    assert!(!driver.active());
    // Published jitter returns to the stock (unjittered) state for the motion stage.
    assert_eq!(driver.jitter_uv().current_u, 0.0);
    assert_eq!(driver.jitter().phase_count, 8);
}

#[test]
fn install_refuses_a_different_build_after_the_trampolines_exist() {
    let table = platform();
    let driver = installed(2, &table);
    assert!(driver.quiesce());
    assert!(!driver.install(0x10000 as *mut c_void, 64, 64, 0x30 as *const c_void));
    assert!(fake(|f| f
        .logs
        .iter()
        .any(|l| l.contains("a different build"))));
    // The verified build re-enables the same trampolines.
    assert!(driver.install(0x10000 as *mut c_void, 64, 64, 0x20 as *const c_void));
}

#[test]
fn scope_frames_neither_jitter_nor_read_the_main_camera() {
    let table = platform();
    let driver = installed(5, &table);
    fake(|f| f.scope = true);
    driver.prepare(null_mut());
    let order = calls();
    assert!(!order.iter().any(|c| c == "camera"));
    assert_eq!(driver.lock().as_ref().unwrap().sequence.frames, 0);
    assert!(driver.jitter().scope);
    fake(|f| f.scope = false);
    driver.prepare(null_mut());
    let order = calls();
    assert!(
        position(&order, "camera")
            < position(&order, "prepare stage=2 jitter=true render=1286x724")
    );
    assert!(
        order
            .iter()
            .any(|c| c == "prepare stage=2 jitter=true render=1286x724")
    );
    let published = driver.jitter();
    assert!(published.applied && published.frame == 1);
    assert_eq!(driver.jitter_uv().current_u, 0.0);
    assert_eq!(driver.jitter_uv().current_v, (-1.0f32 / 6.0) / 720.0);
    assert!(published.reset);
    // A frame without H1 (menu, loading) between two main frames breaks continuity; consecutive frames keep it.
    driver.frame_end();
    driver.prepare(null_mut());
    assert!(!driver.jitter().reset);
    assert_eq!(driver.jitter().sequence_index, 1);
    driver.frame_end();
    driver.frame_end();
    driver.prepare(null_mut());
    assert!(driver.jitter().reset);
    assert_eq!(driver.jitter().sequence_index, 0);
}

#[test]
fn h1_detects_a_cut_from_the_previous_main_camera() {
    let table = platform();
    let driver = installed(5, &table);
    let cut = || driver.lock().as_ref().unwrap().frame_cut;
    driver.prepare(null_mut());
    assert!(!cut());
    driver.frame_end();
    fake(|f| f.eye_x = 4.0);
    driver.prepare(null_mut());
    assert!(!cut());
    driver.frame_end();
    fake(|f| f.eye_x = 8.25);
    driver.prepare(null_mut());
    assert!(cut());
    driver.frame_end();
    fake(|f| f.eye_x = f32::NAN);
    driver.prepare(null_mut());
    assert!(!cut());
}

#[test]
fn live_jitter_off_skips_the_sample() {
    let table = platform();
    let driver = installed(4, &table);
    driver.set_option(RSF_ANOMALY_SR_OPTION_JITTER, 0);
    assert_eq!(calls(), ["save SRJitter=0"]);
    driver.prepare(null_mut());
    assert_eq!(driver.lock().as_ref().unwrap().sequence.frames, 0);
    assert!(!driver.jitter().applied);
}

#[test]
fn temporal_seam_counts_each_refusal_once_and_always_draws_a_world_image() {
    let table = platform();
    let driver = installed(5, &table);
    // Not ready: one Prepare refusal, then the bilinear pass.
    driver.prepare(null_mut());
    calls();
    driver.seam(null_mut());
    assert_eq!(
        calls(),
        ["controller seam", "refused 2", "count bilinear", "bilinear"]
    );
    // Ready but the pipeline is not enabled: the inputs are prepared and the bilinear pass draws.
    fake(|f| f.temporal_ready = true);
    driver.prepare(null_mut());
    calls();
    driver.seam(null_mut());
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=false fp_zero=true",
            "prepared missing=true",
            "count bilinear",
            "bilinear"
        ]
    );
    // Evaluation that the Runtime refuses: no extra refusal reason, bilinear output.
    fake(|f| {
        f.driver_enabled = true;
        f.engine_frame = 9;
        f.motion = rsf_anomaly_sr_motion_target {
            texture: CONTEXT,
            written: true,
            main: true,
            frame: 9,
            width: 1280,
            height: 720,
        };
    });
    driver.prepare(null_mut());
    calls();
    driver.seam(null_mut());
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=true fp_zero=true",
            "prepared missing=false",
            "evaluate",
            "count bilinear",
            "bilinear"
        ]
    );
    let pass = fake(|f| f.passes.pop()).unwrap();
    assert!(!pass.motion_missing && pass.jitter.applied && pass.generation == 3);
    assert_eq!((pass.render_width, pass.output_width), (1280, 1920));
    // A motion target from another frame is not this frame's motion.
    fake(|f| {
        f.motion.frame = 8;
        f.evaluate = true;
    });
    driver.prepare(null_mut());
    calls();
    driver.seam(null_mut());
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=false fp_zero=true",
            "prepared missing=true",
            "evaluate",
            "reinsert",
            "refused 5",
            "count bilinear",
            "bilinear"
        ]
    );
    // Success reinserts and draws no fallback.
    fake(|f| f.reinsert = true);
    driver.prepare(null_mut());
    calls();
    driver.seam(null_mut());
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=false fp_zero=true",
            "prepared missing=true",
            "evaluate",
            "reinsert"
        ]
    );
    // Refused input targets are logged and refused before any input is prepared.
    fake(|f| f.targets = -1);
    driver.prepare(null_mut());
    calls();
    driver.seam(null_mut());
    assert_eq!(
        calls(),
        ["controller seam", "refused 2", "count bilinear", "bilinear"]
    );
    assert!(fake(|f| f.logs.iter().any(|l| l.contains(
        "SR input targets refused at render 1280x720, output 1920x1080"
    ))));
}

#[test]
fn frame_end_applies_the_drained_boundary_after_the_controller() {
    let table = platform();
    let driver = installed(5, &table);
    fake(|f| f.driver_enabled = true);
    driver.frame_end();
    assert_eq!(
        calls(),
        ["controller frame_end", "boundary 1920x1080", "swap true"]
    );
    // A refused controller keeps its swap disabled.
    fake(|f| f.refused = true);
    driver.frame_end();
    assert_eq!(calls(), ["controller frame_end", "boundary 1920x1080"]);
    // Stage 2 has no pipeline boundary.
    let stage2 = installed(2, &table);
    stage2.frame_end();
    assert_eq!(calls(), ["controller frame_end"]);
}

#[test]
fn live_options_apply_and_persist_through_the_adapter_path() {
    let table = platform();
    let driver = installed(5, &table);
    driver.set_option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT, 0);
    assert_eq!(calls(), ["transport true", "save SRColourTransport=1"]);
    assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT), 0);
    driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET, -5);
    assert_eq!(calls(), ["save SRMipBias=-0.5"]);
    assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET), -5);
    driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS, 0);
    driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET, -10);
    assert_eq!(calls(), ["save SRMipBias=off"]);
    driver.set_option(RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO, 1);
    assert_eq!(calls(), ["save SRExposure=auto"]);
    // Configuration is refused while the hooks are installed.
    assert!(!driver.configure(&default_config(), None, null_mut()));
}

#[test]
fn overlay_offers_only_present_runtimes_and_a_reachable_default() {
    let table = platform();
    let driver = installed(5, &table);
    let choices = driver.choices();
    assert_eq!(choices.backends, 1 << BACKEND_XESS);
    assert!(!choices.preset);
    assert_eq!(choices.defaults.backend, BACKEND_XESS);
    let stage4 = installed(4, &table);
    assert_eq!(stage4.choices().backends, 0);
    assert!(!stage4.overlay_report(std::ptr::dangling_mut::<c_void>()));
}

#[test]
fn report_emits_deltas_and_survives_a_timer_reset() {
    let table = platform();
    let driver = installed(3, &table);
    driver.frame_end();
    let mut lines = Vec::new();
    driver.report(&mut |line| lines.push(line.to_owned()));
    assert!(lines[0].starts_with("Anomaly SR stage window frames=1 no_world=1 "));
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Anomaly SR stage 3 window"))
    );
    lines.clear();
    driver.report(&mut |line| lines.push(line.to_owned()));
    assert!(lines[0].starts_with("Anomaly SR stage window frames=0 no_world=0 "));
}
