// SPDX-License-Identifier: MIT
//! The provider-dispatch contract of vulkan_fg.rs over synthetic providers, without GPU or SDK
//! calls. `fakes` stands in for the four provider modules under `cfg(test)`.

pub(super) use fakes as dlss;
pub(super) use fakes as ffx;
pub(super) use fakes as fsr4;
pub(super) use fakes as xess;

/// The per-scenario fake state.
pub(super) struct State {
    ffx_creates: u32,
    dlss_creates: u32,
    destroys: u32,
    enabled: u32,
    generated_count: u32,
    calls: u32,
    explicit_presents: u32,
    refuse_destroy: bool,
    refuse_create: bool,
    refuse_state: bool,
    missing_owner: bool,
    fail_after_create: bool,
    fail_drain: bool,
    borrowed_setup: *const rsf_fgvk_setup,
    presented_source: *const rsf_frame_record,
    fsr4_creates: u32,
    xess_creates: u32,
    fsr4_create_result: i32,
    fsr4_partial: bool,
    refuse_latency: bool,
    xell: rsf_fgvk_latency_status,
}

impl Default for State {
    fn default() -> Self {
        // Null pointers, zero counters and false flags.
        unsafe { std::mem::zeroed() }
    }
}

pub(super) mod fakes {
    use super::super::Handle;
    use super::State;
    use crate::sys::*;
    pub(crate) use crate::vulkan_fsr4_fg::Options;
    use crate::vulkan_presentation::{
        rsf_core_fgvk_presentation_shutdown_begin, rsf_core_fgvk_presentation_shutdown_end,
    };
    use rsf_game_sys::backend_result::{
        FEATURE_FAILED as FIXTURE_FEATURE_FAILED, INIT_FAILED as FIXTURE_INIT_FAILED,
        NOT_READY as FIXTURE_NOT_READY, NOT_SUPPORTED as FIXTURE_NOT_SUPPORTED, OK as FIXTURE_OK,
    };
    use rsf_game_sys::rsf_frame_record;
    use std::{
        cell::UnsafeCell,
        ffi::c_void,
        ptr,
        sync::{Condvar, Mutex},
        time::Duration,
    };

    struct Fixture(UnsafeCell<State>);
    // The scenario serializes provider calls. During the one blocked sleep it only
    // probes the runtime's borrow gates, which must refuse before calling the fake.
    unsafe impl Sync for Fixture {}
    static STATE: Fixture = Fixture(UnsafeCell::new(unsafe { std::mem::zeroed() }));

    pub(super) fn fixture_state() -> *mut State {
        STATE.0.get()
    }

