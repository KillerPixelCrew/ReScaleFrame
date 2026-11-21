// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn vendor_present_delta_requires_one_generation_and_increasing_totals() {
    let previous = PresentTotal {
        generation: 1,
        count: 1000,
    };
    let sample = |generation, count| PresentTotal { generation, count };
    assert_eq!(sample(1, 1240).delta(previous), Some(240));
    assert_eq!(sample(1, 999).delta(previous), None);
    assert_eq!(sample(2, 120).delta(previous), None);
    assert_eq!(sample(2, 2000).delta(previous), None);
}

#[test]
fn changing_scope_hold_applies_to_the_next_source_without_resetting_history() {
    use crate::fg_policy_abi::{RSF_ANOMALY_RESET_SCOPE_END, RSF_ANOMALY_SKIP_SCOPE};

    let mut policy = Policy::new(4);
    let extent = [1280, 720];
    policy.begin(10, extent, false);
    policy.ui_boundary();
    policy.inputs(true);
    policy.presented();
    assert_eq!(policy.end().skip, RSF_ANOMALY_SKIP_NONE);

    policy.begin(11, extent, false);
    policy.scope();
    policy.ui_boundary();
    policy.presented();
    assert_eq!(policy.end().skip, RSF_ANOMALY_SKIP_SCOPE);
    assert_eq!(policy.begin(12, extent, false).skip, RSF_ANOMALY_SKIP_SCOPE);
    policy.set_scope_hold(1);
    assert_eq!(policy.current().skip, RSF_ANOMALY_SKIP_SCOPE);
    policy.ui_boundary();
    policy.presented();
    policy.end();

    let resumed = policy.begin(13, extent, false);
    assert_eq!(resumed.skip, RSF_ANOMALY_SKIP_NONE);
    assert_eq!(resumed.reset, RSF_ANOMALY_RESET_SCOPE_END);
}

#[test]
fn sr_off_motion_requires_the_current_engine_frame_and_view() {
    let render = Render {
        native_frame: 7,
        extent: [1280, 720],
        ..Default::default()
    };
    let texture = ptr::dangling_mut::<u8>().cast();
    let mut target = rsf_anomaly_fg_motion_target {
        texture,
        width: 1280,
        height: 720,
        frame: 6,
        written: true,
        main: true,
    };
    assert!(render.motion_input(target).is_null());
    target.frame = 7;
    assert_eq!(render.motion_input(target), texture);
    target.main = false;
    assert!(render.motion_input(target).is_null());
}
use crate::fg_inputs_abi::rsf_anomaly_input_platform;
use std::cell::{Cell, RefCell};

