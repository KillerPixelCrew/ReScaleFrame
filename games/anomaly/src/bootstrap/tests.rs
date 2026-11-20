// SPDX-License-Identifier: MIT
use super::*;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering::SeqCst};

static REGISTERED: AtomicU32 = AtomicU32::new(0);
static SR: AtomicU32 = AtomicU32::new(0);
static MOTION: AtomicU32 = AtomicU32::new(0);
static FG: AtomicU32 = AtomicU32::new(0);
static INPUT: AtomicU32 = AtomicU32::new(0);
static OVERLAY: AtomicU32 = AtomicU32::new(0);
static SINK: AtomicU32 = AtomicU32::new(0);
static PREPARED: AtomicU32 = AtomicU32::new(0);
static STARTED: AtomicU32 = AtomicU32::new(0);
static OVERLAY_ACCEPTS: AtomicBool = AtomicBool::new(false);
static PREPARE_RESULT: AtomicI32 = AtomicI32::new(RSF_OK);
static OPTIONS_VALID: AtomicBool = AtomicBool::new(true);
static SESSION: u8 = 0;

unsafe extern "C" fn label(_: usize) -> *const c_char {
    c"standard".as_ptr()
}
unsafe extern "C" fn sha256(_: *const u16, _: *mut c_char) -> bool {
    false
}
unsafe extern "C" fn register_options(_: *const u16) {
    REGISTERED.fetch_add(1, SeqCst);
}
unsafe extern "C" fn sr(_: *const u16, configure: bool, _: rsf_game_log_fn) -> bool {
    if configure {
        SR.fetch_add(1, SeqCst);
    }
    true
}
unsafe extern "C" fn motion(_: *const u16, _: rsf_game_log_fn) {
    MOTION.fetch_add(1, SeqCst);
}
unsafe extern "C" fn fg(
    _: *const u16,
    configure: bool,
    _: HMODULE,
    _: usize,
    _: rsf_game_log_fn,
) -> bool {
    if configure {
        FG.fetch_add(1, SeqCst);
    }
    true
}
unsafe extern "C" fn fg_installed() -> bool {
    FG.load(SeqCst) != 0
}
unsafe extern "C" fn input(_: usize, _: *mut *const c_char) -> u32 {
    INPUT.fetch_add(1, SeqCst);
    0
}
unsafe extern "C" fn overlay(_: HMODULE, _: *const u16, _: rsf_game_log_fn) -> bool {
    OVERLAY.fetch_add(1, SeqCst);
    OVERLAY_ACCEPTS.load(SeqCst)
}
unsafe extern "C" fn overlay_sink() {
    SINK.fetch_add(1, SeqCst);
}
unsafe extern "C" fn prepare(options: *const SessionOptions, session: *mut *mut c_void) -> i32 {
    PREPARED.fetch_add(1, SeqCst);
    let options = unsafe { &*options };
    let probe = unsafe { &*options.probe };
    let valid = options.struct_size == 128
        && options.abi_version == SESSION_ABI_VERSION
        && !options.plugin_path.is_null()
        && probe.pe_machine == AMD64
        && unsafe { CStr::from_ptr(probe.executable_name_utf8) } == c"AnomalyDX11.exe"
        && options.services.abi_version == RSF_GAME_ABI_VERSION
        && options.services.session_id != 0
        && options.services.log.is_some();
    if !valid {
        OPTIONS_VALID.store(false, SeqCst);
    }
    // Success and a busy refusal both hand back a session the caller must retain.
    unsafe { *session = ptr::addr_of!(SESSION).cast_mut().cast() };
    PREPARE_RESULT.load(SeqCst)
}
unsafe extern "C" fn start(_: *mut c_void) -> i32 {
    STARTED.fetch_add(1, SeqCst);
    RSF_OK
}

fn counts() -> [u32; 9] {
    [
        &REGISTERED,
        &SR,
        &MOTION,
        &FG,
        &INPUT,
        &PREPARED,
        &STARTED,
        &OVERLAY,
        &SINK,
    ]
    .map(|c| c.load(SeqCst))
}

// The C++ entry repeated every cold-start step on a retry, so the overlay listed the restart options (and after a
// refused session, the motion pack row) twice, and reconfigured stages beside a partial Runtime session.
#[test]
fn retry_repeats_only_unfinished_steps() {
    let builds = [BuildIdentity {
        executable: c"AnomalyDX11.exe".as_ptr(),
        sha256: c"00".as_ptr(),
    }];
    let leaves = Leaves {
        builds: builds.as_ptr(),
        build_count: builds.len(),
        label,
        sha256,
        register_options,
        sr,
        motion,
        fg,
        fg_installed,
        input,
        overlay,
        overlay_sink,
    };
    let ini: Vec<u16> = "C:\\anomaly.ini\0".encode_utf16().collect();
    let request = Request {
        ini: &ini,
        build: 0,
        identity: &builds[0],
        observe: true,
        overlay: true,
    };
    let runtime = Runtime {
        module: ptr::null_mut(),
        prepare: Some(prepare),
        start: Some(start),
    };

    let mut entry = Entry::new();
    assert_eq!(
        orchestrate(&mut entry, &leaves, &request, &runtime),
        OVERLAY_REFUSED
    );
    assert_eq!(counts(), [1, 1, 1, 1, 1, 1, 1, 1, 0]);
    OVERLAY_ACCEPTS.store(true, SeqCst);
    assert_eq!(orchestrate(&mut entry, &leaves, &request, &runtime), READY);
    assert!(entry.started && entry.observed);
    assert_eq!(counts(), [1, 1, 1, 1, 1, 1, 1, 2, 1]);
    assert!(OPTIONS_VALID.load(SeqCst));

    PREPARE_RESULT.store(-4, SeqCst);
    let mut entry = Entry::new();
    assert_eq!(
        orchestrate(&mut entry, &leaves, &request, &runtime),
        PREPARE_REFUSED
    );
    assert!(!entry.session.is_null() && !entry.observed);
    assert_eq!(counts(), [2, 2, 2, 2, 2, 2, 1, 2, 1]);
    assert_eq!(
        orchestrate(&mut entry, &leaves, &request, &runtime),
        PREPARE_REFUSED
    );
    assert_eq!(counts(), [2, 2, 2, 2, 2, 2, 1, 2, 1]);
}
