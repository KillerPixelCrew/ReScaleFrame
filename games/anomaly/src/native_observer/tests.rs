// SPDX-License-Identifier: MIT
use super::*;
use std::slice;
use std::sync::atomic::AtomicBool;

static INPUT_IDLE: AtomicBool = AtomicBool::new(false);
static MOTION_REFUSED: AtomicBool = AtomicBool::new(false);
static FG_FRAME_OPEN: AtomicBool = AtomicBool::new(false);
static FG_ENDS: AtomicU32 = AtomicU32::new(0);
static FG_RETIRE_READY: AtomicBool = AtomicBool::new(false);
static TEARDOWN: Mutex<Vec<&str>> = Mutex::new(Vec::new());

unsafe extern "C" fn sample_copy(out: *mut c_void, source: *const c_void, size: usize) -> bool {
    if source.addr() == 1 {
        return false;
    }
    unsafe { ptr::copy_nonoverlapping(source.cast::<u8>(), out.cast::<u8>(), size) };
    true
}

unsafe extern "C" fn accept_sample(_: *const f32, _: *const f32) {}

#[test]
fn camera_copy_refusal_keeps_the_last_complete_sample() {
    let observer = Observer::new(|| false, || true);
    let platform = Platform {
        guarded_copy: Some(sample_copy),
        camera: Some(accept_sample),
        ..Platform::default()
    };
    let view = [1.0; 16];
    let projection = [2.0; 16];
    observer.camera(&platform, view.as_ptr(), projection.as_ptr());
    assert_eq!(observer.observation().camera_updates, 1);
    assert_eq!(observer.observation().camera_finite, 1);

    observer.camera(&platform, projection.as_ptr(), ptr::without_provenance(1));
    let observation = observer.observation();
    assert_eq!(observation.camera_updates, 1);
    assert_eq!(observation.camera_finite, 0);
    assert_eq!(observation.view, view);
    assert_eq!(observation.projection, projection);
}

unsafe extern "C" fn check_selected_hooks(
    image: *mut c_void,
    size: u32,
    sites: *const Site,
    count: usize,
) -> bool {
    let sites = unsafe { slice::from_raw_parts(sites, count) };
    image.addr() == 0x1234
        && size == PROFILES[1].image_size
        && count == HOOK_COUNT
        && sites.iter().enumerate().all(|(i, site)| {
            site.rva == PROFILES[1].sites[i].rva
                && site.expected == PROFILES[1].sites[i].expected
                && site.replacement.addr() == 0x2000 + i
                && site.original.addr() == 0x4000 + i * size_of::<usize>()
        })
}

#[test]
fn verified_profile_stays_with_the_hook_batch() {
    let bindings = std::array::from_fn(|i| Binding {
        replacement: ptr::without_provenance_mut(0x2000 + i),
        original: ptr::without_provenance_mut(0x4000 + i * size_of::<usize>()),
    });
    let mut engine = Engine {
        image: ptr::without_provenance_mut(0x1234),
        build: 1,
        ..Engine::default()
    };
    assert!(unsafe { rsf_anomaly_observer_install(&engine, &bindings, check_selected_hooks) });
    engine.build = PROFILES.len();
    assert!(!unsafe { rsf_anomaly_observer_install(&engine, &bindings, check_selected_hooks) });
}

unsafe extern "C" fn disable_input() -> bool {
    lock(&TEARDOWN).push("input");
    true
}
unsafe extern "C" fn disable_hooks() -> bool {
    lock(&TEARDOWN).push("hooks");
    true
}
unsafe extern "C" fn input_idle() -> bool {
    INPUT_IDLE.load(Ordering::Relaxed)
}
unsafe extern "C" fn quiesce_sr() -> bool {
    lock(&TEARDOWN).push("SR");
    true
}
unsafe extern "C" fn quiesce_motion() -> bool {
    lock(&TEARDOWN).push("motion");
    !MOTION_REFUSED.load(Ordering::Relaxed)
}
unsafe extern "C" fn disable_geometry() -> bool {
    lock(&TEARDOWN).push("geometry");
    true
}
unsafe extern "C" fn sr_frame_end() {}
unsafe extern "C" fn fg_end_before() {
    FG_ENDS.fetch_add(1, Ordering::Relaxed);
}
fn retire_fg_inputs() -> bool {
    if !FG_RETIRE_READY.load(Ordering::Relaxed) {
        return false;
    }
    lock(&TEARDOWN).push("FG inputs");
    true
}

#[test]
fn lifecycle_and_status_refuse_reentry_while_the_owner_is_locked() {
    let observer = Observer::new(|| false, || true);
    let platform = Platform::default();
    let services: rsf_game_host_services = unsafe { std::mem::zeroed() };
    {
        let _control = lock(&observer.control);
        assert_eq!(observer.prepare(&services, &platform), BUSY);
        assert_eq!(observer.start(&platform), BUSY);
        assert_eq!(observer.control(&platform, false), BUSY);
        assert_eq!(observer.control(&platform, true), BUSY);
        assert!(observer.status().is_none());
    }
    assert_eq!(observer.status().unwrap().prepared, 0);
}

