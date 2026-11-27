// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::rsf_camera_frame;
use std::cell::RefCell;

// A fake leaf table: each test thread records the calls its stage makes, in order.
#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    ini: Vec<(&'static str, &'static str)>,
    scope: bool,
    unreadable_view: bool,
    unreadable_options: bool,
    options_word3: u32,
    render_target: usize,
    registered: u8,
    precache: u32,
    unreadable_loading: bool,
    unreadable_precache: bool,
    precache_reads: u32,
    generation: u64,
    eye_x: f32,
    window_open: bool,
    suppress_swap: bool,
    spatial_recovery: bool,
    jittered: bool,
    refused: bool,
    driver_enabled: bool,
    temporal_ready: bool,
    motion: rsf_anomaly_sr_motion_target,
    engine_frame: u32,
    evaluate: bool,
    reinsert: bool,
    fail_bilinear: bool,
    timer_tick: u64,
    timer_result: i32,
    hooks_created: bool,
    refuse_remove_hooks: bool,
    refuse_shutdown: bool,
    refuse_persist: bool,
    passes: Vec<rsf_anomaly_sr_pass>,
    logs: Vec<String>,
    offered: u32,
    /// The stage drives the stale-window closer and the GPU timer leaves; otherwise they refuse.
    close_stale: bool,
    timers: bool,
    /// The INI leaves read the file through the production Win32 reader.
    production_ini: bool,
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

pub(crate) extern "C" fn render_target() -> *mut c_void {
    std::ptr::without_provenance_mut(fake(|f| f.render_target))
}

pub(crate) fn frame_facts(world: bool, temporal: bool) -> rsf_anomaly_sr_frame_reads {
    let native_device = device();
    let reads = fake(|f| rsf_anomaly_sr_frame_reads {
        facts: rsf_anomaly_sr_frame_facts {
            device: native_device,
            device_width: 1920,
            device_height: 1080,
            generation: f.generation,
            ..Default::default()
        },
        render_view: u32::from(f.scope),
        options_word3: f.options_word3,
        view_read: !f.unreadable_view,
        options_read: !f.unreadable_options,
    });
    record(format!("facts world={world} temporal={temporal}"));
    reads
}

pub(crate) fn loading_registered() -> Option<u8> {
    fake(|f| (!f.unreadable_loading).then_some(f.registered))
}

