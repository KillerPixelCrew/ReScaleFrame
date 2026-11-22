// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_game_sys::rsf_frame_record;
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    ptr,
    sync::{Condvar, Mutex},
    time::Duration,
};

struct Fixture(UnsafeCell<rsf_dispatch_fixture_state>);
// The scenario serializes provider calls. During the one blocked sleep it only
// probes the runtime's borrow gates, which must refuse before calling the fake.
unsafe impl Sync for Fixture {}
static STATE: Fixture = Fixture(UnsafeCell::new(unsafe { std::mem::zeroed() }));

pub(super) fn fixture_state() -> *mut rsf_dispatch_fixture_state {
    STATE.0.get()
}

pub(super) fn initialize() {
    let state = rsf_dispatch_fixture_state {
        generated_count: 1,
        xell: rsf_fgvk_latency_status {
            struct_size: size_of::<rsf_fgvk_latency_status>() as u32,
            abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
            available: 1,
            mode: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe { fixture_state().write(state) };
}

macro_rules! f {
    ($field:ident) => {
        (*fixture_state()).$field
    };
}

const FFX: usize = 33;
const DLSS: usize = 11;
const FSR4: usize = 44;
const XESS: usize = 55;
const SUBOPTIMAL: i32 = 1_000_001_003;
const OUT_OF_DATE: i32 = -1_000_001_004;

macro_rules! frame_callbacks {
    ($owner:ty, $prepare:ident, $hudless:ident, $submitted:ident, $present:ident, $source:ident, $bits:literal, $result:expr) => {
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $prepare(
            _: *mut $owner,
            _: *mut c_void,
            _: *const rsf_fgvk_frame,
        ) -> i32 {
            unsafe { f!(calls) |= $bits };
            FIXTURE_OK
        }
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $hudless(
            _: *mut $owner,
            _: *mut c_void,
            _: *const rsf_fgvk_frame,
        ) -> i32 {
            unsafe { f!(calls) |= $bits << 1 };
            FIXTURE_OK
        }
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $submitted(_: *mut $owner, _: *const *mut c_void, _: u32) {
            unsafe { f!(calls) |= $bits << 2 };
        }
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $present(_: *mut $owner, _: *const c_void) -> i32 {
            unsafe { f!(calls) |= $bits << 3 };
            $result
        }
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $source(
            owner: *mut $owner,
            info: *const c_void,
            source: *const rsf_frame_record,
        ) -> i32 {
            unsafe {
                f!(presented_source) = source;
                f!(explicit_presents) += 1;
                $present(owner, info)
            }
        }
    };
}

frame_callbacks!(
    rsf_ffxgvk,
    rsf_ffxgvk_prepare,
    rsf_ffxgvk_hudless,
    rsf_ffxgvk_submitted,
    rsf_ffxgvk_present,
    rsf_ffxgvk_present_source,
    1,
    SUBOPTIMAL
);
frame_callbacks!(
    rsf_dlssgvk,
    rsf_dlssgvk_prepare,
    rsf_dlssgvk_hudless,
    rsf_dlssgvk_submitted,
    rsf_dlssgvk_present,
    rsf_dlssgvk_present_source,
    16,
    OUT_OF_DATE
);
frame_callbacks!(
    rsf_xessgvk,
    rsf_xessgvk_prepare,
    rsf_xessgvk_hudless,
    rsf_xessgvk_submitted,
    rsf_xessgvk_present,
    rsf_xessgvk_present_source,
    256,
    SUBOPTIMAL
);
frame_callbacks!(
    rsf_fsr4gvk,
    rsf_fsr4gvk_prepare,
    rsf_fsr4gvk_hudless,
    rsf_fsr4gvk_submitted,
    rsf_fsr4gvk_present,
    rsf_fsr4gvk_present_source,
    4096,
    SUBOPTIMAL
);

unsafe fn created<T>(owner: usize, out: *mut *mut T, chain: *mut u64) {
    unsafe {
        out.write(ptr::without_provenance_mut(owner));
        chain.write(owner as u64);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ffxgvk_create(
    setup: *const rsf_fgvk_setup,
    out: *mut *mut rsf_ffxgvk,
    chain: *mut u64,
) -> i32 {
    unsafe {
        f!(ffx_creates) += 1;
        f!(borrowed_setup) = setup;
        if f!(refuse_create) {
            return FIXTURE_INIT_FAILED;
        }
        chain.write(FFX as u64);
        if f!(missing_owner) {
            return FIXTURE_OK;
        }
        created(FFX, out, chain);
        if f!(fail_after_create) {
            FIXTURE_INIT_FAILED
        } else {
            FIXTURE_OK
        }
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlssgvk_create(
    setup: *const rsf_fgvk_setup,
    out: *mut *mut rsf_dlssgvk,
    chain: *mut u64,
) -> i32 {
    unsafe {
        f!(dlss_creates) += 1;
        f!(borrowed_setup) = setup;
        created(DLSS, out, chain);
    }
    FIXTURE_OK
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4gvk_create(
    setup: *const rsf_fgvk_setup,
    options: &rsf_fsr4gvk_options,
    out: *mut *mut rsf_fsr4gvk,
    chain: *mut u64,
) -> i32 {
    assert_eq!(options.struct_size as usize, size_of_val(options));
    assert_eq!(
        (
            options.abi_version,
            options.window as usize,
            options.timeline_enabled,
            options.generated_count
        ),
        (1, 17, 1, 1)
    );
    unsafe {
        assert_eq!(*(*setup).graphics.runtime_directory_utf8 as u8, b'F');
        f!(fsr4_creates) += 1;
        f!(borrowed_setup) = setup;
        let result = f!(fsr4_create_result);
        if result == 0 || f!(fsr4_partial) {
            created(FSR4, out, chain);
        }
        result
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessgvk_create(
    setup: *const rsf_fgvk_setup,
    options: &rsf_xessgvk_options,
    out: *mut *mut rsf_xessgvk,
    chain: *mut u64,
) -> i32 {
    assert_eq!(
        (
            options.window as usize,
            options.timeline_enabled,
            options.generated_count
        ),
        (17, 1, 3)
    );
    unsafe {
        assert_eq!(*options.intel_runtime_directory, u16::from(b'D'));
        f!(xess_creates) += 1;
        f!(borrowed_setup) = setup;
        created(XESS, out, chain);
    }
    FIXTURE_OK
}

macro_rules! images {
    ($name:ident, $type:ty, $id:expr, $count:expr) => {
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $name(owner: *mut $type, count: *mut u32, _: *mut u64) -> i32 {
            assert_eq!(owner as usize, $id);
            unsafe { count.write($count) };
            FIXTURE_OK
        }
    };
}
images!(rsf_ffxgvk_images, rsf_ffxgvk, FFX, 3);
images!(rsf_dlssgvk_images, rsf_dlssgvk, DLSS, 4);
images!(rsf_fsr4gvk_images, rsf_fsr4gvk, FSR4, 3);
images!(rsf_xessgvk_images, rsf_xessgvk, XESS, 3);

macro_rules! acquire {
    ($name:ident, $type:ty, $verify:expr, $index:expr, $result:expr) => {
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $name(
            _: *mut $type,
            timeout: u64,
            semaphore: u64,
            fence: u64,
            index: *mut u32,
        ) -> i32 {
            if $verify {
                assert_eq!((timeout, semaphore, fence), (123, 4, 5));
            }
            unsafe { index.write($index) };
            $result
        }
    };
}
acquire!(rsf_ffxgvk_acquire, rsf_ffxgvk, true, 2, 2);
acquire!(rsf_dlssgvk_acquire, rsf_dlssgvk, false, 1, SUBOPTIMAL);
acquire!(rsf_fsr4gvk_acquire, rsf_fsr4gvk, true, 1, 2);
acquire!(rsf_xessgvk_acquire, rsf_xessgvk, false, 2, 2);

fn enabled(value: u32, count: u32) -> i32 {
    unsafe {
        f!(enabled) = value;
        f!(generated_count) = count;
    }
    FIXTURE_OK
}
#[unsafe(no_mangle)]
extern "C" fn rsf_ffxgvk_enable(_: *mut rsf_ffxgvk, value: u32) -> i32 {
    enabled(value, 1)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_dlssgvk_enable(_: *mut rsf_dlssgvk, value: u32, count: u32) -> i32 {
    if count > 3 {
        FIXTURE_NOT_SUPPORTED
    } else {
        enabled(value, count)
    }
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fsr4gvk_enable_options(_: *mut rsf_fsr4gvk, value: u32, count: u32) -> i32 {
    assert_eq!(count, 1);
    enabled(value, count)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_enable_options(_: *mut rsf_xessgvk, value: u32, count: u32) -> i32 {
    if count > 5 {
        -77
    } else {
        enabled(value, count)
    }
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fsr4gvk_max_generated(_: *const rsf_fsr4gvk) -> u32 {
    1
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_max_generated(_: *const rsf_xessgvk) -> u32 {
    5
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlssgvk_image_layout(_: *const rsf_dlssgvk, layout: *mut u32) -> i32 {
    unsafe { layout.write(1_000_001_002) };
    FIXTURE_OK
}

macro_rules! status {
    ($name:ident, $type:ty, $prepared:expr, $error:expr) => {
        #[unsafe(no_mangle)]
        extern "C" fn $name(_: *mut $type, status: &mut rsf_fgvk_status) -> i32 {
            status.prepared = $prepared;
            if let Some(error) = $error {
                status.result = error;
            }
            FIXTURE_OK
        }
    };
}
status!(rsf_ffxgvk_get_status, rsf_ffxgvk, 30, None);
status!(rsf_fsr4gvk_status, rsf_fsr4gvk, 40, Some(-77));
status!(rsf_xessgvk_status, rsf_xessgvk, 50, Some(-77));

#[unsafe(no_mangle)]
extern "C" fn rsf_dlssgvk_get_status(_: *mut rsf_dlssgvk, status: &mut rsf_dlssgvk_status) -> i32 {
    if unsafe { f!(refuse_state) } {
        return FIXTURE_NOT_READY;
    }
    status.presentation = rsf_fgvk_status {
        struct_size: size_of::<rsf_fgvk_status>() as u32,
        abi_version: RSF_FGVK_ABI_VERSION,
        prepared: 10,
        ..Default::default()
    };
    status.max_generated_frames = 3;
    FIXTURE_OK
}

fn refusal(error: i32) -> i32 {
    if unsafe { f!(refuse_destroy) } {
        error
    } else {
        FIXTURE_OK
    }
}
fn retire() -> i32 {
    let result = refusal(FIXTURE_NOT_READY);
    if result == FIXTURE_OK {
        unsafe { f!(destroys) += 1 };
    }
    result
}
#[unsafe(no_mangle)]
extern "C" fn rsf_ffxgvk_drain(_: *mut rsf_ffxgvk) -> i32 {
    if unsafe { f!(fail_drain) } {
        let result = unsafe { rsf_core_fgvk_presentation_shutdown_begin() };
        if result == FIXTURE_OK {
            unsafe { rsf_core_fgvk_presentation_shutdown_end(0) };
        }
        assert_ne!(
            result, FIXTURE_OK,
            "latency SDK shutdown cannot overlap a provider drain"
        );
        return FIXTURE_FEATURE_FAILED;
    }
    refusal(FIXTURE_NOT_READY)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_dlssgvk_drain(_: *mut rsf_dlssgvk) -> i32 {
    refusal(FIXTURE_NOT_READY)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_ffxgvk_destroy(_: *mut rsf_ffxgvk) -> i32 {
    retire()
}
#[unsafe(no_mangle)]
extern "C" fn rsf_dlssgvk_destroy(_: *mut rsf_dlssgvk) -> i32 {
    retire()
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fsr4gvk_destroy(owner: *mut rsf_fsr4gvk) -> i32 {
    assert_eq!(owner as usize, FSR4);
    refusal(-77)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fsr4gvk_drain(_: *mut rsf_fsr4gvk) -> i32 {
    refusal(-77)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_destroy(_: *mut rsf_xessgvk) -> i32 {
    refusal(-77)
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_drain(_: *mut rsf_xessgvk) -> i32 {
    refusal(2)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sleep {
    Idle,
    Armed,
    Entered,
}
static SLEEP: Mutex<Sleep> = Mutex::new(Sleep::Idle);
static CHANGED: Condvar = Condvar::new();

pub(super) fn block_sleep() {
    *SLEEP.lock().unwrap() = Sleep::Armed;
}
pub(super) fn wait_sleep() -> bool {
    let (sleep, _) = CHANGED
        .wait_timeout_while(SLEEP.lock().unwrap(), Duration::from_secs(2), |state| {
            *state != Sleep::Entered
        })
        .unwrap();
    *sleep == Sleep::Entered
}
pub(super) fn release_sleep() {
    *SLEEP.lock().unwrap() = Sleep::Idle;
    CHANGED.notify_all();
}
fn latency_result() -> i32 {
    unsafe {
        let result = if f!(refuse_latency) { -77 } else { FIXTURE_OK };
        f!(xell).result = result;
        result
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_latency_options(_: *mut rsf_xessgvk, mode: u32, interval: u32) -> i32 {
    assert_eq!(interval, 5000);
    let result = latency_result();
    if result == FIXTURE_OK {
        unsafe { f!(xell).mode = mode };
    }
    result
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_sleep(_: *mut rsf_xessgvk, frame: u32) -> i32 {
    assert_eq!(frame, 42);
    let result = latency_result();
    if result == FIXTURE_OK {
        unsafe { f!(xell).sleep_calls += 1 };
    }
    let mut sleep = SLEEP.lock().unwrap();
    if *sleep == Sleep::Armed {
        *sleep = Sleep::Entered;
        CHANGED.notify_all();
        drop(
            CHANGED
                .wait_while(sleep, |state| *state == Sleep::Entered)
                .unwrap(),
        );
    }
    result
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_latency(_: *mut rsf_xessgvk, frame: u32, marker: u32) -> i32 {
    assert!(frame == 42 && marker <= 6);
    let result = latency_result();
    if result == FIXTURE_OK {
        unsafe { f!(xell).marker_calls += 1 };
    }
    result
}
#[unsafe(no_mangle)]
extern "C" fn rsf_xessgvk_latency_status(
    _: *mut rsf_xessgvk,
    status: &mut rsf_fgvk_latency_status,
) -> i32 {
    *status = unsafe { f!(xell) };
    FIXTURE_OK
}
