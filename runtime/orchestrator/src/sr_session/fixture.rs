// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::rsf_frame_record;
use std::cell::RefCell;

#[derive(Default)]
struct State {
    closes: u32,
    reset_seen: u32,
    announcements: u32,
    fail_open: bool,
    empty_open: bool,
    fail_plan: bool,
    fail_frame: bool,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn state<T>(action: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| action(&mut state.borrow_mut()))
}

unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    if unsafe { CStr::from_ptr(message) } == c"fixture" {
        state(|s| s.announcements += 1);
    }
}
unsafe extern "C" fn probe(_: *const rsf_backend_probe_desc, _: *mut rsf_backend_caps) -> i32 {
    0
}
unsafe extern "C" fn open(desc: *const rsf_sr_open_desc, session: *mut *mut c_void) -> i32 {
    let desc = unsafe { &*desc };
    unsafe { session.write(ptr::null_mut()) };
    if state(|s| s.fail_open) {
        return -7;
    }
    if state(|s| s.empty_open) {
        return 0;
    }
    if desc.runtime_directory_utf8.is_null()
        || unsafe { CStr::from_ptr(desc.runtime_directory_utf8) } != c"copied-path"
    {
        return INVALID_ARGUMENT;
    }
    unsafe { session.write(Box::into_raw(Box::new(desc.fsr_major)).cast()) };
    0
}
unsafe extern "C" fn plan(_: *mut c_void, _: u32, width: *mut u32, height: *mut u32) -> i32 {
    if state(|s| s.fail_plan) {
        return -9;
    }
    unsafe {
        width.write(64);
        height.write(64);
    }
    0
}
unsafe extern "C" fn evaluate(_: *mut c_void, _: *mut c_void, frame: *const rsf_sr_frame) -> i32 {
    state(|s| {
        s.reset_seen = unsafe { (*frame).reset };
        if s.fail_frame { -9 } else { 0 }
    })
}
unsafe extern "C" fn release(_: *mut c_void) -> i32 {
    0
}
unsafe extern "C" fn close(session: *mut c_void) {
    drop(unsafe { Box::from_raw(session.cast::<u32>()) });
    state(|s| s.closes += 1);
}
unsafe extern "C" fn version(session: *mut c_void, id: *mut u64, name: *mut *const c_char) -> i32 {
    unsafe {
        id.write(u64::from(session.cast::<u32>().read()));
        name.write(c"fixture".as_ptr());
    }
    0
}
static PROVIDER: rsf_sr_provider = rsf_sr_provider {
    struct_size: size_of::<rsf_sr_provider>() as u32,
    probe: Some(probe),
    open: Some(open),
    plan: Some(plan),
    evaluate: Some(evaluate),
    release_resources: Some(release),
    close: Some(close),
    get_version: Some(version),
};