pub(crate) fn precache_frame() -> Option<u32> {
    fake(|f| {
        f.precache_reads += 1;
        (!f.unreadable_precache).then_some(f.precache)
    })
}
pub(crate) fn controller() -> rsf_anomaly_sr_controller {
    let (open, jittered, refused) = fake(|f| (f.window_open, f.jittered, f.refused));
    rsf_anomaly_sr_controller {
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
    }
}
pub(crate) unsafe extern "C" fn prepare(
    facts: *const rsf_anomaly_sr_frame_facts,
    _: rsf_anomaly_sr_log,
    _: *mut c_void,
) -> bool {
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
pub(crate) extern "C" fn geometry_active() -> bool {
    true
}
pub(crate) unsafe extern "C" fn read_camera(out: *mut rsf_anomaly_sr_camera) -> bool {
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
pub(crate) fn now() -> (i64, i64) {
    (1000, 10_000)
}
fn seam(run: &mut dyn FnMut(crate::sr_swap::SeamPass<'_>) -> bool) {
    record("controller seam");
    if !fake(|f| f.window_open) {
        return;
    }
    fake(|f| f.window_open = false);
    let colour = TargetFields {
        surface: CONTEXT,
        srv: std::ptr::without_provenance_mut(0x30),
        ..Default::default()
    };
    let position = TargetFields {
        srv: std::ptr::without_provenance_mut(0x31),
        ..Default::default()
    };
    let distort = TargetFields {
        srv: std::ptr::without_provenance_mut(0x32),
        ..Default::default()
    };
    let engine_colour = TargetFields {
        surface: CONTEXT,
        rtv: std::ptr::without_provenance_mut(0x40),
        ..Default::default()
    };
    let engine_position = TargetFields {
        rtv: std::ptr::without_provenance_mut(0x41),
        ..Default::default()
    };
    let engine_distort = TargetFields {
        rtv: std::ptr::without_provenance_mut(0x42),
        ..Default::default()
    };
    let input = UpscaleInputs {
        generic0_twin: &colour,
        position_twin: &position,
        generic1_twin: &distort,
        depth_srv: std::ptr::without_provenance_mut(0x33),
        generic0: &engine_colour,
        position: &engine_position,
        generic1: &engine_distort,
        engine_dsv: std::ptr::without_provenance_mut(0x43),
        render_width: 1280,
        render_height: 720,
        output_width: 1920,
        output_height: 1080,
        spatial_recovery: fake(|f| f.spatial_recovery),
    };
    run(crate::sr_swap::SeamPass::Upscale(&input));
}
pub(crate) extern "C" fn context() -> *mut c_void {
    CONTEXT
}
pub(crate) extern "C" fn device() -> *mut c_void {
    crate::sr_d3d11::fixture_device()
}
pub(crate) extern "C" fn temporal_ready() -> bool {
    fake(|f| f.temporal_ready)
}
pub(crate) fn motion_target() -> rsf_anomaly_sr_motion_target {
    fake(|f| f.motion)
}
pub(crate) extern "C" fn create_motion_view(_: *mut c_void, texture: *mut c_void) -> *mut c_void {
    texture
}
pub(crate) extern "C" fn release_view(_: *mut c_void) {}
pub(crate) extern "C" fn engine_frame() -> u32 {
    fake(|f| f.engine_frame)
}
pub(crate) unsafe extern "C" fn draw_pass(kind: u32, views: *const DrawViews) -> bool {
    let views = unsafe { &*views };
    match kind {
        PREPARE => {
            assert_eq!((views.output.width, views.output.height), (1280, 720));
            assert_eq!(views.inputs[0].addr(), 0x30);
            assert_eq!(views.inputs[1].addr(), 0x33);
            assert_eq!(views.first_person_depth, 0.02f32.to_bits());
            let motion = views.inputs[2] == CONTEXT;
            let first_person_zero = views.first_person_zero != 0;
            record(format!(
                "seam prepare motion={motion} fp_zero={first_person_zero}"
            ));
            true
        }
        UPSCALE | REINSERT => {
            assert_eq!((views.output.width, views.output.height), (1920, 1080));
            assert_eq!((views.source.width, views.source.height), (1280, 720));
            assert_eq!(views.targets.map(|view| view.addr()), [0x40, 0x41, 0x42]);
            assert_eq!(views.depth.addr(), 0x43);
            assert_eq!(
                views.inputs.map(|view| view.addr())[..4],
                [0x30, 0x31, 0x32, 0x33]
            );
            if kind == UPSCALE {
                assert!(views.inputs[4].is_null());
                record("bilinear");
                !fake(|f| f.fail_bilinear)
            } else {
                assert!(!views.inputs[4].is_null());
                record("reinsert");
                fake(|f| f.reinsert)
            }
        }
        _ => unreachable!(),
    }
}
pub(crate) extern "C" fn driver_enabled() -> bool {
    fake(|f| f.driver_enabled)
}
pub(crate) extern "C" fn driver_fallback(reason: u8, drawn: bool) {
    if reason <= REFUSAL_REINSERT {
        record(format!("refused {reason}"));
    }
    record(if drawn {
        "count bilinear"
    } else {
        "bilinear failed"
    });
}
pub(crate) extern "C" fn driver_count_prepared(missing: bool) {
    record(format!("prepared missing={missing}"));
}
pub(crate) unsafe extern "C" fn driver_evaluate(
    pass: *mut rsf_anomaly_sr_pass_inputs,
    _: *mut rsf_camera_frame,
    _: *mut u32,
) -> bool {
    let pass = unsafe { (*pass).frame };
    record("evaluate");
    fake(|f| {
        f.passes.push(pass);
        f.evaluate
    })
}
pub(crate) extern "C" fn controller_frame_end() {
    record("controller frame_end");
}
pub(crate) fn device_size() -> (u32, u32) {
    (1920, 1080)
}
pub(crate) extern "C" fn driver_boundary(width: u32, height: u32) {
    record(format!("boundary {width}x{height}"));
}
pub(crate) extern "C" fn set_swap_enabled(enabled: bool) {
    record(format!("swap {enabled}"));
}
pub(crate) extern "C" fn hooks_created() -> bool {
    fake(|f| f.hooks_created)
}
pub(crate) extern "C" fn install_hooks() -> bool {
    record("install hooks");
    fake(|f| f.hooks_created = true);
    true
}
pub(crate) extern "C" fn install_position_slot() -> bool {
    true
}
pub(crate) extern "C" fn set_rm_near(on: bool) {
    record(format!("rm_near {on}"));
}
pub(crate) extern "C" fn remove_hooks() -> bool {
    record("remove hooks");
    !fake(|f| f.refuse_remove_hooks)
}
pub(crate) extern "C" fn close_stale() {
    record("close stale");
    fake(|f| f.window_open = false);
}
pub(crate) extern "C" fn shutdown() -> bool {
    record("controller shutdown");
    !fake(|f| f.refuse_shutdown)
}
pub(crate) extern "C" fn driver_stop() {
    record("driver stop");
}
pub(crate) extern "C" fn host_reset() {
    record("host reset");
}
pub(crate) extern "C" fn driver_bind() -> bool {
    record("driver bind");
    true
}
pub(crate) unsafe extern "C" fn set_engine(_: *mut c_void, _: *const c_void, temporal: bool) {
    record(format!("set engine temporal={temporal}"));
}
pub(crate) unsafe extern "C" fn ini_int(
    _: *const c_void,
    key: *const c_char,
    fallback: i32,
) -> i32 {
    let key = unsafe { CStr::from_ptr(key) }.to_str().unwrap();
    fake(|f| {
        f.ini
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(fallback, |(_, v)| v.parse().unwrap_or(0))
    })
}
pub(crate) unsafe extern "C" fn ini_text(
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
pub(crate) unsafe extern "C" fn persist_int(key: *const c_char, value: i32) -> bool {
    record(format!(
        "save {}={value}",
        unsafe { CStr::from_ptr(key) }.to_string_lossy()
    ));
    fake(|f| !f.refuse_persist)
}
pub(crate) unsafe extern "C" fn persist_text(key: *const c_char, value: *const c_char) -> bool {
    let (key, value) = unsafe { (CStr::from_ptr(key), CStr::from_ptr(value)) };
    record(format!(
        "save {}={}",
        key.to_string_lossy(),
        value.to_string_lossy()
    ));
    fake(|f| !f.refuse_persist)
}
pub(crate) extern "C" fn apply_colour_transport(transport: bool) {
    record(format!("transport {transport}"));
}
unsafe extern "C" fn admit_backend(input: *mut rsf_game_sys::rsf_d3d11_sr_admission) -> i32 {
    let input = unsafe { &mut *input };
    assert_eq!(input.backend_choices, RSF_ANOMALY_SR_ADAPTER_BACKENDS);
    assert!(input.device.is_null());
    input.offered_backends = fake(|f| f.offered);
    input.selected_backend = if input.offered_backends == 0 {
        0
    } else {
        BACKEND_AUTO
    };
    0
}
pub(crate) unsafe extern "C" fn configured(
    _: rsf_anomaly_sr_log,
    _: *mut c_void,
    driver: *const rsf_anomaly_sr_driver_config,
) {
    let text = match unsafe { driver.as_ref() } {
        None => "configured".to_owned(),
        Some(d) => {
            let text = |value: *const c_char| unsafe { CStr::from_ptr(value) }.to_string_lossy();
            format!(
                "configured stage={} sl={} ffx={} xess={} exposure={} transport={} zero={} signature={} backend={}",
                d.stage,
                text(d.streamline),
                text(d.fidelityfx),
                text(d.xess),
                d.exposure_auto,
                d.colour_transport,
                d.first_person_zero,
                d.require_signature,
                d.initial.backend
            )
        }
    };
    record(text);
}
unsafe extern "C" fn log(_: *mut c_void, text: *const c_char) {
    let text = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    fake(|f| f.logs.push(text));
}

pub(crate) fn verify_guards(
    _: *mut c_void,
    _: u32,
    _: *const c_void,
    _: bool,
    _: bool,
) -> Option<&'static CStr> {
    None
}

/// A bound stage's leaf token, for the owners the tests drive without a stage.
const BOUND: Platform = Platform { bound: true };

fn installed(stage: u32) -> Stage {
    let driver = Stage::new();
    driver.bind();
    // Each instance stands for a fresh process: no trampolines exist yet.
    fake(|f| f.hooks_created = false);
    let config = Config {
        stage,
        ..default_config()
    };
    let paths = Paths {
        ini: Vec::new(),
        vendors: VendorPaths {
            streamline: c"V:\\sl".into(),
            fidelityfx: c"V:\\ffx".into(),
            xess: c"V:\\xess".into(),
            ..Default::default()
        },
    };
    assert!(driver.configure(&config, paths, Some(log), null_mut()));
    assert!(driver.install(0x10000 as *mut c_void, 64, 64, 0x20 as *const c_void));
    assert!(driver.active());
    calls();
    driver
}

#[test]
fn frame_facts_require_the_current_target_and_readable_world_options() {
    let driver = Stage::new();
    driver.bind();
    let mut state = State::default();
    let target = std::ptr::without_provenance_mut(0x20);
    fake(|f| f.render_target = 0x20);
    let facts = driver.gather(&mut state, target);
    assert_eq!(facts.render_target, target);
    assert!(!facts.scope && !facts.msaa);
    assert!(driver.gather(&mut state, CONTEXT).render_target.is_null());

    fake(|f| {
        f.unreadable_view = true;
        f.unreadable_options = true;
    });
    let facts = driver.gather(&mut state, target);
    assert!(facts.scope && facts.msaa);
    fake(|f| {
        f.unreadable_view = false;
        f.unreadable_options = false;
        f.scope = true;
        f.options_word3 = 1 << 14;
    });
    let facts = driver.gather(&mut state, target);
    assert!(facts.scope && facts.msaa);
}

#[test]
fn loading_gate_preserves_precache_read_order_and_refuses_unknown_facts() {
    let driver = Stage::new();
    driver.bind();
    assert!(!driver.loading());
    fake(|f| {
        f.precache_reads = 0;
        f.registered = 1;
    });
    assert!(driver.loading());
    assert_eq!(fake(|f| f.precache_reads), 1);
    fake(|f| {
        f.precache_reads = 0;
        f.unreadable_loading = true;
    });
    assert!(driver.loading());
    assert_eq!(fake(|f| f.precache_reads), 0);
    fake(|f| {
        f.registered = 0;
        f.unreadable_loading = false;
        f.unreadable_precache = true;
    });
    assert!(driver.loading());
    fake(|f| {
        f.unreadable_precache = false;
        f.precache = 1;
    });
    assert!(driver.loading());
}

#[test]
fn fg_borrows_stock_depth_only_after_the_matching_unswapped_sr_off_prepare() {
    let driver = installed(5);
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
fn wcstod_converts_the_longest_decimal_prefix() {
    assert_eq!(wcstod(&wide_text("0.67")), (0.67, 4));
    assert_eq!(wcstod(&wide_text("  -1.5e1x")), (-15.0, 8));
    assert_eq!(wcstod(&wide_text("1e")), (1.0, 1));
    assert_eq!(wcstod(&wide_text("0x1p-1")), (0.0, 1));
    assert_eq!(wcstod(&wide_text("INF")).0, f64::INFINITY);
    assert!(wcstod(&wide_text("nan(1)")).0.is_nan());
    assert_eq!(wcstod(&wide_text("x1")).1, 0);
    assert_eq!(wcstod(&wide_text(".")).1, 0);
}

#[test]
fn options_parse_like_the_native_reader() {
    let driver = Stage::new();
    driver.bind();
    fake(|f| {
        f.ini = vec![
            ("SRStage", "3"),
            ("SRScale", "0.5"),
            ("SRMipBias", "  OFF "),
            ("SRExposure", "AUTO"),
            ("SRCutDistance", "5000"),
            ("SRCutAngle", "30deg"),
            ("SRStageCaptureCount", "99"),
            ("SRStageCaptureInterval", "0"),
            // No backend has id 8.
            ("SRBackend", "8"),
            ("SRQuality", "4"),
        ]
    });
    let ini = std::ptr::dangling::<c_void>();
    let options = driver.ini(ini).options();
    assert_eq!(options.stage, 3);
    assert_eq!((options.scale, options.scale_defaulted), (0.5, false));
    assert!(!options.mip_bias && options.exposure_auto);
    assert_eq!((options.cut_distance, options.cut_degrees), (1000.0, 30.0));
    assert_eq!((options.capture_count, options.capture_interval), (32, 1));
    // A backend the adapter cannot run keeps the default; a valid quality is kept.
    assert_eq!(
        (options.settings.backend, options.settings.quality),
        (BACKEND_AUTO, 4)
    );
    fake(|f| f.ini = vec![("SRScale", "0.67x"), ("SRMipBias", "-5")]);
    assert_eq!(driver.ini(ini).scale(), (DEFAULT_SCALE, true));
    assert_eq!(driver.ini(ini).mip_bias(), (-2.0, true));
}

#[test]
fn quiesce_releases_hooks_before_state_and_the_pipeline_before_resources() {
    fake(|f| f.close_stale = true);
    let driver = installed(5);
    driver.prepare(null_mut());
    assert!(fake(|f| f.window_open));
    calls();
    fake(|f| f.refuse_remove_hooks = true);
    assert!(!driver.quiesce());
    assert!(!driver.active());
    assert_eq!(calls(), ["remove hooks", "close stale"]);
    assert!(!fake(|f| f.window_open) && !driver.jitter().applied);
    driver.prepare(null_mut());
    driver.prepared();
    driver.seam_with(null_mut(), seam);
    driver.frame_end();
    assert!(!driver.sun_begin());
    assert!(calls().is_empty());
    fake(|f| {
        f.refuse_remove_hooks = false;
        f.refuse_shutdown = true;
    });
    assert!(!driver.quiesce());
    assert_eq!(calls(), ["remove hooks", "controller shutdown"]);
    fake(|f| f.refuse_shutdown = false);
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
    let driver = installed(2);
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
    let driver = installed(5);
    fake(|f| f.scope = true);
    driver.prepare(null_mut());
    let order = calls();
    assert!(!order.iter().any(|c| c == "camera"));
    assert_eq!(driver.lock().sequence.frames, 0);
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
    let driver = installed(5);
    let cut = || driver.lock().frame_cut;
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
    let driver = installed(4);
    driver.set_option(RSF_ANOMALY_SR_OPTION_JITTER, 0);
    assert_eq!(calls(), ["save SRJitter=0"]);
    driver.prepare(null_mut());
    assert_eq!(driver.lock().sequence.frames, 0);
    assert!(!driver.jitter().applied);
}

#[test]
fn temporal_seam_counts_each_refusal_once_and_always_draws_a_world_image() {
    let driver = installed(5);
    // Not ready: one Prepare refusal, then the bilinear pass.
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
    assert_eq!(
        calls(),
        ["controller seam", "bilinear", "refused 2", "count bilinear"]
    );
    // Ready but the pipeline is not enabled: the inputs are prepared and the bilinear pass draws.
    fake(|f| f.temporal_ready = true);
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=false fp_zero=true",
            "prepared missing=true",
            "bilinear",
            "count bilinear"
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
    driver.seam_with(null_mut(), seam);
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=true fp_zero=true",
            "prepared missing=false",
            "evaluate",
            "bilinear",
            "count bilinear"
        ]
    );
    let pass = fake(|f| f.passes.pop()).unwrap();
    assert_eq!(pass.native_frame, 9);
    assert!(!pass.motion_missing && pass.jitter.applied && pass.generation == 3);
    assert_eq!((pass.render_width, pass.output_width), (1280, 1920));
    // A motion target from another frame is not this frame's motion.
    fake(|f| {
        f.motion.frame = 8;
        f.evaluate = true;
    });
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
    assert_eq!(
        calls(),
        [
            "controller seam",
            "seam prepare motion=false fp_zero=true",
            "prepared missing=true",
            "evaluate",
            "reinsert",
            "bilinear",
            "refused 5",
            "count bilinear"
        ]
    );
    // Success reinserts and draws no fallback.
    fake(|f| f.reinsert = true);
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
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
    fake(|f| {
        f.reinsert = false;
        f.fail_bilinear = true;
    });
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
    let failed = calls();
    assert!(position(&failed, "bilinear") < position(&failed, "bilinear failed"));
    assert!(!failed.iter().any(|line| line == "count bilinear"));
    fake(|f| f.fail_bilinear = false);
    // Refused input targets are logged and refused before any input is prepared.
    driver.lock().temporal_targets = None;
    crate::sr_d3d11::refuse_fixture_target_view();
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
    assert_eq!(
        calls(),
        ["controller seam", "bilinear", "refused 2", "count bilinear"]
    );
    assert!(fake(|f| f.logs.iter().any(|l| l.contains(
        "SR input targets refused at render 1280x720, output 1920x1080"
    ))));
}

#[test]
fn frame_end_applies_the_drained_boundary_after_the_controller() {
    let driver = installed(5);
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
    let stage2 = installed(2);
    stage2.frame_end();
    assert_eq!(calls(), ["controller frame_end"]);
}

#[test]
fn live_options_apply_and_persist_through_the_adapter_path() {
    let driver = installed(5);
    driver.set_option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT, 0);
    assert_eq!(calls(), ["save SRColourTransport=1", "transport true"]);
    assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT), 0);
    driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET, -5);
    assert_eq!(calls(), ["save SRMipBias=-0.5"]);
    assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET), -5);
    driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS, 0);
    assert!(driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET, -10));
    assert_eq!(
        calls(),
        ["save SRMipBias=off -0.5", "save SRMipBias=off -1.0"]
    );
    driver.set_option(RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO, 1);
    assert_eq!(calls(), ["save SRExposure=auto"]);
    // Configuration is refused while the hooks are installed.
    assert!(!driver.configure(&default_config(), Paths::default(), None, null_mut()));
    assert!(calls().is_empty());

    fake(|f| f.refuse_persist = true);
    assert!(!driver.set_option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT, 1));
    assert_eq!(calls(), ["save SRColourTransport=0"]);
    assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_COLOUR_TRANSPORT), 0);

    assert!(!driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET, -15));
    assert_eq!(calls(), ["save SRMipBias=off -1.5"]);
    assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET), -10);
}

