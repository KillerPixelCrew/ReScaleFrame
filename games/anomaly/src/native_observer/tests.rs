// SPDX-License-Identifier: MIT
use super::*;
use std::sync::atomic::AtomicBool;

static INPUT_IDLE: AtomicBool = AtomicBool::new(false);
static MOTION_REFUSED: AtomicBool = AtomicBool::new(false);
static TEARDOWN: Mutex<Vec<&str>> = Mutex::new(Vec::new());

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

#[test]
fn stop_drains_native_and_input_invocations_before_retiring_the_stages() {
    let observer = Observer::new();
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
        ..Platform::default()
    };

    assert!(observer.enter());
    assert_eq!(observer.control(&platform, true), BUSY);
    assert_eq!(observer.status().active, 0);
    assert_eq!(observer.status().prepared, 1);
    assert!(!observer.enter());
    observer.leave(); // A stock-only trampoline is still tracked.
    observer.leave();
    assert_eq!(observer.control(&platform, true), BUSY);
    assert_eq!(*lock(&TEARDOWN), ["input"]);

    INPUT_IDLE.store(true, Ordering::Relaxed);
    MOTION_REFUSED.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(&platform, true), NATIVE_REFUSED);
    assert_eq!(observer.status().prepared, 1);
    assert_eq!(*lock(&TEARDOWN), ["input", "SR", "motion"]);
    MOTION_REFUSED.store(false, Ordering::Relaxed);
    assert_eq!(observer.control(&platform, true), OK);
    assert_eq!(observer.control(&platform, true), OK);
    assert_eq!(observer.status().prepared, 0);
    assert_eq!(
        *lock(&TEARDOWN),
        ["input", "SR", "motion", "motion", "hooks", "geometry"]
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
    let observer = Observer::new();
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
