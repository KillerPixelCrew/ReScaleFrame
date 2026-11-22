// SPDX-License-Identifier: MIT
//! Admission and lifetime of the 13 CPU producer hooks for Skyrim SE 1.7.104.0.
//! Native detours keep the engine ABI, caller addresses and original calls on their own frames.

use crate::cpu_policy::bad_screen_query;
use crate::cpu_sources::rsf_skyrim_cpu_clear_proof;
use crate::hook_set::HookSet;
use crate::platform::{self, Platform};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use winapi::um::profileapi::QueryPerformanceFrequency;
use winapi::um::winnt::LARGE_INTEGER;

const NAMED_CALLBACK_SLOT: u32 = 0x20b5130;
const NAMED_POLL: u32 = 0x6e6030;

// Indexed by the native detour table in cpu_events.cpp.
const HOOKS: [(u32, [u8; 19]); 13] = [
    (
        0x658870,
        [
            0x48, 0x8b, 0xc4, 0x55, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81,
            0xec, 0x30, 0x01, 0x00, 0x00,
        ],
    ), // tick
    (
        0xcf95f0,
        [
            0x48, 0x89, 0x5c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec,
            0x30, 0x0f, 0x29, 0x74, 0x24,
        ],
    ), // poll
    (
        0x6565b0,
        [
            0x40, 0x53, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xec,
            0x60, 0x01, 0x00, 0x00, 0x48,
        ],
    ), // render
    (
        0xcfa650,
        [
            0x48, 0x8b, 0xc4, 0x48, 0x89, 0x50, 0x10, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55,
            0x41, 0x56, 0x41, 0x57, 0x48,
        ],
    ), // dispatch
    (
        0x656e60,
        [
            0x48, 0x8b, 0xc4, 0x89, 0x50, 0x10, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41,
            0x56, 0x41, 0x57, 0x48, 0x8d,
        ],
    ), // world
    (
        0x7ad900,
        [
            0x48, 0x8b, 0xc4, 0x56, 0x57, 0x41, 0x54, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xec,
            0x80, 0x00, 0x00, 0x00, 0x48,
        ],
    ), // controls
    (
        0x8fd590,
        [
            0x40, 0x57, 0x48, 0x83, 0xec, 0x70, 0x48, 0xc7, 0x44, 0x24, 0x20, 0xfe, 0xff, 0xff,
            0xff, 0x48, 0x89, 0x9c, 0x24,
        ],
    ), // camera_update
    (
        0x60eaf0,
        [
            0x48, 0x83, 0xec, 0x28, 0x48, 0x83, 0x3d, 0x94, 0x1f, 0xc1, 0x02, 0x00, 0x74, 0x69,
            0x48, 0x89, 0x5c, 0x24, 0x30,
        ],
    ), // jobs_submit
    (
        0x60eb80,
        [
            0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0x1d, 0x03, 0x1f, 0xc1, 0x02, 0x48,
            0x85, 0xdb, 0x74, 0x26, 0x83,
        ],
    ), // jobs_join
    (
        NAMED_POLL,
        [
            0x48, 0x8b, 0x0d, 0xf9, 0xa4, 0xb4, 0x02, 0xe9, 0xb4, 0x3d, 0xf7, 0xff, 0xcc, 0xcc,
            0xcc, 0xcc, 0xcc, 0xcc, 0xcc,
        ],
    ), // named_poll
    (
        0x776290,
        [
            0x48, 0x8b, 0xc4, 0x41, 0x56, 0x48, 0x81, 0xec, 0xe0, 0x00, 0x00, 0x00, 0x48, 0xc7,
            0x44, 0x24, 0x30, 0xfe, 0xff,
        ],
    ), // movement
    (
        0x745200,
        [
            0x48, 0x8b, 0xc4, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57,
            0x48, 0x8d, 0xa8, 0x98, 0xfe,
        ],
    ), // player_update
    (
        0x558e70,
        [
            0x40, 0x57, 0x48, 0x83, 0xec, 0x30, 0x48, 0xc7, 0x44, 0x24, 0x20, 0xfe, 0xff, 0xff,
            0xff, 0x48, 0x89, 0x5c, 0x24,
        ],
    ), // player_camera_update
];