#[test]
fn stage_options_save_before_changing_configuration_or_capture_schedule() {
    let driver = installed(5);
    let options = [
        (
            OPTION_SCALE,
            75,
            90,
            "save SRScale=0.75",
            "save SRScale=0.90",
        ),
        (
            OPTION_CUT_DISTANCE,
            51,
            62,
            "save SRCutDistance=5.1",
            "save SRCutDistance=6.2",
        ),
        (
            OPTION_CUT_ANGLE,
            300,
            600,
            "save SRCutAngle=30.0",
            "save SRCutAngle=60.0",
        ),
        (
            OPTION_CAPTURE_START,
            450,
            700,
            "save SRStageCaptureStart=450",
            "save SRStageCaptureStart=700",
        ),
        (
            OPTION_CAPTURE_COUNT,
            8,
            10,
            "save SRStageCaptureCount=8",
            "save SRStageCaptureCount=10",
        ),
        (
            OPTION_CAPTURE_INTERVAL,
            40,
            30,
            "save SRStageCaptureInterval=40",
            "save SRStageCaptureInterval=30",
        ),
        (
            OPTION_REQUIRE_SIGNATURE,
            1,
            0,
            "save SRRequireSignature=1",
            "save SRRequireSignature=0",
        ),
    ];
    for (id, accepted, _, write, _) in options {
        assert!(driver.set_option(id, accepted));
        assert_eq!(driver.option(id), accepted);
        assert_eq!(calls(), [write]);
    }
    assert_eq!(
        driver.with(|s| (
            s.config.scale,
            s.config.scale_defaulted,
            s.capture.next,
            s.capture.remaining
        )),
        (0.75, false, 450, 8)
    );

    // Give the in-flight schedule progress that an erroneous reset would erase.
    driver.with(|s| {
        s.capture.next = 490;
        s.capture.remaining = 7;
    });
    fake(|f| f.refuse_persist = true);
    for (id, accepted, refused, _, write) in options {
        assert!(!driver.set_option(id, refused));
        assert_eq!(driver.option(id), accepted);
        assert_eq!(calls(), [write]);
    }
    assert_eq!(
        driver.with(|s| (
            s.config.scale,
            s.config.scale_defaulted,
            s.capture.next,
            s.capture.remaining
        )),
        (0.75, false, 490, 7)
    );
}