#[unsafe(no_mangle)]
extern "C" fn rsf_sr_session_fixture() -> i32 {
    state(|s| *s = State::default());
    let mut passed = true;
    let mut check = |ok: bool, label: &str| {
        if !ok {
            eprintln!("FAIL: {label}");
            passed = false;
        }
    };
    let mut path = *b"copied-path\0";
    let path_ptr = path.as_ptr().cast();
    let command = ptr::without_provenance_mut(1);
    let mut setup: rsf_sr_session_setup = unsafe { std::mem::zeroed() };
    setup.struct_size = size_of_val(&setup) as u32;
    setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
    setup.open.struct_size = size_of_val(&setup.open) as u32;
    setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.open.api = RSF_API_D3D12;
    setup.open.device = command;
    setup.open.output_width = 128;
    setup.open.output_height = 128;
    setup.open.log = Some(log);
    setup.fsr2_directory_utf8 = path_ptr;
    setup.fsr3_directory_utf8 = path_ptr;
    setup.fsr4_directory_utf8 = path_ptr;
    setup.xess_directory_utf8 = path_ptr;
    let mut session = ptr::null_mut();
    check(
        unsafe { create(&setup, &mut session, &PROVIDER, &PROVIDER) } == 0,
        "create",
    );
    path[0] = b'X';
    std::hint::black_box(&path);
    let select = |backend| unsafe { rsf_sr_session_select(session, backend, 1, 0) };
    let status = || {
        let mut status: rsf_sr_session_status = unsafe { std::mem::zeroed() };
        status.struct_size = size_of_val(&status) as u32;
        unsafe { rsf_sr_session_get_status(session, &mut status) };
        status
    };
    check(
        select(RSF_SR_FSR2) == 0,
        "select FSR2 and use the copied SDK path",
    );
    check(
        state(|s| s.announcements) == 1,
        "announce committed version once",
    );
    state(|s| s.fail_open = true);
    check(select(4) == -7, "refuse FSR4");
    check(
        status().requested == 4 && status().effective == RSF_SR_FSR2 && state(|s| s.closes) == 0,
        "refusal preserves effective backend",
    );
    state(|s| {
        s.fail_open = false;
        s.empty_open = true;
    });
    check(
        select(4) == INIT_FAILED,
        "success without a context refuses the switch",
    );
    check(
        status().effective == RSF_SR_FSR2 && state(|s| s.closes) == 0,
        "missing context preserves effective backend",
    );
    state(|s| {
        s.empty_open = false;
        s.fail_plan = true;
    });
    check(
        select(RSF_SR_XESS) != 0 && state(|s| s.closes) == 1,
        "failed plan closes replacement only",
    );
    check(
        state(|s| s.announcements) == 1,
        "refused candidates never announce an active version",
    );
    state(|s| s.fail_plan = false);
    check(
        select(RSF_SR_XESS) == 0 && state(|s| s.closes) == 2,
        "commit replacement closes original",
    );

    let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
    record.struct_size = size_of_val(&record) as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.frame_id = 1;
    record.session_id = 1;
    record.resource_generation = 1;
    record.render_width = 64;
    record.render_height = 64;
    record.output_width = 128;
    record.output_height = 128;
    record.frame_time_ms = 16.0;
    record.camera.struct_size = size_of_val(&record.camera) as u32;
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    record.camera.near_plane = 0.1;
    record.camera.vertical_fov_radians = 1.0;
    let mut frame: rsf_sr_frame = unsafe { std::mem::zeroed() };
    frame.struct_size = size_of_val(&frame) as u32;
    frame.record = &raw const record;
    frame.pre_exposure = 1.0;
    frame.view_space_to_meters = 1.0;
    frame.motion_scale_x = 1.0;
    frame.motion_scale_y = 1.0;
    let resources = |frame: &mut rsf_sr_frame, generation| {
        for resource in [
            &mut frame.color,
            &mut frame.depth,
            &mut frame.motion,
            &mut frame.output,
        ] {
            resource.struct_size = size_of_val(resource) as u32;
            resource.resource = command;
            resource.generation = generation;
            resource.width = 128;
            resource.height = 128;
        }
    };
    resources(&mut frame, 1);
    let run = |frame: &rsf_sr_frame| unsafe { rsf_sr_session_evaluate(session, command, frame) };
    let warm = |frame: &rsf_sr_frame| unsafe { rsf_sr_session_warm_up(session, command, frame) };
    let reset = || state(|s| s.reset_seen != 0);
    // Refresh the raw record pointer after each edit so Rust retains unique access
    // to the mutable fixture while each synchronous call borrows the current record.
    macro_rules! frame {
        () => {{
            frame.record = &raw const record;
            &frame
        }};
    }
    record.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    check(
        run(frame!()) == ABI_MISMATCH,
        "camera ABI cannot validate the frame",
    );
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    check(
        run(frame!()) == ABI_MISMATCH,
        "frame ABI cannot validate the camera",
    );
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    frame.sharpness = 1.01;
    check(run(frame!()) == INVALID_ARGUMENT, "out-of-range sharpening");
    frame.sharpness = 0.0;
    check(run(frame!()) == 0 && reset(), "first frame reset");
    record.frame_id += 1;
    check(
        run(frame!()) == 0 && !reset(),
        "contiguous frame keeps history",
    );
    record.output_width = 127;
    check(
        warm(frame!()) == STALE_RESOURCES && !reset(),
        "warm-up refuses a mismatched output extent",
    );
    record.output_width = 128;
    check(
        warm(frame!()) == 0 && reset(),
        "warm-up resets without consuming the frame",
    );
    check(run(frame!()) == INVALID_ARGUMENT, "duplicate refused");
    record.frame_id += 1;
    check(
        run(frame!()) == 0 && reset(),
        "history reset after refused input",
    );
    record.frame_id += 2;
    check(run(frame!()) == 0 && reset(), "frame gap reset");
    record.frame_id += 1;
    record.resource_generation += 1;
    check(run(frame!()) == STALE_RESOURCES, "stale generation refused");
    resources(&mut frame, record.resource_generation);
    check(run(frame!()) == 0 && reset(), "new surfaces reset");
    record.frame_id += 1;
    state(|s| s.fail_frame = true);
    check(run(frame!()) == -9, "vendor failure reported");
    record.frame_id += 1;
    state(|s| s.fail_frame = false);
    check(
        run(frame!()) == 0 && reset(),
        "vendor failure invalidates history",
    );
    record.frame_id += 1;
    frame.jitter_x = f32::NAN;
    check(
        run(frame!()) == INVALID_ARGUMENT,
        "nonfinite jitter refused",
    );
    frame.jitter_x = 0.0;
    check(
        unsafe { rsf_sr_session_select(session, 0, 0, 0) } == 0,
        "disable",
    );
    check(run(frame!()) == NOT_READY, "disabled evaluation refused");
    unsafe { rsf_sr_session_destroy(session) };
    check(state(|s| s.closes) == 3, "each session closed once");
    setup.abi_version += 1;
    check(
        unsafe { create(&setup, &mut session, &PROVIDER, &PROVIDER) } == ABI_MISMATCH,
        "wrong ABI",
    );
    i32::from(!passed)
}