const _: () = assert!(HOOKS.len() == crate::cpu_call_abi::RSF_SKYRIM_CPU_HOOK_COUNT as usize);

const INDICES: std::ops::Range<usize> = 0..HOOKS.len();

// C++ owns the native calls and unwind receipts. Rust binds their typed addresses to the
// checked hook order and publishes the original trampoline before enabling a detour.
unsafe extern "C" {
    fn rsf_skyrim_cpu_tick(main: *mut c_void) -> i16;
    fn rsf_skyrim_cpu_poll(manager: *mut c_void, timestep: f32) -> usize;
    fn rsf_skyrim_cpu_render(main: *mut c_void) -> usize;
    fn rsf_skyrim_cpu_dispatch(manager: *mut c_void, list: *mut c_void) -> i32;
    fn rsf_skyrim_cpu_world(main: *mut c_void, output: u32, flags: u8) -> usize;
    fn rsf_skyrim_cpu_controls(owner: *mut c_void, list: *mut c_void, source: *mut c_void)
    -> usize;
    fn rsf_skyrim_cpu_camera_update(state: *mut c_void) -> usize;
    fn rsf_skyrim_cpu_jobs_submit() -> u32;
    fn rsf_skyrim_cpu_jobs_join() -> u32;
    fn rsf_skyrim_cpu_named_poll(data: *mut c_void) -> usize;
    fn rsf_skyrim_cpu_movement(
        actor: *mut c_void,
        timestep: f32,
        argument: *mut c_void,
        output: *mut f32,
    );
    fn rsf_skyrim_cpu_player_update(actor: *mut c_void, timestep: f32) -> usize;
    fn rsf_skyrim_cpu_player_camera_update(camera: *mut c_void);
}

const DETOURS: [*const c_void; HOOKS.len()] = [
    rsf_skyrim_cpu_tick as *const c_void,
    rsf_skyrim_cpu_poll as *const c_void,
    rsf_skyrim_cpu_render as *const c_void,
    rsf_skyrim_cpu_dispatch as *const c_void,
    rsf_skyrim_cpu_world as *const c_void,
    rsf_skyrim_cpu_controls as *const c_void,
    rsf_skyrim_cpu_camera_update as *const c_void,
    rsf_skyrim_cpu_jobs_submit as *const c_void,
    rsf_skyrim_cpu_jobs_join as *const c_void,
    rsf_skyrim_cpu_named_poll as *const c_void,
    rsf_skyrim_cpu_movement as *const c_void,
    rsf_skyrim_cpu_player_update as *const c_void,
    rsf_skyrim_cpu_player_camera_update as *const c_void,
];

static ORIGINALS: [AtomicPtr<c_void>; HOOKS.len()] =
    [const { AtomicPtr::new(ptr::null_mut()) }; HOOKS.len()];

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_original(index: u32) -> *mut c_void {
    ORIGINALS
        .get(index as usize)
        .map_or(ptr::null_mut(), |slot| slot.load(Ordering::Acquire))
}