#[test]
fn refused_stage_three_capture_edits_keep_the_current_burst() {
    let driver = Stage::new();
    driver.bind();
    driver.active.store(true, Ordering::Release);
    driver.with(|s| {
        s.config.stage = 3;
        s.config.capture = true;
        s.config.capture_start = 600;
        s.config.capture_count = 8;
        s.burst_revision = 7;
        s.burst.armed = true;
    });
    fake(|f| f.refuse_persist = true);
    assert!(!driver.set_option(OPTION_CAPTURE_START, 700));
    assert!(!driver.set_option(OPTION_CAPTURE_COUNT, 32));
    assert_eq!(
        calls(),
        [
            "save SRStageCaptureStart=700",
            "save SRStageCaptureCount=32"
        ]
    );
    assert_eq!(
        driver.with(|s| (
            s.config.capture_start,
            s.config.capture_count,
            s.burst_revision,
            s.burst.armed
        )),
        (600, 8, 7, true)
    );
}

#[test]
fn overlay_uses_the_runtime_mask_and_auto_default() {
    const XESS: u32 = 5;
    let driver = installed(5);
    driver.with(|state| state.config.settings.backend = BACKEND_DLSS);
    fake(|f| f.offered = (1 << BACKEND_AUTO) | (1 << XESS));
    let choices = driver.choices_with(Some(admit_backend));
    assert_eq!(choices.backends, (1 << BACKEND_AUTO) | (1 << XESS));
    assert!(!choices.preset);
    assert_eq!(choices.defaults.backend, BACKEND_AUTO);
    driver.with(|state| state.config.settings.backend = XESS);
    assert_eq!(
        driver.choices_with(Some(admit_backend)).defaults.backend,
        XESS
    );
    assert_eq!(driver.choices_with(None).backends, 0);
    let stage4 = installed(4);
    assert_eq!(stage4.choices_with(Some(admit_backend)).backends, 0);
    assert!(!stage4.overlay_report(std::ptr::dangling_mut::<c_void>()));
}

