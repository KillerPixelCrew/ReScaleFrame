// SPDX-License-Identifier: MIT
use super::*;
use std::sync::atomic::AtomicBool;

static INPUT_IDLE: AtomicBool = AtomicBool::new(false);
static MOTION_REFUSED: AtomicBool = AtomicBool::new(false);
static FG_FRAME_OPEN: AtomicBool = AtomicBool::new(false);
static FG_ENDS: AtomicU32 = AtomicU32::new(0);
static FG_RETIRE_READY: AtomicBool = AtomicBool::new(false);
static TEARDOWN: Mutex<Vec<&str>> = Mutex::new(Vec::new());

pub(crate) unsafe extern "C" fn sample_copy(
    out: *mut c_void,
    source: *const c_void,
    size: usize,
) -> bool {
    if source.addr() == 1 {
        return false;
    }
    unsafe { ptr::copy_nonoverlapping(source.cast::<u8>(), out.cast::<u8>(), size) };
    true
}

#[test]
fn camera_copy_refusal_keeps_the_last_complete_sample() {
    let observer = Observer::new(|| false, || true);
    let view = [1.0; 16];
    let projection = [2.0; 16];
    observer.camera(view.as_ptr(), projection.as_ptr());
    assert_eq!(observer.observation().camera_updates, 1);
    assert_eq!(observer.observation().camera_finite, 1);

    observer.camera(projection.as_ptr(), ptr::without_provenance(1));
    let observation = observer.observation();
    assert_eq!(observation.camera_updates, 1);
    assert_eq!(observation.camera_finite, 0);
    assert_eq!(observation.view, view);
    assert_eq!(observation.projection, projection);
}

#[test]
fn verified_profile_stays_with_the_hook_batch() {
    let detours = std::array::from_fn(|i| ptr::without_provenance_mut(0x2000 + i));
    let trampolines = ptr::without_provenance_mut(0x4000);
    let (size, sites) = sites(1, &detours, trampolines).unwrap();
    assert_eq!(size, PROFILES[1].image_size);
    assert!(sites.iter().enumerate().all(|(i, site)| {
        site.rva == PROFILES[1].sites[i].rva
            && site.expected == PROFILES[1].sites[i].expected
            && site.replacement.addr() == 0x2000 + i
            && site.original.addr() == 0x4000 + i * size_of::<usize>()
    }));
    assert!(super::sites(PROFILES.len(), &detours, trampolines).is_none());
}

const NO_ENGINE: Engine = Engine {
    image: ptr::null_mut(),
    build: 0,
};
pub(crate) fn disable_input() -> bool {
    lock(&TEARDOWN).push("input");
    true
}
pub(crate) fn disable_hooks() -> bool {
    lock(&TEARDOWN).push("hooks");
    true
}
pub(crate) fn disable_geometry() -> bool {
    lock(&TEARDOWN).push("geometry");
    true
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
    let services: rsf_game_host_services = unsafe { std::mem::zeroed() };
    {
        let _control = lock(&observer.control);
        assert_eq!(observer.prepare(&services), BUSY);
        assert_eq!(observer.start(), BUSY);
        assert_eq!(observer.control(false), BUSY);
        assert_eq!(observer.control(true), BUSY);
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
        engine: NO_ENGINE,
        log: None,
        user: ptr::null_mut(),
    });
    {
        let mut control = lock(&observer.control);
        control.geometry_installed = true;
        control.retirement = Retirement::Live;
    }
    observer.activity.store(ACTIVE, Ordering::Relaxed);

    assert!(observer.enter());
    assert_eq!(observer.control(true), BUSY);
    let status = observer.status().unwrap();
    assert_eq!(status.active, 0);
    assert_eq!(status.prepared, 1);
    assert!(!observer.enter());
    observer.leave(); // A stock-only trampoline is still tracked.
    observer.leave();
    assert_eq!(observer.control(true), BUSY);
    assert_eq!(*lock(&TEARDOWN), ["input"]);

    INPUT_IDLE.store(true, Ordering::Relaxed);
    FG_FRAME_OPEN.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(true), BUSY);
    assert_eq!(*lock(&TEARDOWN), ["input"]);
    assert!(!observer.enter());
    assert!(observer.end_before(false));
    FG_FRAME_OPEN.store(false, Ordering::Relaxed);
    observer.leave();
    assert!(!observer.end_before(false));
    assert_eq!(FG_ENDS.load(Ordering::Relaxed), 1);

    MOTION_REFUSED.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(true), NATIVE_REFUSED);
    assert_eq!(observer.status().unwrap().prepared, 1);
    assert_eq!(*lock(&TEARDOWN), ["input", "SR", "motion"]);
    MOTION_REFUSED.store(false, Ordering::Relaxed);
    assert_eq!(observer.control(true), BUSY);
    assert_eq!(observer.status().unwrap().prepared, 1);
    FG_RETIRE_READY.store(true, Ordering::Relaxed);
    assert_eq!(observer.control(true), OK);
    assert_eq!(observer.control(true), OK);
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