fn qpc_frequency() -> u64 {
    let mut value: LARGE_INTEGER = unsafe { std::mem::zeroed() };
    if unsafe { QueryPerformanceFrequency(&mut value) } == 0 {
        return 0;
    }
    u64::try_from(unsafe { *value.QuadPart() }).unwrap_or(0)
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Services {
    pub(crate) host: rsf_game_host_services,
    pub(crate) frequency: u64,
}

// The SDK table borrows process services used by native CPU producer threads. Detours
// copy it under the lock within their entry lease. Start may copy it to log a refusal.
unsafe impl Send for Services {}

#[repr(C)]
pub(crate) struct StartRefusal {
    pub(crate) stage: *const c_char,
    pub(crate) rva: u32,
    pub(crate) status: i32,
}

const _: () = {
    assert!(size_of::<Services>() == size_of::<rsf_game_host_services>() + 8);
    assert!(std::mem::offset_of!(Services, frequency) == size_of::<rsf_game_host_services>());
    assert!(size_of::<StartRefusal>() == 16);
};

struct Control {
    hooks: HookSet<{ HOOKS.len() }>,
    base: usize,
    pinned: bool,
    frequency: u64,
}

static CONTROL: Mutex<Control> = Mutex::new(Control {
    hooks: HookSet::new(),
    base: 0,
    pinned: false,
    frequency: 0,
});
// Separate short lock: enabling or disabling hooks can suspend native threads, and calls
// into the engine or host may nest other CPU detours. They never hold this lock.
static SERVICES: Mutex<Option<Services>> = Mutex::new(None);
// Close admission and count detours in the same atomic order. A late entry after
// disable's drain check cannot observe the old active session.
const ACTIVE: u32 = 1 << 31;
const ENTERED: u32 = ACTIVE - 1;
static PRODUCERS: AtomicU32 = AtomicU32::new(0);

fn target(base: usize, index: usize) -> usize {
    base + HOOKS[index].0 as usize
}

fn admission(platform: &Platform, base: usize) -> Result<(), StartRefusal> {
    let named_slot = base.checked_add(NAMED_CALLBACK_SLOT as usize);
    let named_poll = base.checked_add(NAMED_POLL as usize);
    if base == 0
        || named_poll.is_none()
        || named_slot.and_then(|address| platform.read_usize(address)) != named_poll
    {
        return Err(StartRefusal::new(c"named callback", NAMED_CALLBACK_SLOT, 0));
    }
    let screen_rva = bad_screen_query(platform, base);
    if screen_rva != 0 {
        return Err(StartRefusal::new(c"screen query bytes", screen_rva, 0));
    }
    for (rva, bytes) in &HOOKS {
        // Every hook RVA is below the checked named slot.
        if !platform.matches(base + *rva as usize, bytes) {
            return Err(StartRefusal::new(c"hook bytes", *rva, 0));
        }
    }
    Ok(())
}

impl StartRefusal {
    fn new(stage: &'static CStr, rva: u32, status: i32) -> Self {
        Self {
            stage: stage.as_ptr(),
            rva,
            status,
        }
    }
}

impl Control {
    fn remove_unpinned(&mut self) {
        let base = self.base;
        self.hooks.remove(INDICES, |index| {
            let removed = unsafe {
                rsf_windows::minhook::remove_idempotent(target(base, index) as *mut c_void)
            };
            if removed {
                ORIGINALS[index].store(ptr::null_mut(), Ordering::Release);
            }
            removed
        });
    }

    fn prepare(&mut self, platform: &Platform) -> bool {
        if active() || rsf_skyrim_cpu_rust_busy() || !self.hooks.is_disabled() {
            return false;
        }
        let base = platform.main_module();
        if admission(platform, base).is_err() {
            return false;
        }

        if !self.pinned {
            self.remove_unpinned();
            if !self.hooks.is_empty() {
                return false;
            }
            self.base = base;
            if !self.hooks.create(|index| {
                let Some(original) = platform.hook_create(target(base, index), DETOURS[index])
                else {
                    return false;
                };
                ORIGINALS[index].store(original, Ordering::Release);
                true
            }) {
                self.remove_unpinned();
                return false;
            }
        } else if self.base != base || !self.hooks.is_complete() {
            return false;
        }

        let frequency = qpc_frequency();
        if frequency == 0 || (!self.pinned && platform.pin_module(DETOURS[0] as usize) == 0) {
            if !self.pinned {
                self.remove_unpinned();
            }
            return false;
        }
        self.pinned = true;
        self.frequency = frequency;
        *SERVICES.lock().unwrap_or_else(PoisonError::into_inner) = None;
        true
    }

    fn start(&mut self, platform: &Platform) -> Result<(), StartRefusal> {
        if active() || rsf_skyrim_cpu_rust_busy() {
            return Err(StartRefusal::new(c"active or entered", 0, 0));
        }
        // Disabled targets are the only ones whose live prologues can be checked.
        if let Some(index) = self.hooks.first_unavailable() {
            return Err(StartRefusal::new(c"hook state", HOOKS[index].0, 0));
        }
        let base = platform.main_module();
        if !self.pinned
            || base != self.base
            || SERVICES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_none()
        {
            return Err(StartRefusal::new(c"hook state", 0, 0));
        }
        admission(platform, base)?;

        rsf_skyrim_cpu_clear_proof(true);
        if let Err((index, status)) = self.hooks.enable(|index| unsafe {
            rsf_windows::minhook::enable_status(target(base, index) as *mut c_void)
        }) {
            // Keep preparation and any hook whose disable refused so stop can retry.
            self.disable_hooks();
            return Err(StartRefusal::new(c"hook enable", HOOKS[index].0, status));
        }
        PRODUCERS.fetch_or(ACTIVE, Ordering::Release);
        Ok(())
    }

    fn disable_hooks(&mut self) -> bool {
        let base = self.base;
        self.hooks.disable(INDICES, |index| unsafe {
            rsf_windows::minhook::disable(target(base, index) as *mut c_void)
        })
    }

    fn disable(&mut self) -> bool {
        rsf_skyrim_cpu_rust_quiesce();
        self.disable_hooks() && !rsf_skyrim_cpu_rust_busy()
    }
}

pub(crate) extern "C" fn prepare() -> bool {
    let Some(platform) = platform::get() else {
        return false;
    };
    CONTROL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .prepare(platform)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_cpu_rust_bind(host: &rsf_game_host_services) -> bool {
    let control = CONTROL.lock().unwrap_or_else(PoisonError::into_inner);
    if control.frequency == 0 || !control.hooks.is_disabled() || rsf_skyrim_cpu_rust_busy() {
        return false;
    }
    *SERVICES.lock().unwrap_or_else(PoisonError::into_inner) = Some(Services {
        host: *host,
        frequency: control.frequency,
    });
    true
}

pub(crate) fn start() -> Result<(), StartRefusal> {
    let platform = platform::get().ok_or_else(|| StartRefusal::new(c"hook state", 0, 0))?;
    CONTROL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .start(platform)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_cpu_rust_quiesce() {
    PRODUCERS.fetch_and(ENTERED, Ordering::AcqRel);
    // Runtime closes reservations at session release. There is no engine event at quiesce.
    rsf_skyrim_cpu_clear_proof(false);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_cpu_rust_disable() -> bool {
    CONTROL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .disable()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_cpu_rust_stop() -> bool {
    let mut control = CONTROL.lock().unwrap_or_else(PoisonError::into_inner);
    if !control.disable() {
        return false;
    }
    // The caller handles the other subsystem refusals before calling stop. No further
    // native entries may borrow these services, but resident trampolines can still forward.
    *SERVICES.lock().unwrap_or_else(PoisonError::into_inner) = None;
    true
}

pub(crate) fn services() -> Option<Services> {
    *SERVICES.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn active() -> bool {
    PRODUCERS.load(Ordering::Acquire) & ACTIVE != 0
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_cpu_rust_busy() -> bool {
    PRODUCERS.load(Ordering::Acquire) & ENTERED != 0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_enter() {
    PRODUCERS.fetch_add(1, Ordering::AcqRel);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cpu_leave() {
    PRODUCERS.fetch_sub(1, Ordering::AcqRel);
}