#[test]
fn sun_camera_recovery_runs_spatial_seam_without_temporal_evaluation() {
    let driver = installed(5);
    fake(|f| {
        f.spatial_recovery = true;
        f.temporal_ready = true;
        f.evaluate = true;
    });
    driver.prepare(null_mut());
    calls();
    driver.seam_with(null_mut(), seam);
    assert_eq!(calls(), ["controller seam", "bilinear", "count bilinear"]);
}

#[test]
fn report_uses_live_exposure_and_first_person_options() {
    let driver = installed(5);
    let pipeline_report = || {
        let mut result = String::new();
        driver.report(&mut |line| {
            if line.starts_with("Anomaly SR stage 5 pipeline") {
                result = line.to_owned();
            }
        });
        result
    };
    driver.set_option(RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO, 0);
    driver.set_option(RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO, 1);
    assert!(pipeline_report().contains("exposure=constant(1.0) first_person_zero=1"));
    driver.set_option(RSF_ANOMALY_SR_OPTION_EXPOSURE_AUTO, 1);
    assert!(pipeline_report().contains("exposure=auto first_person_zero=1"));
    driver.set_option(RSF_ANOMALY_SR_OPTION_FIRST_PERSON_ZERO, 0);
    assert!(pipeline_report().contains("exposure=auto first_person_zero=0"));
}

pub(crate) extern "C" fn retain_timer(device: *mut c_void) -> *mut c_void {
    device
}
pub(crate) extern "C" fn create_timer_query(device: *mut c_void, _: bool) -> *mut c_void {
    device
}
pub(crate) extern "C" fn issue_timer_query(_: *mut c_void, _: *mut c_void, _: bool) {}
pub(crate) unsafe extern "C" fn timer_clock(
    _: *mut c_void,
    _: *mut c_void,
    hz: *mut u64,
    disjoint: *mut bool,
) -> i32 {
    unsafe {
        hz.write(1000);
        disjoint.write(false)
    };
    fake(|f| f.timer_result)
}
pub(crate) unsafe extern "C" fn timer_timestamp(
    _: *mut c_void,
    _: *mut c_void,
    ticks: *mut u64,
) -> i32 {
    let tick = fake(|f| {
        f.timer_tick += 2;
        f.timer_tick
    });
    unsafe { ticks.write(tick) };
    0
}

#[test]
fn report_emits_deltas_and_survives_a_timer_reset() {
    fake(|f| f.timers = true);
    let table = BOUND;
    let driver = installed(3);
    driver.frame_end();
    let current = device();
    driver.with(|state| {
        state.timer.begin(&table, current, CONTEXT);
        state.timer.seam(&table, current, CONTEXT, true);
        state.timer.finish(&table, current, CONTEXT);
    });
    let mut lines = Vec::new();
    driver.report(&mut |line| lines.push(line.to_owned()));
    assert!(lines[0].starts_with("Anomaly SR stage window frames=1 no_world=1 "));
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Anomaly SR stage 3 window"))
    );
    assert!(lines[0].contains("swapped=2.000 (n=1)"));
    let replacement = std::ptr::without_provenance_mut(0x1200);
    driver.with(|state| state.timer.begin(&table, replacement, CONTEXT));
    lines.clear();
    driver.report(&mut |line| lines.push(line.to_owned()));
    assert!(lines[0].starts_with("Anomaly SR stage window frames=0 no_world=0 "));
    assert!(lines[0].contains("swapped=0.000 (n=0)"));
}