#[test]
fn window_facts_share_observer_admission_and_retirement() {
    let observer = Observer::new(|| false, || true);
    let mut reports = 0;
    observer.observe_window(|| reports += 1);
    observer.activity.store(ACTIVE, Ordering::Relaxed);
    observer.observe_window(|| {
        assert_eq!(observer.activity.load(Ordering::Relaxed), ACTIVE | 1);
        observer.activity.fetch_and(!ACTIVE, Ordering::AcqRel);
        reports += 1;
    });
    observer.observe_window(|| reports += 1);
    assert_eq!(reports, 1);
    assert_eq!(observer.activity.load(Ordering::Relaxed), 0);
}

#[test]
fn stop_drains_native_and_input_invocations_before_retiring_the_stages() {
    let observer = Observer::new(|| FG_FRAME_OPEN.load(Ordering::Relaxed), retire_fg_inputs);
    lock(&observer.state).session = Some(Session {
        engine: Engine::default(),
        log: None,
        user: ptr::null_mut(),
    });
    {
        let mut control = lock(&observer.control);
        control.geometry_installed = true;
        control.retirement = Retirement::Live;
    }
    observer.activity.store(ACTIVE, Ordering::Relaxed);
    let platform = Platform {
        disable_input: Some(disable_input),
        disable_hooks: Some(disable_hooks),
        input_idle: Some(input_idle),
        quiesce_sr: Some(quiesce_sr),
        quiesce_motion: Some(quiesce_motion),
        disable_geometry: Some(disable_geometry),
        sr_frame_end: Some(sr_frame_end),
        end_before: Some(fg_end_before),
        ..Platform::default()
    };

    assert!(observer.enter());
    assert_eq!(observer.control(&platform, true), BUSY);
    let status = observer.status().unwrap();
    assert_eq!(status.active, 0);
    assert_eq!(status.prepared, 1);
    assert!(!observer.enter());
    observer.leave(); // A stock-only trampoline is still tracked.
    observer.leave();
    assert_eq!(observer.control(&platform, true), BUSY);
    assert_eq!(*lock(&TEARDOWN), ["input"]);

    INPUT_IDLE.store(true, Ordering::Relaxed);
    FG_FRAME_OPEN.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(&platform, true), BUSY);
    assert_eq!(*lock(&TEARDOWN), ["input"]);
    assert!(!observer.enter());
    assert!(observer.end_before(&platform, false));
    FG_FRAME_OPEN.store(false, Ordering::Relaxed);
    observer.leave();
    assert!(!observer.end_before(&platform, false));
    assert_eq!(FG_ENDS.load(Ordering::Relaxed), 1);

    MOTION_REFUSED.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(&platform, true), NATIVE_REFUSED);
    assert_eq!(observer.status().unwrap().prepared, 1);
    assert_eq!(*lock(&TEARDOWN), ["input", "SR", "motion"]);
    MOTION_REFUSED.store(false, Ordering::Relaxed);
    assert_eq!(observer.control(&platform, true), BUSY);
    assert_eq!(observer.status().unwrap().prepared, 1);
    FG_RETIRE_READY.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(&platform, true), OK);
    assert_eq!(observer.control(&platform, true), OK);
    assert_eq!(observer.status().unwrap().prepared, 0);
    assert_eq!(
        *lock(&TEARDOWN),
        [
            "input",
            "SR",
            "motion",
            "motion",
            "hooks",
            "geometry",
            "FG inputs"
        ]
    );
}

static FRAME_READS: AtomicU32 = AtomicU32::new(0);
static FG_SAMPLE: Mutex<Option<(u32, u32, u32)>> = Mutex::new(None);

unsafe extern "C" fn read_frame(_: *const Engine, out: *mut Frame) -> bool {
    let reads = FRAME_READS.fetch_add(1, Ordering::Relaxed);
    // Write partial data on the second call to exercise the native fault case.
    unsafe {
        out.write(Frame {
            id: 81 + reads,
            width: 1600,
            height: 900,
        });
    }
    reads == 0
}
unsafe extern "C" fn fg_render_begin(id: u32, width: u32, height: u32) {
    *lock(&FG_SAMPLE) = Some((id, width, height));
}

#[test]
fn observation_and_fg_share_one_guarded_source_frame_sample() {
    let observer = Observer::new(|| false, || true);
    lock(&observer.state).session = Some(Session {
        engine: Engine::default(),
        log: None,
        user: ptr::null_mut(),
    });
    let platform = Platform {
        read_frame: Some(read_frame),
        render_begin: Some(fg_render_begin),
        ..Platform::default()
    };

    observer.render_begin(&platform);
    let observed = observer.observation();
    assert_eq!(observed.render_frames, 1);
    assert_eq!(FRAME_READS.load(Ordering::Relaxed), 1);
    assert_eq!(
        lock(&FG_SAMPLE).take(),
        Some((observed.engine_frame, observed.width, observed.height))
    );

    observer.render_begin(&platform);
    let observed = observer.observation();
    assert_eq!(observed.render_frames, 2);
    assert_eq!(
        (observed.engine_frame, observed.width, observed.height),
        (0, 0, 0)
    );
    assert!(lock(&FG_SAMPLE).is_none());
}