    pub(super) fn initialize() {
        let state = State {
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
        ($prepare:ident, $hudless:ident, $submitted:ident, $present:ident, $source:ident, $bits:literal, $result:expr) => {
            pub(crate) unsafe fn $prepare(
                _: *mut c_void,
                _: *mut c_void,
                _: *const rsf_fgvk_frame,
            ) -> i32 {
                unsafe { f!(calls) |= $bits };
                FIXTURE_OK
            }
            pub(crate) unsafe fn $hudless(
                _: *mut c_void,
                _: *mut c_void,
                _: *const rsf_fgvk_frame,
            ) -> i32 {
                unsafe { f!(calls) |= $bits << 1 };
                FIXTURE_OK
            }
            pub(crate) unsafe fn $submitted(_: *mut c_void, _: *const *mut c_void, _: u32) {
                unsafe { f!(calls) |= $bits << 2 };
            }
            pub(crate) unsafe fn $present(_: *mut c_void, _: *const c_void) -> i32 {
                unsafe { f!(calls) |= $bits << 3 };
                $result
            }
            pub(crate) unsafe fn $source(
                owner: *mut c_void,
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
        rsf_ffxgvk_prepare,
        rsf_ffxgvk_hudless,
        rsf_ffxgvk_submitted,
        rsf_ffxgvk_present,
        rsf_ffxgvk_present_source,
        1,
        SUBOPTIMAL
    );
    frame_callbacks!(
        rsf_dlssgvk_prepare,
        rsf_dlssgvk_hudless,
        rsf_dlssgvk_submitted,
        rsf_dlssgvk_present,
        rsf_dlssgvk_present_source,
        16,
        OUT_OF_DATE
    );
    frame_callbacks!(
        rsf_xessgvk_prepare,
        rsf_xessgvk_hudless,
        rsf_xessgvk_submitted,
        rsf_xessgvk_present,
        rsf_xessgvk_present_source,
        256,
        SUBOPTIMAL
    );
    frame_callbacks!(
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

    pub(crate) unsafe fn rsf_ffxgvk_create(
        setup: *const rsf_fgvk_setup,
        out: *mut Handle,
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
    pub(crate) unsafe fn rsf_dlssgvk_create(
        setup: *const rsf_fgvk_setup,
        out: *mut Handle,
        chain: *mut u64,
    ) -> i32 {
        unsafe {
            f!(dlss_creates) += 1;
            f!(borrowed_setup) = setup;
            created(DLSS, out, chain);
        }
        FIXTURE_OK
    }
    pub(crate) unsafe fn rsf_fsr4gvk_create(
        setup: *const rsf_fgvk_setup,
        options: *const crate::vulkan_fsr4_fg::Options,
        out: *mut Handle,
        chain: *mut u64,
    ) -> i32 {
        let options = unsafe { &*options };
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
    pub(crate) unsafe fn rsf_xessgvk_create(
        setup: *const rsf_fgvk_setup,
        options: *const rsf_xessgvk_options,
        out: *mut Handle,
        chain: *mut u64,
    ) -> i32 {
        let options = unsafe { &*options };
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
        ($name:ident, $id:expr, $count:expr) => {
            pub(crate) unsafe fn $name(owner: *mut c_void, count: *mut u32, _: *mut u64) -> i32 {
                assert_eq!(owner as usize, $id);
                unsafe { count.write($count) };
                FIXTURE_OK
            }
        };
    }
    images!(rsf_ffxgvk_images, FFX, 3);
    images!(rsf_dlssgvk_images, DLSS, 4);
    images!(rsf_fsr4gvk_images, FSR4, 3);
    images!(rsf_xessgvk_images, XESS, 3);

    macro_rules! acquire {
        ($name:ident, $verify:expr, $index:expr, $result:expr) => {
            pub(crate) unsafe fn $name(
                _: *mut c_void,
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
    acquire!(rsf_ffxgvk_acquire, true, 2, 2);
    acquire!(rsf_dlssgvk_acquire, false, 1, SUBOPTIMAL);
    acquire!(rsf_fsr4gvk_acquire, true, 1, 2);
    acquire!(rsf_xessgvk_acquire, false, 2, 2);

    fn enabled(value: u32, count: u32) -> i32 {
        unsafe {
            f!(enabled) = value;
            f!(generated_count) = count;
        }
        FIXTURE_OK
    }
    pub(crate) unsafe fn rsf_ffxgvk_enable(_: Handle, value: u32) -> i32 {
        enabled(value, 1)
    }
    pub(crate) unsafe fn rsf_dlssgvk_enable(_: Handle, value: u32, count: u32) -> i32 {
        if count > 3 {
            FIXTURE_NOT_SUPPORTED
        } else {
            enabled(value, count)
        }
    }
    pub(crate) unsafe fn rsf_fsr4gvk_enable_options(_: Handle, value: u32, count: u32) -> i32 {
        assert_eq!(count, 1);
        enabled(value, count)
    }
    pub(crate) unsafe fn rsf_xessgvk_enable_options(_: Handle, value: u32, count: u32) -> i32 {
        if count > 5 {
            -77
        } else {
            enabled(value, count)
        }
    }
    pub(crate) unsafe fn rsf_fsr4gvk_max_generated(_: Handle) -> u32 {
        1
    }
    pub(crate) unsafe fn rsf_xessgvk_max_generated(_: Handle) -> u32 {
        5
    }
    pub(crate) unsafe fn rsf_dlssgvk_image_layout(_: Handle, layout: *mut u32) -> i32 {
        unsafe { layout.write(1_000_001_002) };
        FIXTURE_OK
    }

    macro_rules! status {
        ($name:ident, $prepared:expr, $error:expr) => {
            pub(crate) unsafe fn $name(_: *mut c_void, status: *mut rsf_fgvk_status) -> i32 {
                let status = unsafe { &mut *status };
                status.prepared = $prepared;
                if let Some(error) = $error {
                    status.result = error;
                }
                FIXTURE_OK
            }
        };
    }
    status!(rsf_ffxgvk_get_status, 30, None);
    status!(rsf_fsr4gvk_status, 40, Some(-77));
    status!(rsf_xessgvk_status, 50, Some(-77));

    pub(crate) unsafe fn rsf_dlssgvk_get_status(_: Handle, status: *mut rsf_dlssgvk_status) -> i32 {
        let status = unsafe { &mut *status };
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
    pub(crate) unsafe fn rsf_ffxgvk_drain(_: Handle) -> i32 {
        if unsafe { f!(fail_drain) } {
            let result = rsf_core_fgvk_presentation_shutdown_begin();
            if result == FIXTURE_OK {
                rsf_core_fgvk_presentation_shutdown_end(0);
            }
            assert_ne!(
                result, FIXTURE_OK,
                "latency SDK shutdown cannot overlap a provider drain"
            );
            return FIXTURE_FEATURE_FAILED;
        }
        refusal(FIXTURE_NOT_READY)
    }
    pub(crate) unsafe fn rsf_dlssgvk_drain(_: Handle) -> i32 {
        refusal(FIXTURE_NOT_READY)
    }
    pub(crate) unsafe fn rsf_ffxgvk_destroy(_: Handle) -> i32 {
        retire()
    }
    pub(crate) unsafe fn rsf_dlssgvk_destroy(_: Handle) -> i32 {
        retire()
    }
    pub(crate) unsafe fn rsf_fsr4gvk_destroy(owner: Handle) -> i32 {
        assert_eq!(owner as usize, FSR4);
        refusal(-77)
    }
    pub(crate) unsafe fn rsf_fsr4gvk_drain(_: Handle) -> i32 {
        refusal(-77)
    }
    pub(crate) unsafe fn rsf_xessgvk_destroy(_: Handle) -> i32 {
        refusal(-77)
    }
    pub(crate) unsafe fn rsf_xessgvk_drain(_: Handle) -> i32 {
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

    pub(crate) unsafe fn rsf_xessgvk_latency_options(_: Handle, mode: u32, interval: u32) -> i32 {
        assert_eq!(interval, 5000);
        let result = latency_result();
        if result == FIXTURE_OK {
            unsafe { f!(xell).mode = mode };
        }
        result
    }
    pub(crate) unsafe fn rsf_xessgvk_sleep(_: Handle, frame: u32) -> i32 {
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
    pub(crate) unsafe fn rsf_xessgvk_latency(_: Handle, frame: u32, marker: u32) -> i32 {
        assert!(frame == 42 && marker <= 6);
        let result = latency_result();
        if result == FIXTURE_OK {
            unsafe { f!(xell).marker_calls += 1 };
        }
        result
    }
    pub(crate) unsafe fn rsf_xessgvk_latency_status(
        _: Handle,
        status: *mut rsf_fgvk_latency_status,
    ) -> i32 {
        unsafe { *status = f!(xell) };
        FIXTURE_OK
    }
}

use self::fakes::{block_sleep, fixture_state, initialize, release_sleep, wait_sleep};
use super::*;
use crate::vulkan_presentation::{
    rsf_core_fgvk_presentation_attach, rsf_core_fgvk_presentation_borrow,
    rsf_core_fgvk_presentation_return, rsf_core_fgvk_presentation_unbind,
};
use rsf_game_sys::backend_result::{
    ABI_MISMATCH as FIXTURE_ABI_MISMATCH, FEATURE_FAILED as FIXTURE_FEATURE_FAILED,
    INIT_FAILED as FIXTURE_INIT_FAILED, INVALID_ARGUMENT as FIXTURE_INVALID_ARGUMENT,
    NOT_COMPILED as FIXTURE_NOT_COMPILED, NOT_READY as FIXTURE_NOT_READY,
    NOT_SUPPORTED as FIXTURE_NOT_SUPPORTED, OK as FIXTURE_OK,
};
use rsf_game_sys::{RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record};
use std::ptr;

struct Lease {
    owner: *mut rsf_fgvk,
    closing: bool,
}

impl Lease {
    fn borrow() -> Self {
        let mut closing = 0;
        let owner = unsafe { rsf_core_fgvk_presentation_borrow(&mut closing) };
        Self {
            owner,
            closing: closing != 0,
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.owner.is_null() {
            rsf_core_fgvk_presentation_return();
        }
    }
}

fn host_options(intel_runtime_directory: *const u16) -> rsf_fgvk_host_options {
    rsf_fgvk_host_options {
        struct_size: size_of::<rsf_fgvk_host_options>() as u32,
        abi_version: RSF_FGVK_HOST_ABI_VERSION,
        window: ptr::without_provenance_mut(17),
        timeline_enabled: 1,
        intel_runtime_directory,
    }
}

fn verify_sleep_borrow(owner: *mut rsf_fgvk) {
    // Release the fake SDK's blocked sleep even if a main-thread assertion fails.
    struct ReleaseSleep;
    impl Drop for ReleaseSleep {
        fn drop(&mut self) {
            release_sleep();
        }
    }

    block_sleep();
    let address = owner as usize;
    std::thread::scope(|scope| {
        let sleeping = scope.spawn(move || {
            let lease = Lease::borrow();
            assert_eq!(
                lease.owner as usize, address,
                "successful creation binds latency context"
            );
            assert_eq!(
                unsafe { rsf_fgvk_latency_sleep(lease.owner, 42) },
                FIXTURE_OK
            );
        });
        let release = ReleaseSleep;
        assert!(wait_sleep(), "provider sleep entered");
        assert_eq!(unsafe { rsf_fgvk_drain(owner) }, FIXTURE_NOT_READY);
        assert_eq!(unsafe { rsf_fgvk_destroy(owner) }, FIXTURE_NOT_READY);
        assert!(
            Lease::borrow().owner.is_null(),
            "closing provider refuses new CPU users"
        );
        drop(release);
        sleeping.join().unwrap();
    });
}

fn run(platform: &Platform, fsr4: bool, xess: bool) {
    // Access the fixture record only between provider calls.
    initialize();
    let fixture = fixture_state();
    macro_rules! f {
        ($field:ident) => {
            (*fixture).$field
        };
    }
    unsafe {
        let mut identity = rsf_frame_record {
            struct_size: std::mem::offset_of!(rsf_frame_record, phase) as u32,
            ..std::mem::zeroed()
        };
        identity.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        identity.frame_id = 42;
        identity.session_id = 7;
        let mut setup = rsf_fgvk_setup {
            struct_size: size_of::<rsf_fgvk_setup>() as u32,
            abi_version: RSF_FGVK_ABI_VERSION,
            ..Default::default()
        };
        setup.graphics.struct_size = size_of_val(&setup.graphics) as u32;
        setup.graphics.abi_version = RSF_SRVK_ABI_VERSION;
        setup.graphics.backend = 5; // SR selection must not choose the FG provider.
        // The legacy FFX count-one creation with no host record, against the current setup.
        macro_rules! create_ffx {
            ($owner:ident, $chain:ident) => {
                create(
                    platform,
                    &setup,
                    FFX,
                    1,
                    ptr::null(),
                    &mut $owner,
                    &mut $chain,
                )
            };
        }
        let mut owner = ptr::null_mut();
        let mut chain = 0;
        assert!(
            create(
                platform,
                ptr::null(),
                1,
                1,
                ptr::null(),
                &mut owner,
                &mut chain
            ) == FIXTURE_INVALID_ARGUMENT
                && owner.is_null()
                && chain == 0,
            "null setup clears outputs"
        );
        setup.struct_size -= 1;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_INVALID_ARGUMENT,
            "short setup refused"
        );
        setup.struct_size += 1;
        setup.abi_version += 1;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_ABI_MISMATCH,
            "setup ABI refused"
        );
        setup.abi_version -= 1;
        let mut invalid = setup;
        invalid.abi_version += 1;
        invalid.graphics = Default::default();
        assert!(
            create(
                platform,
                &invalid,
                FFX,
                1,
                ptr::null(),
                &mut owner,
                &mut chain
            ) == FIXTURE_ABI_MISMATCH,
            "outer ABI mismatch takes precedence over embedded size"
        );
        invalid = setup;
        invalid.graphics.struct_size -= 1;
        invalid.graphics.abi_version += 1;
        assert!(
            create(
                platform,
                &invalid,
                FFX,
                1,
                ptr::null(),
                &mut owner,
                &mut chain
            ) == FIXTURE_INVALID_ARGUMENT,
            "embedded size takes precedence over embedded ABI"
        );
        invalid = setup;
        invalid.graphics.abi_version += 1;
        assert!(
            create(
                platform,
                &invalid,
                FFX,
                1,
                ptr::null(),
                &mut owner,
                &mut chain
            ) == FIXTURE_ABI_MISMATCH,
            "embedded ABI refused"
        );
        assert!(
            create(platform, &setup, 2, 1, ptr::null(), &mut owner, &mut chain)
                == FIXTURE_NOT_SUPPORTED
                && f!(ffx_creates) == 0
                && f!(dlss_creates) == 0,
            "unsupported FG provider never allocates a concrete owner"
        );
        assert!(
            create(platform, &setup, 3, 2, ptr::null(), &mut owner, &mut chain)
                == FIXTURE_NOT_SUPPORTED
                && f!(ffx_creates) == 0,
            "FFX unsupported MFG request never clamps or allocates"
        );
        assert!(
            create_ffx!(owner, chain) == FIXTURE_OK
                && !owner.is_null()
                && chain == 33
                && f!(borrowed_setup) == ptr::from_ref(&setup)
                && f!(enabled) == 0,
            "legacy create preserves FFX/count1 and borrowed setup"
        );
        let mut value = 0;
        assert!(
            rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK && value == 3,
            "legacy FG independent of XeSS SR selection"
        );
        assert!(
            rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK && value == 5,
            "FFX image layout"
        );
        assert!(
            rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK && value == 1,
            "FFX count limit"
        );
        let mut second = ptr::null_mut();
        let mut second_chain = 0;
        assert!(
            create(
                platform,
                &setup,
                1,
                3,
                ptr::null(),
                &mut second,
                &mut second_chain
            ) == FIXTURE_NOT_READY
                && second.is_null()
                && second_chain == 0
                && f!(dlss_creates) == 0,
            "single admission spans both providers"
        );
        assert!(
            rsf_fgvk_enable_options(owner, 1, 2) == FIXTURE_NOT_SUPPORTED && f!(enabled) == 0,
            "invalid FFX options preserve request"
        );
        assert!(
            rsf_fgvk_enable(owner, 1) == FIXTURE_OK && f!(enabled) != 0 && f!(generated_count) == 1,
            "legacy enable preserves count1"
        );
        let command = ptr::without_provenance_mut(1);
        let frame = rsf_fgvk_frame::default();
        let info = ptr::from_ref(&frame).cast();
        assert!(
            rsf_fgvk_prepare(owner, command, &frame) == FIXTURE_OK
                && rsf_fgvk_hudless(owner, command, &frame) == FIXTURE_OK,
            "FFX recording routed"
        );
        rsf_fgvk_submitted(owner, &command, 1);
        assert!(
            rsf_fgvk_present(owner, info) == 1000001003 && f!(calls) == 15,
            "FFX submitted/Present and result routed"
        );
        assert!(
            rsf_fgvk_present_source(owner, info, &identity) == 1000001003
                && f!(presented_source) == ptr::from_ref(&identity),
            "FFX explicit identity prefix and WSI result forwarded"
        );
        let mut malformed = identity;
        malformed.struct_size -= 1;
        let before_invalid = f!(explicit_presents);
        assert!(
            rsf_fgvk_present_source(owner, info, &malformed) == -3
                && f!(explicit_presents) == before_invalid,
            "short source prefix never reaches WSI provider"
        );
        malformed = identity;
        malformed.abi_version += 1;
        assert!(
            rsf_fgvk_present_source(owner, info, &malformed) == -3
                && f!(explicit_presents) == before_invalid,
            "source ABI mismatch never reaches WSI provider"
        );
        assert!(
            rsf_fgvk_present_source(owner, info, ptr::null()) == 1000001003
                && f!(presented_source).is_null(),
            "unassociated output is passed explicitly, not replaced by a cached source"
        );
        assert!(
            rsf_fgvk_acquire(owner, 123, 4, 5, &mut value) == 2 && value == 2,
            "positive Vulkan timeout preserved"
        );
        let mut status = rsf_fgvk_status {
            struct_size: size_of::<rsf_fgvk_status>() as u32,
            abi_version: RSF_FGVK_ABI_VERSION,
            ..Default::default()
        };
        assert!(
            rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK && status.prepared == 30,
            "FFX status routed"
        );
        f!(refuse_destroy) = true;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_NOT_READY
                && rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK
                && value == 3,
            "failed destroy retains concrete owner"
        );
        assert!(
            create(
                platform,
                &setup,
                1,
                3,
                ptr::null(),
                &mut second,
                &mut second_chain
            ) == FIXTURE_NOT_READY
                && f!(dlss_creates) == 0,
            "failed destroy retains global admission"
        );
        f!(refuse_destroy) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "FFX retirement retry succeeds"
        );
        assert!(
            create(platform, &setup, 1, 3, ptr::null(), &mut owner, &mut chain) == FIXTURE_OK
                && chain == 11
                && f!(generated_count) == 3,
            "DLSS creation forwards MFG count3"
        );
        assert!(
            rsf_fgvk_enable(owner, 1) == FIXTURE_OK && f!(enabled) != 0 && f!(generated_count) == 3,
            "legacy enable retains selected DLSS MFG count"
        );
        assert!(
            rsf_fgvk_enable_options(owner, 0, 4) == FIXTURE_NOT_SUPPORTED
                && f!(enabled) != 0
                && f!(generated_count) == 3,
            "unsupported DLSS options preserve both count and enabled state"
        );
        assert!(
            rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK && value == 1000001002,
            "DLSS proxy image layout forwarded"
        );
        assert!(
            rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK && value == 3,
            "DLSS real reported limit forwarded"
        );
        status.prepared = 0;
        assert!(
            rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK
                && status.prepared == 10
                && status.abi_version == 1,
            "DLSS nested ABI1 status copied"
        );
        f!(refuse_state) = true;
        value = 77;
        assert!(
            rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_NOT_READY && value == 77,
            "failed capability query preserves output"
        );
        f!(refuse_state) = false;
        f!(calls) = 0;
        let _ = rsf_fgvk_prepare(owner, command, &frame);
        let _ = rsf_fgvk_hudless(owner, command, &frame);
        rsf_fgvk_submitted(owner, &command, 1);
        assert!(
            rsf_fgvk_present(owner, info) == -1000001004 && f!(calls) == 240,
            "DLSS recording/submission/WSI error forwarded"
        );
        assert!(
            rsf_fgvk_present_source(owner, info, &identity) == -1000001004
                && f!(presented_source) == ptr::from_ref(&identity),
            "DLSS explicit identity preserves original WSI failure"
        );
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "DLSS retirement succeeds"
        );
        f!(refuse_destroy) = true;
        owner = ptr::null_mut();
        assert!(
            create(platform, &setup, 1, 4, ptr::null(), &mut owner, &mut chain)
                == FIXTURE_NOT_READY
                && !owner.is_null()
                && chain == 11,
            "failed unsupported-count cleanup returns retained owner for retry"
        );
        assert!(
            create_ffx!(second, second_chain) == FIXTURE_NOT_READY && second.is_null(),
            "partial failed creation retains global admission"
        );
        f!(calls) = 0;
        assert!(
            rsf_core_fgvk_presentation_attach(owner) == FIXTURE_NOT_READY,
            "partial owner refuses latency attachment"
        );
        assert!(
            rsf_fgvk_prepare(owner, command, &frame) == FIXTURE_NOT_READY
                && rsf_fgvk_enable(owner, 1) == FIXTURE_NOT_READY
                && rsf_fgvk_present(owner, info) == -3
                && f!(calls) == 0,
            "partial owner refuses new provider work"
        );
        f!(refuse_destroy) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "partial create cleanup retry succeeds"
        );
        f!(refuse_create) = true;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_INIT_FAILED && owner.is_null() && chain == 0,
            "concrete create error releases clean generic admission"
        );
        f!(refuse_create) = false;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_OK && rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "creation resumes after clean factory failure"
        );
        owner = ptr::null_mut();
        assert!(
            create(platform, &setup, 4, 3, ptr::null(), &mut owner, &mut chain)
                == FIXTURE_NOT_SUPPORTED
                && owner.is_null()
                && chain == 0,
            "FSR4 rejects unsupported MFG before concrete creation or fallback"
        );
        if fsr4 {
            assert!(
                create(platform, &setup, 4, 1, ptr::null(), &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT
                    && f!(fsr4_creates) == 0,
                "FSR4 needs explicit host contract"
            );
            let mut fsr4_host = host_options(ptr::null());
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT
                    && f!(fsr4_creates) == 0,
                "FSR4 requires FidelityFX runtime directory rather than Intel host path"
            );
            setup.graphics.runtime_directory_utf8 = c"FidelityFX".as_ptr();
            fsr4_host.struct_size -= 1;
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT,
                "short FSR4 host refused"
            );
            fsr4_host.struct_size += 1;
            fsr4_host.abi_version += 1;
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_ABI_MISMATCH,
                "FSR4 host ABI refused"
            );
            fsr4_host.abi_version -= 1;
            fsr4_host.timeline_enabled = 0;
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT,
                "FSR4 external timeline must be enabled"
            );
            fsr4_host.timeline_enabled = 1;
            let before_fsr4_ffx = f!(ffx_creates);
            f!(fsr4_create_result) = RSF_FSR4FG_NOT_SUPPORTED;
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_NOT_SUPPORTED
                    && owner.is_null()
                    && chain == 0
                    && f!(ffx_creates) == before_fsr4_ffx,
                "missing exact FSR4 provider never falls back to FFX3"
            );
            f!(fsr4_create_result) = 0;
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain) == FIXTURE_OK
                    && chain == 44
                    && f!(borrowed_setup) == ptr::from_ref(&setup)
                    && f!(enabled) == 0
                    && f!(generated_count) == 1,
                "FSR4 create is SR-independent and starts Off"
            );
            assert!(
                rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK
                    && value == 4
                    && rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK
                    && value == 5
                    && rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK
                    && value == 1,
                "FSR4 owner, shader-read layout and count-one limit"
            );
            assert!(
                rsf_fgvk_enable(owner, 1) == FIXTURE_OK
                    && f!(enabled) != 0
                    && f!(generated_count) == 1,
                "FSR4 enable activates count one"
            );
            // Separate asserts: each call changes what the fixture reads back.
            assert!(
                rsf_fgvk_enable_options(owner, 0, 2) == FIXTURE_NOT_SUPPORTED && f!(enabled) != 0,
                "FSR4 invalid options retain prior active count-one state"
            );
            f!(calls) = 0;
            let _ = rsf_fgvk_prepare(owner, command, &frame);
            let _ = rsf_fgvk_hudless(owner, command, &frame);
            rsf_fgvk_submitted(owner, &command, 1);
            assert!(
                rsf_fgvk_present(owner, info) == 1000001003
                    && f!(calls) == 61440
                    && rsf_fgvk_acquire(owner, 123, 4, 5, &mut value) == 2
                    && value == 1
                    && rsf_fgvk_images(owner, &mut value, ptr::null_mut()) == 0
                    && value == 3,
                "FSR4 recording and all WSI routes preserve results"
            );
            assert!(
                rsf_fgvk_present_source(owner, info, &identity) == 1000001003
                    && f!(presented_source) == ptr::from_ref(&identity),
                "FSR4 explicit identity forwarded"
            );
            assert!(
                rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK
                    && status.prepared == 40
                    && status.result == -77,
                "FSR4 status preserves private SDK result"
            );
            f!(refuse_destroy) = true;
            assert!(
                rsf_fgvk_drain(owner) == FIXTURE_FEATURE_FAILED
                    && rsf_fgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                    && create_ffx!(second, second_chain) == FIXTURE_NOT_READY,
                "FSR4 failed retirement preserves owner and process admission"
            );
            f!(refuse_destroy) = false;
            assert!(
                rsf_fgvk_destroy(owner) == FIXTURE_OK,
                "FSR4 retirement retry succeeds"
            );
            f!(fsr4_create_result) = -77;
            f!(fsr4_partial) = true;
            f!(refuse_destroy) = true;
            assert!(
                create(platform, &setup, 4, 1, &fsr4_host, &mut owner, &mut chain)
                    == FIXTURE_FEATURE_FAILED
                    && !owner.is_null()
                    && chain == 44,
                "FSR4 failed partial cleanup returns retained owner and chain"
            );
            f!(refuse_destroy) = false;
            assert!(
                rsf_fgvk_destroy(owner) == FIXTURE_OK,
                "FSR4 partial owner retires on retry"
            );
            f!(fsr4_partial) = false;
            f!(fsr4_create_result) = 0;
        } else {
            assert!(
                create(platform, &setup, 4, 1, ptr::null(), &mut owner, &mut chain)
                    == FIXTURE_NOT_COMPILED
                    && owner.is_null()
                    && chain == 0,
                "missing FSR4 build is explicit without provider fallback"
            );
        }
        if xess {
            assert!(
                create(platform, &setup, 5, 3, ptr::null(), &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT
                    && f!(xess_creates) == 0,
                "legacy creation cannot select XeSS without its host contract"
            );
            let intel = [b'D' as u16, b':' as u16, 0];
            let mut host = host_options(intel.as_ptr());
            host.struct_size -= 1;
            assert!(
                create(platform, &setup, 5, 3, &host, &mut owner, &mut chain)
                    == FIXTURE_INVALID_ARGUMENT,
                "short host contract refused"
            );
            host.struct_size += 1;
            host.abi_version += 1;
            assert!(
                create(platform, &setup, 5, 3, &host, &mut owner, &mut chain)
                    == FIXTURE_ABI_MISMATCH,
                "host ABI mismatch refused"
            );
            host.abi_version -= 1;
            setup.graphics.backend = 1;
            assert!(
                create(platform, &setup, 5, 3, &host, &mut owner, &mut chain) == FIXTURE_OK
                    && chain == 55
                    && f!(generated_count) == 3,
                "explicit XeSS FG remains independent of DLSS SR and preserves requested count"
            );
            assert!(
                rsf_fgvk_backend(owner, &mut value) == FIXTURE_OK && value == 5,
                "XeSS owner dispatched"
            );
            assert!(
                rsf_fgvk_image_layout(owner, &mut value) == FIXTURE_OK && value == 5,
                "XeSS layout dispatched"
            );
            assert!(
                rsf_fgvk_max_generated(owner, &mut value) == FIXTURE_OK && value == 5,
                "XeSS native count dispatched"
            );
            assert!(
                create_ffx!(second, second_chain) == FIXTURE_NOT_READY,
                "XeSS shares process owner admission"
            );
            assert!(
                rsf_fgvk_enable(owner, 1) == FIXTURE_OK
                    && f!(generated_count) == 3
                    && f!(enabled) != 0,
                "legacy XeSS enable preserves MFG count"
            );
            assert!(
                rsf_fgvk_enable_options(owner, 0, 6) == FIXTURE_FEATURE_FAILED
                    && f!(enabled) != 0
                    && f!(generated_count) == 3,
                "XeSS unsupported count is never clamped"
            );
            f!(calls) = 0;
            let _ = rsf_fgvk_prepare(owner, command, &frame);
            let _ = rsf_fgvk_hudless(owner, command, &frame);
            rsf_fgvk_submitted(owner, &command, 1);
            assert!(
                rsf_fgvk_present(owner, info) == 1000001003
                    && f!(calls) == 3840
                    && rsf_fgvk_acquire(owner, 0, 0, 0, &mut value) == 2
                    && value == 2,
                "XeSS WSI results and submission routes preserved"
            );
            assert!(
                rsf_fgvk_present_source(owner, info, &identity) == 1000001003
                    && f!(presented_source) == ptr::from_ref(&identity),
                "XeSS explicit identity forwarded"
            );
            assert!(
                rsf_fgvk_get_status(owner, &mut status) == FIXTURE_OK
                    && status.prepared == 50
                    && status.result == -77,
                "native XeSS errors remain in bounded status"
            );
            assert!(
                rsf_fgvk_latency_options(owner, 2, 5000) == FIXTURE_NOT_SUPPORTED
                    && f!(xell).mode == 1,
                "XeLL Boost refused without state mutation"
            );
            assert!(
                rsf_fgvk_latency_options(owner, 0, 5000) == FIXTURE_OK && f!(xell).mode == 0,
                "XeLL Off options forwarded"
            );
            assert!(
                rsf_fgvk_latency_sleep(owner, 0) == FIXTURE_INVALID_ARGUMENT
                    && rsf_fgvk_latency_sleep(owner, u64::from(u32::MAX) + 1)
                        == FIXTURE_INVALID_ARGUMENT
                    && rsf_fgvk_latency_marker(owner, 42, 7) == FIXTURE_INVALID_ARGUMENT,
                "XeLL domain rejects invalid or truncated identities"
            );
            assert!(
                rsf_fgvk_latency_sleep(owner, 42) == FIXTURE_OK
                    && rsf_fgvk_latency_marker(owner, 42, 6) == FIXTURE_OK,
                "XeLL sleep and supported marker reach private owner"
            );
            f!(refuse_latency) = true;
            assert!(
                rsf_fgvk_latency_sleep(owner, 42) == FIXTURE_FEATURE_FAILED,
                "XeLL native sleep failure preserved as public refusal"
            );
            let mut latency = rsf_fgvk_latency_status {
                struct_size: size_of::<rsf_fgvk_latency_status>() as u32,
                abi_version: RSF_FGVK_LATENCY_ABI_VERSION,
                ..Default::default()
            };
            assert!(
                rsf_fgvk_latency_get_status(owner, &mut latency) == FIXTURE_OK
                    && latency.sleep_calls == 1
                    && latency.marker_calls == 1
                    && latency.result == -77
                    && latency.mode == 0,
                "XeLL counts cover accepted calls and retain exact native error"
            );
            f!(refuse_latency) = false;
            verify_sleep_borrow(owner);
            f!(refuse_destroy) = true;
            assert!(
                rsf_fgvk_drain(owner) == FIXTURE_FEATURE_FAILED
                    && rsf_fgvk_destroy(owner) == FIXTURE_FEATURE_FAILED
                    && create_ffx!(second, second_chain) == FIXTURE_NOT_READY,
                "XeSS failed retirement preserves process owner"
            );
            f!(refuse_destroy) = false;
            assert!(
                rsf_fgvk_destroy(owner) == FIXTURE_OK,
                "XeSS owner retry retires safely"
            );
        } else {
            assert!(
                create(platform, &setup, 5, 3, ptr::null(), &mut owner, &mut chain)
                    == FIXTURE_NOT_COMPILED
                    && owner.is_null()
                    && chain == 0,
                "XeSS missing SDK is explicit and does not allocate"
            );
        }
        let other_context = ptr::without_provenance_mut(99);
        assert!(
            rsf_core_fgvk_presentation_attach(other_context) == FIXTURE_OK,
            "independent existing latency binding retained"
        );
        owner = ptr::null_mut();
        chain = 0;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_NOT_READY && owner.is_null() && chain == 0,
            "attachment collision rolls provider creation back"
        );
        f!(refuse_destroy) = true;
        assert!(
            create_ffx!(owner, chain) != FIXTURE_OK && !owner.is_null(),
            "failed rollback retains partial provider owner"
        );
        f!(refuse_destroy) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "partial unattached owner cleanup retry"
        );
        {
            let retained = Lease::borrow();
            assert!(
                retained.owner == other_context,
                "partial-owner cleanup cannot release another active binding"
            );
        }
        assert!(
            rsf_core_fgvk_presentation_unbind() == FIXTURE_OK,
            "independent binding released by its owner"
        );
        f!(missing_owner) = true;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_INIT_FAILED && owner.is_null() && chain == 0,
            "success without a provider is refused and does not publish the chain"
        );
        f!(missing_owner) = false;
        f!(fail_after_create) = true;
        let before_partial = f!(destroys);
        assert!(
            create_ffx!(owner, chain) == FIXTURE_INIT_FAILED
                && owner.is_null()
                && chain == 0
                && f!(destroys) == before_partial + 1,
            "failed create retires the partial provider before freeing the claim"
        );
        f!(fail_after_create) = false;
        assert!(
            create_ffx!(owner, chain) == FIXTURE_OK,
            "claim is reusable after early failures"
        );
        f!(fail_drain) = true;
        assert!(
            rsf_fgvk_drain(owner) == FIXTURE_FEATURE_FAILED,
            "drain failure is forwarded"
        );
        {
            let lease = Lease::borrow();
            assert!(
                lease.owner == owner && !lease.closing,
                "failed drain reopens CPU borrows"
            );
        }
        f!(fail_drain) = false;
        assert!(
            rsf_fgvk_destroy(owner) == FIXTURE_OK,
            "provider retires after a failed drain"
        );
        assert!(
            rsf_fgvk_destroy(ptr::null_mut()) == FIXTURE_OK
                && rsf_fgvk_drain(ptr::null_mut()) == FIXTURE_OK,
            "null retirement remains idempotent"
        );
    }
}

/// The three CMake provider variants: no optional provider, FSR4 and XeSS. They share the
/// process-wide admission, so they run in sequence.
#[test]
fn dispatch_admission_routing_and_retirement() {
    static BASE: Platform = Platform {
        dlss: Some(&DLSS_PROVIDER),
        ffx: Some(&FFX_PROVIDER),
        fsr4: None,
        xess: None,
    };
    static WITH_FSR4: Platform = Platform {
        fsr4: Some(&FSR4_PROVIDER),
        ..BASE
    };
    static WITH_XESS: Platform = Platform {
        xess: Some(&XESS_PROVIDER),
        ..BASE
    };
    run(&BASE, false, false);
    run(&WITH_FSR4, true, false);
    run(&WITH_XESS, false, true);
}