#[test]
fn timer_retires_failed_queries_and_keeps_pending_samples() {
    fake(|f| f.timers = true);
    let table = BOUND;
    let mut timer = gpu_timer::Timer::default();
    let device = device();
    let issue = |timer: &mut gpu_timer::Timer| {
        timer.begin(&table, device, CONTEXT);
        timer.seam(&table, device, CONTEXT, false);
        timer.finish(&table, device, CONTEXT);
    };
    issue(&mut timer);
    fake(|f| f.timer_result = 1);
    let pending = timer.sample(&table, device, CONTEXT);
    assert_eq!((pending.samples, pending.skipped), ([0, 0], 0));
    fake(|f| f.timer_result = -1);
    for _ in 0..8 {
        issue(&mut timer);
    }
    let failed = timer.sample(&table, device, CONTEXT);
    assert_eq!((failed.samples, failed.skipped), ([0, 0], 9));
    fake(|f| f.timer_result = 0);
    issue(&mut timer);
    let ready = timer.sample(&table, device, CONTEXT);
    assert_eq!((ready.samples, ready.skipped), ([1, 0], 9));
}

#[cfg(windows)]
#[test]
fn the_cold_entry_keeps_only_a_selected_configuration() {
    let driver = Stage::new();
    driver.bind();
    calls();
    let ini = units(Path::new(r"D:\Anomaly\bin\rsf.ini"));
    assert!(!unsafe {
        rsf_anomaly_sr_stage_configure_ini(std::ptr::null(), true, Some(log), null_mut())
    });
    fake(|f| f.ini = vec![("SRStage", "0")]);
    assert!(!driver.configure_ini(&ini, true, Some(log), null_mut()));
    // SRStageCapture alone selects the hooks; a read without `apply` keeps nothing.
    fake(|f| f.ini = vec![("SRStage", "0"), ("SRStageCapture", "1")]);
    assert!(driver.configure_ini(&ini, false, Some(log), null_mut()));
    assert!(calls().is_empty());
    assert!(!driver.selected() && !driver.capture());
    assert!(driver.configure_ini(&ini, true, Some(log), null_mut()));
    // Below Stage 4 the TemporalDriver gets no options.
    assert_eq!(calls(), ["configured"]);
    assert!(driver.selected() && driver.capture());
    assert_eq!(driver.choices_with(Some(admit_backend)).backends, 0);
}

#[cfg(windows)]
#[test]
fn vendor_directories_sit_beside_the_ini() {
    let driver = Stage::new();
    driver.bind();
    calls();
    fake(|f| {
        f.ini = vec![("SRStage", "5"), ("SRBackend", "5"), ("SRExposure", "auto")];
    });
    // The carrier can pass a long-path INI name; the vendor directories must not keep the prefix.
    let ini = units(Path::new(r"\\?\D:\Anomaly 1.5.3\bin\rsf.ini"));
    assert!(driver.configure_ini(&ini, true, Some(log), null_mut()));
    let beside = r"D:\Anomaly 1.5.3\bin\ReScaleFrame";
    assert_eq!(
        calls(),
        [format!(
            r"configured stage=5 sl={beside}\streamline ffx={beside}\fidelityfx xess={beside}\xess exposure=true transport=false zero=true signature=false backend=5"
        )]
    );
}

#[test]
fn a_device_reset_rereads_srscale_from_the_kept_ini() {
    let driver = Stage::new();
    driver.bind();
    fake(|f| {
        f.hooks_created = false;
        f.generation = 1;
        f.ini = vec![("SRStage", "2"), ("SRScale", "0.5")];
        f.logs.clear();
    });
    let ini = units(Path::new(r"D:\Anomaly\bin\rsf.ini"));
    assert!(driver.configure_ini(&ini, true, Some(log), null_mut()));
    assert!(driver.install(0x10000 as *mut c_void, 64, 64, 0x20 as *const c_void));
    driver.prepare(null_mut());
    fake(|f| {
        f.ini = vec![("SRStage", "2"), ("SRScale", "0.75")];
        f.generation = 2;
    });
    driver.prepare(null_mut());
    let reread = "Anomaly SR stage 2: SRScale 0.500 -> 0.750 after a device reset.";
    assert!(fake(|f| f.logs.iter().any(|l| l == reread)));
    // A stage configured without an INI keeps its scale.
    let other = installed(2);
    fake(|f| {
        f.logs.clear();
        f.generation = 3;
    });
    other.prepare(null_mut());
    fake(|f| f.generation = 4);
    other.prepare(null_mut());
    assert!(!fake(|f| f.logs.iter().any(|l| l.contains("device reset"))));
}

#[cfg(windows)]
#[test]
fn live_options_keep_their_panel_labels_and_ranges() {
    let options = live_options();
    let rows: Vec<_> = options
        .iter()
        .map(|o| {
            let label = unsafe { CStr::from_ptr(o.label) }.to_str().unwrap();
            let bound = o.get.is_some() && o.set.is_some();
            (
                label, o.toggle, o.restart, o.minimum, o.maximum, o.step, o.divisor, bound,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("DLSS input: linear HDR colour", 1, 0, 0, 1, 1, 1, true),
            ("DLSS auto exposure", 1, 0, 0, 1, 1, 1, true),
            ("First-person pixels: zero motion", 1, 0, 0, 1, 1, 1, true),
            (
                "Subpixel jitter (SR needs it; off = no upscaling)",
                1,
                0,
                0,
                1,
                1,
                1,
                true
            ),
            ("Texture mip bias", 1, 0, 0, 1, 1, 1, true),
            ("Mip bias offset", 0, 0, -20, 10, 1, 10, true),
            ("SR fallback scale", 0, 0, 33, 100, 1, 100, true),
            ("SR capture start frame", 0, 0, 0, i32::MAX, 1, 1, true),
            ("SR capture frame count", 0, 0, 0, 32, 1, 1, true),
            ("SR capture interval", 0, 0, 1, i32::MAX, 1, 1, true),
            ("SR camera cut distance (m)", 0, 0, 1, 10000, 1, 10, true),
            ("SR camera cut angle (deg)", 0, 0, 10, 1800, 1, 10, true),
            ("Require signed SR libraries", 1, 1, 0, 1, 1, 1, true),
        ]
    );
    // Each getter reads its own live value of the process stage (the INI defaults until configured).
    let values = options.map(|o| unsafe { o.get.unwrap()() });
    assert_eq!(values, [1, 0, 1, 1, 1, 0, 67, 600, 3, 300, 40, 450, 0]);
    let driver = Stage::new();
    driver.bind();
    let mip = &options[5];
    for value in [mip.minimum, 5, mip.maximum] {
        driver.set_option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET, value);
        assert_eq!(driver.option(RSF_ANOMALY_SR_OPTION_MIP_BIAS_OFFSET), value);
    }
}