#[test]
fn fg_settings_keep_option_bounds_and_normalize_only_vendor_paths() {
    let terminated = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<_>>();
    let streamline = terminated(r"\\?\D:\RSF/streamline\");
    let fidelityfx = terminated(r"\\?\UNC\server\share/fidelityfx\");
    let capture = terminated(r"\\?\D:\RSF\capture");
    let native = rsf_anomaly_fg_config {
        bridge: true,
        enabled: true,
        backend: 42,
        generated: 9,
        latency: 3,
        frame_limit_us: u32::MAX,
        scope_hold: 0,
        capture: true,
        capture_count: 999,
        capture_interval: 0,
        streamline_dir: streamline.as_ptr(),
        fidelityfx_dir: fidelityfx.as_ptr(),
        capture_dir: capture.as_ptr(),
        ..Default::default()
    };
    let options = Options::from_native(&native);
    let frontend = options.frontend;
    assert_eq!(
        (
            options.bridge,
            frontend.enabled,
            frontend.backend,
            frontend.generated
        ),
        (true, 1, RSF_FG_BACKEND_AUTO, 3)
    );
    assert_eq!(
        (
            frontend.latency,
            frontend.frame_limit_us,
            options.scope_hold
        ),
        (RSF_REFLEX_BOOST, 1_000_000, 1)
    );
    assert_eq!(options.streamline.as_c_str(), c"D:\\RSF\\streamline");
    assert_eq!(
        options.fidelityfx.as_c_str(),
        c"\\\\server\\share\\fidelityfx"
    );
    let capture = options.capture.into_inner().unwrap();
    assert_eq!((capture.settings.count, capture.settings.interval), (16, 1));
    assert_eq!(capture.directory.as_c_str(), c"\\\\?\\D:\\RSF\\capture");
}

#[test]
fn live_capture_rearms_without_spending_budget_on_refused_or_old_jobs() {
    use super::options::Capture;

    let mut settings = CaptureSettings {
        enabled: false,
        start: 10,
        count: 1,
        interval: 4,
    };
    let mut capture = Capture::new(c"capture".to_owned(), settings);
    assert!(capture.claim(10).is_none());
    settings.enabled = true;
    capture.configure(settings);
    assert!(capture.claim(9).is_none());
    let (_, first) = capture.claim(10).unwrap();
    assert!(capture.claim(10).is_none());
    capture.finish(10, first, false);
    assert_eq!(capture.total, 0);
    let (_, old_run) = capture.claim(11).unwrap();

    settings.enabled = false;
    capture.configure(settings);
    settings.enabled = true;
    capture.configure(settings);
    assert!(capture.claim(12).is_none());
    capture.finish(11, old_run, true);
    let (_, rearmed) = capture.claim(12).unwrap();
    capture.finish(12, rearmed, true);
    assert!(capture.claim(20).is_none());

    settings.count = 2;
    capture.configure(settings);
    assert!(capture.claim(13).is_none());
    settings.interval = 1;
    capture.configure(settings);
    let (_, edited) = capture.claim(13).unwrap();
    capture.finish(13, edited, true);
    assert_eq!(capture.total, 3);
    assert!(capture.claim(14).is_none());
}

#[test]
fn camera_keeps_row_vector_previous_clip_and_cut_threshold() {
    let projection = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 0.0, 1.01, 1.0, 0.0, 0.0, -0.101, 0.0,
    ];
    let camera = |x: f32| {
        let view = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, 0.0, 0.0, 1.0,
        ];
        Camera::new(view, projection).unwrap()
    };
    let previous = camera(0.0);
    assert!(!camera(4.0).is_cut_from(previous));
    assert!(camera(4.01).is_cut_from(previous));
    let (frame, inverse) = camera(2.0).frame(previous, [1920, 1080], 0.016).unwrap();
    assert!((frame.clip_to_previous_clip[8] + 2.0 / 0.101).abs() < 0.001);
    assert!((frame.clip_to_previous_clip[12] - 20.0).abs() < 0.001);
    assert!((inverse[12] + frame.clip_to_previous_clip[12]).abs() < 0.001);
    assert!((frame.near_plane - 0.10).abs() < 0.00001);
    assert!((frame.far_plane - 10.1).abs() < 0.0001);
    assert_eq!((frame.render_width, frame.output_height), (1920, 1080));
    assert!(Camera::new([f32::NAN; 16], projection).is_none());
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Cpu(u32, u64),
    Pass(u32, bool, u64, u64, u32),
    Source(u64),
    Release(usize),
}

thread_local! {
    static CALLS: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
    static BACKBUFFER_AVAILABLE: Cell<bool> = const { Cell::new(false) };
}

fn record(call: Call) {
    CALLS.with(|calls| calls.borrow_mut().push(call));
}

const CHAIN: Raw = 1usize as Raw;
const CONTEXT: Raw = 2usize as Raw;
const BUFFER: Raw = 3usize as Raw;