pub(crate) fn read_frame(_: &Engine) -> Option<Frame> {
    // The second read faults.
    let reads = FRAME_READS.fetch_add(1, Ordering::Relaxed);
    (reads == 0).then_some(Frame {
        id: 81,
        width: 1600,
        height: 900,
    })
}

#[test]
fn observation_and_fg_share_one_guarded_source_frame_sample() {
    let observer = Observer::new(|| false, || true);
    lock(&observer.state).session = Some(Session {
        engine: NO_ENGINE,
        log: None,
        user: ptr::null_mut(),
    });

    observer.render_begin();
    let observed = observer.observation();
    assert_eq!(observed.render_frames, 1);
    assert_eq!(FRAME_READS.load(Ordering::Relaxed), 1);
    assert_eq!(
        lock(&FG_SAMPLE).take(),
        Some((observed.engine_frame, observed.width, observed.height))
    );

    observer.render_begin();
    let observed = observer.observation();
    assert_eq!(observed.render_frames, 2);
    assert_eq!(
        (observed.engine_frame, observed.width, observed.height),
        (0, 0, 0)
    );
    assert!(lock(&FG_SAMPLE).is_none());
}

/// The Rust stages: the teardown order and FG end count of the stop test, the frame sample of
/// the shared-source test, and no-op boundaries.
pub(super) mod stages {
    use super::*;

    pub(crate) fn quiesce_sr() -> bool {
        lock(&TEARDOWN).push("SR");
        true
    }
    pub(crate) fn quiesce_motion() -> bool {
        lock(&TEARDOWN).push("motion");
        !MOTION_REFUSED.load(Ordering::Relaxed)
    }
    pub(crate) fn input_idle() -> bool {
        INPUT_IDLE.load(Ordering::Relaxed)
    }
    pub(crate) fn sr_frame_end() {}
    pub(crate) fn end_before() {
        FG_ENDS.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn end_after() {}
    pub(crate) fn render_begin(id: u32, width: u32, height: u32) {
        *lock(&FG_SAMPLE) = Some((id, width, height));
    }
    pub(crate) unsafe fn camera(_: *const f32, _: *const f32) {}
    pub(crate) fn ui_boundary() {}
    pub(crate) fn scope_frame() {}
    pub(crate) fn device_reset() {}
}
/// The C leaves under test: the stop test's teardown record, the camera test's refusing copy and the shared-source
/// test's frame read. No test reaches the others.
pub(super) mod leaves {
    use super::*;
    pub(crate) use super::{
        disable_geometry, disable_hooks, disable_input, read_frame, sample_copy as guarded_copy,
    };

    pub(crate) fn enable_hooks(_: &Engine) -> bool {
        unreachable!("observer hooks in a unit test")
    }
    pub(crate) fn install_geometry(_: &Engine) -> bool {
        unreachable!("geometry install in a unit test")
    }
    pub(crate) fn install_sr(_: &Engine) -> bool {
        unreachable!("SR install in a unit test")
    }
    pub(crate) fn install_motion(_: &Engine) -> bool {
        unreachable!("motion install in a unit test")
    }
    pub(crate) fn enable_input() -> bool {
        unreachable!("input slot in a unit test")
    }
}