// The production Win32 INI callbacks, against a package path on disk.
#[cfg(windows)]
mod win32 {
    use super::*;
    #[test]
    fn scalar_options_through_the_production_win32_reader() {
        use windows_sys::core::w;

        let root = std::env::temp_dir().join(format!("rsf-sr-scalars-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("rsf.ini");
        std::fs::write(&path, "[Anomaly]\r\n").unwrap();
        let units = units(&path);
        let terminated: Vec<u16> = units.iter().copied().chain([0]).collect();
        let stage = Stage::new();
        stage.bind();
        fake(|f| f.production_ini = true);
        let set = |key, value| {
            assert!(unsafe { crate::ini::write_text(terminated.as_ptr(), key, value) });
        };
        let ini = terminated.as_ptr().cast();
        let read = || stage.ini(ini).options();
        let selected = || stage.configure_ini(&units, false, None, null_mut());
        assert!(!selected() && read().stage == 0 && default_config().stage == 0);
        set(w!("SRStage"), w!("1"));
        set(w!("SRStageCapture"), w!("1"));
        set(w!("SRStageCaptureCount"), w!("99"));
        set(w!("SRStageCaptureInterval"), w!("0"));
        let options = read();
        assert!(selected() && options.capture && options.scale_defaulted);
        assert_eq!(
            (
                options.stage,
                options.capture_start,
                options.capture_count,
                options.capture_interval
            ),
            (1, 600, 16, 1)
        );
        assert_eq!(options.scale, 0.67);
        set(w!("SRStage"), w!("0"));
        assert!(selected());
        set(w!("SRStageCapture"), w!("0"));
        set(w!("SRStage"), w!("2"));
        assert!(selected() && read().stage == 2 && read().scale_defaulted);
        for (text, value, defaulted) in [
            (w!("0.5"), 0.5, false),
            (w!("0.77"), 0.77, false),
            (w!("1.0"), 1.0, false),
            (w!(" 0.6 "), 0.6, false),
            (w!("0.25"), 0.33, false),
            (w!("2"), 1.0, false),
            (w!("0"), 0.67, true),
            (w!("-1"), 0.67, true),
            (w!("abc"), 0.67, true),
            (w!("0.7x"), 0.67, true),
            (w!("nan"), 0.67, true),
        ] {
            set(w!("SRScale"), text);
            assert_eq!(stage.ini(ini).scale(), (value, defaulted));
            assert_eq!(read().scale, value);
        }
        assert_eq!(
            (default_config().scale, default_config().scale_defaulted),
            (0.67, true)
        );
        for invalid in [w!("6"), w!("-1"), w!("x")] {
            set(w!("SRStage"), invalid);
            assert!(!selected() && read().stage == 0);
        }
        set(w!("SRStage"), w!("5"));
        let options = read();
        assert!(options.stage == 5 && selected());
        assert!(
            options.first_person_zero
                && !options.exposure_auto
                && !options.require_signature
                && !options.colour_transport
        );
        assert_eq!(
            (
                options.cut_distance,
                options.cut_degrees,
                options.capture_count
            ),
            (4.0, 45.0, 16)
        );
        assert!(options.settings == default_config().settings);
        for (key, value) in [
            (w!("SRExposure"), w!("auto")),
            (w!("SRFirstPersonZero"), w!("0")),
            (w!("SRCutDistance"), w!("0.001")),
            (w!("SRCutAngle"), w!("x")),
            (w!("SR"), w!("0")),
            (w!("SRBackend"), w!("5")),
            (w!("SRQuality"), w!("6")),
            (w!("DLSSPreset"), w!("6")),
            (w!("SRColourTransport"), w!("1")),
            (w!("SRRequireSignature"), w!("1")),
        ] {
            set(key, value);
        }
        let options = read();
        assert!(
            options.exposure_auto
                && !options.first_person_zero
                && options.require_signature
                && options.colour_transport
        );
        assert_eq!((options.cut_distance, options.cut_degrees), (0.1, 45.0));
        assert_eq!(
            (
                options.settings.enabled,
                options.settings.backend,
                options.settings.quality,
                options.settings.preset
            ),
            (false, 5, 1, 6)
        );
        // Ultra Quality is kept; the Runtime decides which backend serves it.
        set(w!("SRQuality"), w!("5"));
        assert_eq!(read().settings.quality, 5);
        set(w!("SRBackend"), w!("8"));
        assert_eq!(read().settings.backend, BACKEND_AUTO);
        // FSR1 is the Runtime's own EASU pass.
        set(w!("SRBackend"), w!("6"));
        assert_eq!(read().settings.backend, 6);
        // FSR2 is served like the other FidelityFX providers.
        set(w!("SRBackend"), w!("2"));
        assert_eq!(read().settings.backend, 2);
        set(w!("SRStage"), w!("4"));
        assert_eq!(read().stage, 4);
        assert!(selected());
        set(w!("SRScale"), w!("0.67"));
        set(w!("SRStage"), w!("3"));
        set(w!("SRStageCaptureCount"), std::ptr::null());
        let options = read();
        assert!(options.stage == 3 && selected() && options.jitter && options.mip_bias);
        assert_eq!((options.mip_bias_offset, options.capture_count), (0.0, 8));
        set(w!("SRStageCaptureCount"), w!("99"));
        set(w!("SRJitter"), w!("0"));
        assert!(read().capture_count == 32 && !read().jitter);
        for (text, value, enabled) in [
            (w!("-0.3"), -0.3, true),
            (w!("0"), 0.0, true),
            (w!(" -1 "), -1.0, true),
            (w!("-5"), -2.0, true),
            (w!("3"), 1.0, true),
            (w!("off"), 0.0, false),
            (w!(" OFF "), 0.0, false),
            (w!("offset"), 0.0, true),
            (w!("abc"), 0.0, true),
            (w!("-0.5x"), 0.0, true),
            (w!("nan"), 0.0, true),
            (w!(""), 0.0, true),
        ] {
            set(w!("SRMipBias"), text);
            assert_eq!(stage.ini(ini).mip_bias(), (value, enabled));
            let options = read();
            assert_eq!(
                (options.mip_bias_offset, options.mip_bias),
                (value, enabled)
            );
        }
        assert_eq!(
            (default_config().mip_bias_offset, default_config().mip_bias),
            (0.0, true)
        );
        set(w!("SRStage"), w!("2"));
        assert_eq!(read().capture_count, 16);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_package_beside_a_real_ini() {
        let root = std::env::temp_dir().join(format!("rsf-sr-config-{}", std::process::id()));
        let package = root.join("ReScaleFrame");
        std::fs::create_dir_all(&package).unwrap();
        let ini = root.join("rsf.ini");
        std::fs::write(&ini, "[Anomaly]\r\nSRStage=5\r\n").unwrap();
        let driver = Stage::new();
        driver.bind();
        fake(|f| f.production_ini = true);
        let path = units(&ini);
        assert!(driver.configure_ini(&path, true, None, null_mut()));
        driver.with(|state| {
            let vendors = &state.paths.vendors;
            assert_eq!(
                [&vendors.streamline, &vendors.fidelityfx, &vendors.xess]
                    .map(|dir| std::path::PathBuf::from(dir.to_str().unwrap())),
                ["streamline", "fidelityfx", "xess"].map(|dir| package.join(dir))
            )
        });
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The stage's leaves under test (`sr_stage::leaves`): the recording fakes above, the burst readback fakes of the
/// staging tests, and refusals for the leaves no test drives.
pub(crate) mod leaves {
    use super::*;
    pub(crate) use super::{
        apply_colour_transport, configured, context, context as depth_texture, controller,
        controller_frame_end as frame_end, create_motion_view, device, device_size, draw_pass,
        driver_bind, driver_boundary, driver_count_prepared, driver_enabled, driver_evaluate,
        driver_fallback, driver_stop, engine_frame, frame_facts, geometry_active, hooks_created,
        host_reset, install_hooks, install_position_slot, loading_registered, motion_target, now,
        persist_int, persist_text, precache_frame, prepare, read_camera, release_view as release,
        remove_hooks, render_target, set_engine, set_rm_near, set_swap_enabled, shutdown,
        temporal_ready, verify_guards,
    };
    pub(crate) use crate::sr_stage::staging::tests::{
        context_device, copy_subresource, immediate_context, map_read, pixel_bytes, unmap,
    };
    pub(crate) use crate::sr_temporal::report_fixture as driver_report;
    use std::ptr::null;

    /// The INI reads: the fake map, or the production Win32 reader for the tests that set `production_ini`.
    pub(crate) unsafe extern "C" fn ini_int(
        ini: *const c_void,
        key: *const c_char,
        fallback: i32,
    ) -> i32 {
        if fake(|f| f.production_ini) {
            unsafe { crate::ini::rsf_anomaly_sr_ini_int(ini, key, fallback) }
        } else {
            unsafe { super::ini_int(ini, key, fallback) }
        }
    }
    pub(crate) unsafe extern "C" fn ini_text(
        ini: *const c_void,
        key: *const c_char,
        fallback: *const c_char,
        out: *mut u16,
        capacity: u32,
    ) -> u32 {
        if fake(|f| f.production_ini) {
            unsafe { crate::ini::rsf_anomaly_sr_ini_text(ini, key, fallback, out, capacity) }
        } else {
            unsafe { super::ini_text(ini, key, fallback, out, capacity) }
        }
    }

    pub(crate) fn close_stale() {
        if fake(|f| f.close_stale) {
            super::close_stale();
        }
    }
    pub(crate) fn timer_retain_device(device: *mut c_void) -> *mut c_void {
        if fake(|f| f.timers) {
            retain_timer(device)
        } else {
            null_mut()
        }
    }
    pub(crate) fn timer_create_query(device: *mut c_void, disjoint: bool) -> *mut c_void {
        if fake(|f| f.timers) {
            create_timer_query(device, disjoint)
        } else {
            null_mut()
        }
    }
    pub(crate) fn timer_issue(context: *mut c_void, query: *mut c_void, begin: bool) {
        if fake(|f| f.timers) {
            issue_timer_query(context, query, begin);
        }
    }
    pub(crate) unsafe fn timer_clock(
        context: *mut c_void,
        query: *mut c_void,
        frequency: *mut u64,
        disjoint: *mut bool,
    ) -> i32 {
        if fake(|f| f.timers) {
            unsafe { super::timer_clock(context, query, frequency, disjoint) }
        } else {
            -1
        }
    }
    pub(crate) unsafe fn timer_timestamp(
        context: *mut c_void,
        query: *mut c_void,
        ticks: *mut u64,
    ) -> i32 {
        if fake(|f| f.timers) {
            unsafe { super::timer_timestamp(context, query, ticks) }
        } else {
            -1
        }
    }

    // No test drives these; they answer as an absent leaf did.
    pub(crate) fn back_buffer() -> *mut c_void {
        null_mut()
    }
    pub(crate) fn build_sha256() -> *const c_char {
        null()
    }
    pub(crate) fn seam_surface(_: *mut c_void) -> *mut c_void {
        null_mut()
    }
    pub(crate) fn prepared() {}
    pub(crate) fn swap_handle() -> *mut c_void {
        null_mut()
    }
    pub(crate) fn combine_end() {}
    pub(crate) fn rm_near() {}
    pub(crate) fn sun_begin() -> bool {
        false
    }
    pub(crate) fn sun_end(_: bool) {}
    pub(crate) fn position_setup(_: *mut c_void) {}
    pub(crate) fn swap_counters() -> rsf_anomaly_sr_swap_counters {
        rsf_anomaly_sr_swap_counters::default()
    }
    pub(crate) fn host_stats() -> (u64, u64) {
        (0, 0)
    }
    pub(crate) fn disable_hooks() {}
    pub(crate) fn driver_output_format() -> u32 {
        0
    }
    pub(crate) fn driver_planned(_: u32, _: u32) -> Option<[u32; 2]> {
        None
    }
    pub(crate) fn driver_request_dump(_: *const c_char) -> bool {
        false
    }
    pub(crate) fn driver_overlay_report(_: *mut c_void) {}
    // The overlay is the only caller of STAGE.request, and its tests fake the stage controls.
    #[allow(dead_code)]
    pub(crate) fn driver_request(_: *const rsf_anomaly_sr_settings) {}
}