unsafe extern "C" fn install_mock(_: *const rsf_d3d11_present_setup) -> i32 {
    1
}
unsafe extern "C" fn is_owner(chain: Raw) -> i32 {
    i32::from(chain == CHAIN)
}
unsafe extern "C" fn has_owner() -> i32 {
    1
}
unsafe extern "C" fn backend() -> u32 {
    0
}
unsafe extern "C" fn set_log(_: rsf_backend_log_fn, _: Raw) {}
unsafe extern "C" fn set_options(_: *const rsf_native_fg_options) {}
unsafe extern "C" fn status(_: *mut rsf_native_fg_status) -> i32 {
    0
}
unsafe extern "C" fn set_ordering(_: u32) {}
unsafe extern "C" fn cpu(event: *const rsf_game_cpu_event) -> i32 {
    let event = unsafe { &*event };
    record(Call::Cpu(event.stage, event.source_frame_id));
    1
}
unsafe extern "C" fn pass(context: Raw, pass: *const Pass, begin: u32) -> i32 {
    let pass = unsafe { &*pass };
    if pass.role == RSF_GAME_RENDER_FINAL_SCENE
        && (context != CONTEXT || pass.scene_surface != BUFFER)
    {
        return 0;
    }
    record(Call::Pass(
        pass.role,
        begin != 0,
        pass.source_frame_id,
        pass.submission_id,
        pass.flags,
    ));
    1
}
unsafe extern "C" fn source(pass: *const Pass) {
    record(Call::Source(
        unsafe { pass.as_ref() }.map_or(0, |p| p.source_frame_id),
    ));
}
unsafe extern "C" fn capture(_: Raw, _: *const Pass, _: *const rsf_dlss_frame) {}
unsafe extern "C" fn ready(_: u64) -> i32 {
    1
}
unsafe extern "C" fn dump(_: Raw, _: u64, _: *const c_char, _: *const c_char) -> i32 {
    0
}
unsafe extern "C" fn prepare(_: Raw, _: Raw, _: Raw, _: Raw, _: Raw, _: Raw, _: Raw, _: u32) {}
unsafe extern "C" fn retire(_: Raw, _: Raw) {}
unsafe extern "C" fn present(_: Raw, _: *const rsf_observer_present_event) {}
unsafe extern "C" fn publish(_: *const rsf_anomaly_fg_overlay_link) {}
unsafe extern "C" fn immediate(_: Raw) -> Raw {
    CONTEXT
}
unsafe extern "C" fn backbuffer(_: Raw) -> Raw {
    if BACKBUFFER_AVAILABLE.get() {
        BUFFER
    } else {
        ptr::null_mut()
    }
}
unsafe extern "C" fn output_window(_: Raw, window: *mut usize) -> bool {
    unsafe { window.write(4) };
    true
}
unsafe extern "C" fn release(object: Raw) {
    record(Call::Release(object as usize));
}

