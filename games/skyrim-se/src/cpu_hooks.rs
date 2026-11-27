// SPDX-License-Identifier: MIT
//! Admission and lifetime of the 13 CPU producer hooks for Skyrim SE at their discovered sites.
//! Native detours keep the engine ABI, caller addresses and original calls on their own frames.

use crate::cpu_policy::bad_screen_query;
use crate::cpu_sources::clear_proof;
use crate::discovery::{self, Site, rva};
use crate::hook_set::HookSet;
use crate::platform::Platform;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use windows_sys::Win32::System::Performance::QueryPerformanceFrequency;

// Indexed by the native detour table in cpu_events.c. Each is verified at its discovered site.
const HOOKS: [Site; 13] = [
    Site::Tick,
    Site::Poll,
    Site::Render,
    Site::Dispatch,
    Site::World,
    Site::Controls,
    Site::CameraUpdate,
    Site::JobsSubmit,
    Site::JobsJoin,
    Site::NamedPoll,
    Site::Movement,
    Site::PlayerUpdate,
    Site::PlayerCameraUpdate,
];

const _: () = assert!(HOOKS.len() == crate::sys::RSF_SKYRIM_CPU_HOOK_COUNT as usize);

const INDICES: std::ops::Range<usize> = 0..HOOKS.len();

// The C leaves own the native calls and unwind receipts. Rust binds their typed addresses to the
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
    let mut value = 0;
    if unsafe { QueryPerformanceFrequency(&mut value) } == 0 {
        return 0;
    }
    u64::try_from(value).unwrap_or(0)
}

#[derive(Clone, Copy)]
pub(crate) struct Services {
    pub(crate) host: rsf_game_host_services,
    pub(crate) frequency: u64,
}

// The SDK table borrows process services used by native CPU producer threads. Detours
// copy it under the lock within their entry lease. Start may copy it to log a refusal.
unsafe impl Send for Services {}

pub(crate) struct StartRefusal {
    pub(crate) stage: &'static str,
    pub(crate) rva: u32,
    pub(crate) status: i32,
}

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
    base + rva(HOOKS[index])
}

fn admission(platform: &Platform, base: usize) -> Result<(), StartRefusal> {
    // The engine must still call the discovered poll through its named callback slot.
    let named_slot = base.checked_add(rva(Site::NamedCallbackSlot));
    let named_poll = base.checked_add(rva(Site::NamedPoll));
    if base == 0
        || rva(Site::NamedPoll) == 0
        || named_slot.and_then(|address| platform.read_usize(address)) != named_poll
    {
        return Err(StartRefusal::new(
            "named callback",
            Site::NamedCallbackSlot,
            0,
        ));
    }
    if let Some(site) = bad_screen_query(platform, base) {
        return Err(StartRefusal::new("screen query bytes", site, 0));
    }
    for site in HOOKS {
        if !discovery::verify(platform, base, site) {
            return Err(StartRefusal::new("hook bytes", site, 0));
        }
    }
    Ok(())
}

impl StartRefusal {
    fn new(stage: &'static str, site: Site, status: i32) -> Self {
        Self {
            stage,
            rva: rva(site) as u32,
            status,
        }
    }

    fn unsited(stage: &'static str) -> Self {
        Self {
            stage,
            rva: 0,
            status: 0,
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
        if active() || busy() || !self.hooks.is_disabled() {
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
        *lock_or_recover(&SERVICES) = None;
        true
    }

    fn start(&mut self, platform: &Platform) -> Result<(), StartRefusal> {
        if active() || busy() {
            return Err(StartRefusal::unsited("active or entered"));
        }
        // Disabled targets are the only ones whose live prologues can be checked.
        if let Some(index) = self.hooks.first_unavailable() {
            return Err(StartRefusal::new("hook state", HOOKS[index], 0));
        }
        let base = platform.main_module();
        if !self.pinned || base != self.base || lock_or_recover(&SERVICES).is_none() {
            return Err(StartRefusal::unsited("hook state"));
        }
        admission(platform, base)?;

        clear_proof(true);
        if let Err((index, status)) = self.hooks.enable(|index| unsafe {
            rsf_windows::minhook::enable_status(target(base, index) as *mut c_void)
        }) {
            // Keep preparation and any hook whose disable refused so stop can retry.
            self.disable_hooks();
            return Err(StartRefusal::new("hook enable", HOOKS[index], status));
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
        quiesce();
        self.disable_hooks() && !busy()
    }
}

pub(crate) fn prepare() -> bool {
    let platform = &Platform;
    lock_or_recover(&CONTROL).prepare(platform)
}

pub(crate) fn bind(host: &rsf_game_host_services) -> bool {
    let control = lock_or_recover(&CONTROL);
    if control.frequency == 0 || !control.hooks.is_disabled() || busy() {
        return false;
    }
    *lock_or_recover(&SERVICES) = Some(Services {
        host: *host,
        frequency: control.frequency,
    });
    true
}

pub(crate) fn start() -> Result<(), StartRefusal> {
    lock_or_recover(&CONTROL).start(&Platform)
}

pub(crate) fn quiesce() {
    PRODUCERS.fetch_and(ENTERED, Ordering::AcqRel);
    // Runtime closes reservations at session release. There is no engine event at quiesce.
    clear_proof(false);
}

pub(crate) fn disable() -> bool {
    lock_or_recover(&CONTROL).disable()
}

pub(crate) fn stop() -> bool {
    let mut control = lock_or_recover(&CONTROL);
    if !control.disable() {
        return false;
    }
    // The caller handles the other subsystem refusals before calling stop. No further
    // native entries may borrow these services, but resident trampolines can still forward.
    *lock_or_recover(&SERVICES) = None;
    true
}

pub(crate) fn services() -> Option<Services> {
    *lock_or_recover(&SERVICES)
}

pub(crate) fn active() -> bool {
    PRODUCERS.load(Ordering::Acquire) & ACTIVE != 0
}

pub(crate) fn busy() -> bool {
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