#[test]
fn missing_hudless_backbuffer_cannot_advance_fg_history() {
    let bridge = rsf_d3d11_present_api {
        struct_size: size_of::<rsf_d3d11_present_api>() as u32,
        abi_version: RSF_D3D11_PRESENT_API_VERSION,
        install: Some(install_mock),
        is_owner: Some(is_owner),
        has_owner: Some(has_owner),
        backend: Some(backend),
        backend_choices: Some(backend),
        ..Default::default()
    };
    let fg = rsf_d3d11_fg_api {
        struct_size: size_of::<rsf_d3d11_fg_api>() as u32,
        abi_version: RSF_D3D11_FG_API_VERSION,
        set_log: Some(set_log),
        options: Some(set_options),
        status: Some(status),
        reflex_ordering: Some(set_ordering),
        cpu: Some(cpu),
        pass: Some(pass),
        bind_source: Some(source),
        capture: Some(capture),
        inputs_ready: Some(ready),
        dump_inputs: Some(dump),
        prepare: Some(prepare),
        retire: Some(retire),
        present_event: Some(present),
        latency_event: Some(present),
    };
    assert!(
        Tables::bind(
            rsf_d3d11_present_api {
                struct_size: 8,
                ..bridge
            },
            fg
        )
        .is_none()
    );
    assert!(
        Tables::bind(
            bridge,
            rsf_d3d11_fg_api {
                capture: None,
                ..fg
            }
        )
        .is_none()
    );
    // This mock never falls back to SR-off input dispatch. Only the borrowed Runtime data and the
    // synthetic immediate-context/backbuffer leases cross the production stage boundary.
    static INPUTS: rsf_anomaly_input_platform = rsf_anomaly_input_platform {
        release_interface: Some(release),
        device: None,
        describe: None,
        same_device: None,
        build: None,
        release_pipeline: None,
        targets: None,
        release_targets: None,
        motion_view: None,
        dispatch: None,
        first_person_limit: 0.02,
        validity_threshold: 0.5,
    };
    let platform = Box::leak(Box::new(Platform {
        inputs: &INPUTS,
        publish_overlay: Some(publish),
        immediate_context: Some(immediate),
        backbuffer: Some(backbuffer),
        output_window: Some(output_window),
        ..Default::default()
    }));
    rsf_anomaly_fg_configure(
        &rsf_anomaly_fg_config {
            bridge: true,
            ..Default::default()
        },
        None,
        ptr::null_mut(),
    );
    assert!(rsf_anomaly_fg_install_with(&bridge, &fg, platform));
    let mut event = rsf_observer_present_event {
        struct_size: size_of::<rsf_observer_present_event>() as u32,
        swapchain: CHAIN,
        ..Default::default()
    };
    unsafe { present_event(ptr::null_mut(), &event) };
    let run_frame = |native_frame, event: &mut rsf_observer_present_event| {
        rsf_anomaly_fg_stage_input_begin(native_frame);
        rsf_anomaly_fg_stage_render_begin(native_frame, 1280, 720);
        rsf_anomaly_fg_stage_ui_boundary();
        rsf_anomaly_fg_stage_end_before();
        event.completed = 1;
        unsafe { present_event(ptr::null_mut(), event) };
        rsf_anomaly_fg_stage_end_after();
        let status = rsf_anomaly_fg_stage_status();
        (status.presented, status.fed)
    };
    assert_eq!(run_frame(100, &mut event), (1, 0));
    assert_eq!(
        STAGE
            .get()
            .unwrap()
            .engine
            .lock()
            .unwrap()
            .policy
            .counters()
            .skipped[4],
        1
    );
    CALLS.with(|calls| calls.borrow_mut().clear());
    BACKBUFFER_AVAILABLE.set(true);
    assert_eq!(run_frame(101, &mut event), (2, 1));
    let flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_RESET;
    let pass = |role, begin| Call::Pass(role, begin, 2, 2, flags);
    let cpu = |role| Call::Cpu(role, 2);
    CALLS.with(|calls| {
        assert_eq!(
            calls.borrow().as_slice(),
            &[
                cpu(RSF_GAME_CPU_FRAME_BEGIN),
                cpu(RSF_GAME_CPU_INPUT_SAMPLE),
                cpu(RSF_GAME_CPU_SIMULATION_BEGIN),
                cpu(RSF_GAME_CPU_SIMULATION_END),
                pass(RSF_GAME_RENDER_FRAME, true),
                pass(RSF_GAME_RENDER_SUBMISSION, true),
                Call::Source(2),
                pass(RSF_GAME_RENDER_FINAL_SCENE, true),
                pass(RSF_GAME_RENDER_FINAL_SCENE, false),
                Call::Release(BUFFER as usize),
                Call::Release(CONTEXT as usize),
                pass(RSF_GAME_RENDER_SUBMISSION, false),
                Call::Source(0),
                pass(RSF_GAME_RENDER_WINDOW, true),
                pass(RSF_GAME_RENDER_WINDOW, false),
                pass(RSF_GAME_RENDER_FRAME, false),
                cpu(RSF_GAME_CPU_FRAME_END),
            ]
        )
    });
}
