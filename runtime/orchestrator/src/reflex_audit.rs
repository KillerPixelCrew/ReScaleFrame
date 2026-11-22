// SPDX-License-Identifier: MIT
use crate::diagnostic_output::Output;
use crate::render_abi::rsf_backend_log_fn as Log;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows::core::w;
use windows_sys::Win32::{
    Foundation::FARPROC,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};

type Emit = unsafe extern "C" fn(Log, *mut c_void, *const c_void);
const SOURCES: usize = 64;
const LINE_LIMIT: u32 = 192;
const HOOKS: usize = 4;

// Borrow the module already owned by Streamline. The audit pins installed hook
// targets and never obtains a second driver loader reference.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_reflex_audit_query() -> FARPROC {
    let module = unsafe { GetModuleHandleW(w!("nvapi64.dll").as_ptr()) };
    if module.is_null() {
        return None;
    }
    unsafe { GetProcAddress(module, c"nvapi_QueryInterface".as_ptr().cast()) }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Hook {
    target: *mut c_void,
    detour: *mut c_void,
    original: *mut *mut c_void,
}

#[repr(C)]
struct Platform {
    discover: unsafe extern "C" fn(*mut Hook) -> u32,
    initialize: unsafe extern "C" fn() -> u32,
    create: unsafe extern "C" fn(&Hook) -> u32,
    pin: unsafe extern "C" fn(*mut c_void) -> u32,
    enable: unsafe extern "C" fn(*mut c_void) -> u32,
    disable: unsafe extern "C" fn(*mut c_void),
    remove: unsafe extern "C" fn(*mut c_void),
}

const _: () = {
    assert!(size_of::<Hook>() == 3 * size_of::<*mut c_void>());
    assert!(size_of::<Platform>() == 7 * size_of::<*mut c_void>());
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Install {
    Retry,
    Installed,
    Missing,
    Refused,
    Retained,
}

fn install(platform: &Platform) -> Install {
    let mut hooks = [Hook::default(); HOOKS];
    if unsafe { (platform.discover)(hooks.as_mut_ptr()) } == 0 {
        return Install::Missing;
    }
    if unsafe { (platform.initialize)() } == 0 {
        return Install::Retry;
    }
    let made = hooks
        .iter()
        .take_while(|hook| !hook.target.is_null() && unsafe { (platform.create)(hook) } != 0)
        .count();
    let pinned = made == HOOKS
        && hooks
            .iter()
            .all(|hook| unsafe { (platform.pin)(hook.target) } != 0)
        && unsafe { (platform.pin)(hooks[0].detour) } != 0;
    if !pinned {
        for hook in &hooks[..made] {
            unsafe { (platform.remove)(hook.target) };
        }
        return Install::Refused;
    }
    if hooks
        .iter()
        .all(|hook| unsafe { (platform.enable)(hook.target) } != 0)
    {
        return Install::Installed;
    }
    // A failed enable can follow a reachable hook. Leave the pinned original
    // trampolines in place even if every target now disables successfully.
    for hook in &hooks {
        unsafe { (platform.disable)(hook.target) };
    }
    Install::Retained
}

#[derive(Clone, Copy)]
struct Listener {
    log: Log,
    user: usize,
}

impl Listener {
    const NONE: Self = Self { log: None, user: 0 };

    unsafe fn emit(self, payload: *const c_void, emit: Emit) {
        if self.log.is_some() {
            unsafe { emit(self.log, self.user as *mut c_void, payload) };
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Source {
    caller: usize,
    object: usize,
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SyncReport {
    caller: usize,
    device: usize,
    queue_us: i32,
    target_us: u32,
    vblank_us: u32,
    fg: u32,
    dfg_max: u32,
    dfg_fps: u32,
    result: i32,
    enabled: u32,
}

const _: () = assert!(size_of::<SyncReport>() == 48);

struct Audit {
    listener: Listener,
    install: Install,
    seen: [Option<(Source, u64)>; SOURCES],
    lines: u32,
    sync: Option<SyncReport>,
}

impl Audit {
    fn changed(&mut self, source: Source, settings: u64) -> bool {
        if let Some((_, previous)) = self
            .seen
            .iter_mut()
            .flatten()
            .find(|(seen, _)| *seen == source)
        {
            return std::mem::replace(previous, settings) != settings;
        }
        let Some(entry) = self.seen.iter_mut().find(|entry| entry.is_none()) else {
            return false;
        };
        *entry = Some((source, settings));
        true
    }
}

static AUDIT: Mutex<Audit> = Mutex::new(Audit {
    listener: Listener::NONE,
    install: Install::Retry,
    seen: [None; SOURCES],
    lines: 0,
    sync: None,
});

fn audit() -> MutexGuard<'static, Audit> {
    AUDIT.lock().unwrap_or_else(PoisonError::into_inner)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_reflex_audit_start(
    log: Log,
    user: *mut c_void,
    platform: &Platform,
    text: Emit,
) {
    let Some(_output) = Output::enter() else {
        return;
    };
    let listener = Listener {
        log,
        user: user as usize,
    };
    {
        let mut audit = audit();
        if audit.install == Install::Retained {
            return;
        }
        audit.listener = listener;
        audit.seen = [None; SOURCES];
        audit.lines = 0;
        audit.sync = None;
        if audit.install == Install::Installed {
            return;
        }
    }
    let result = install(platform);
    let mut state = audit();
    state.install = result;
    if result == Install::Retained {
        state.listener = Listener::NONE;
    }
    drop(state);
    let message = match result {
        Install::Installed => {
            c"Reflex API audit installed: observing mode, sync and marker callers without filtering"
        }
        Install::Missing => c"Reflex API audit unavailable: NVAPI module/export absent",
        Install::Refused | Install::Retained => {
            c"Reflex API audit refused: existing entry cannot be chained safely"
        }
        _ => return,
    };
    unsafe { listener.emit(message.as_ptr().cast(), text) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_reflex_audit_stop() {
    // If the listener calls stop, its own thread already owns the output lock.
    let _output = Output::enter();
    audit().listener = Listener::NONE;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_reflex_audit_event(
    caller: usize,
    object: usize,
    kind: u32,
    settings: u64,
    payload: *const c_void,
    emit: Emit,
) {
    let Some(_output) = Output::enter() else {
        return;
    };
    let listener = {
        let mut audit = audit();
        if audit.listener.log.is_none()
            || audit.lines >= LINE_LIMIT
            || !audit.changed(
                Source {
                    caller,
                    object,
                    kind,
                },
                settings,
            )
        {
            return;
        }
        audit.lines += 1;
        audit.listener
    };
    unsafe { listener.emit(payload, emit) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_reflex_audit_sync(report: &SyncReport, enable: u32, disable: u32) {
    let mut audit = audit();
    let mut report = *report;
    report.enabled = audit
        .sync
        .filter(|prior| prior.device == report.device)
        .map_or(u32::MAX, |prior| prior.enabled);
    if report.result == 0 {
        if enable != 0 {
            report.enabled = 1;
        }
        if disable != 0 {
            report.enabled = 0;
        }
    }
    audit.sync = Some(report);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_reflex_audit_report(emit: Emit) {
    let Some(_output) = Output::enter() else {
        return;
    };
    let (listener, report) = {
        let audit = audit();
        (audit.listener, audit.sync)
    };
    if let Some(report) = report.filter(|report| report.caller != 0) {
        unsafe { listener.emit(ptr::from_ref(&report).cast(), emit) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static STEPS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn record(step: &'static str) {
        STEPS.with_borrow_mut(|steps| steps.push(step));
    }

    unsafe extern "C" fn discover(output: *mut Hook) -> u32 {
        for index in 0..HOOKS {
            unsafe {
                output.add(index).write(Hook {
                    target: ptr::without_provenance_mut(index + 1),
                    detour: ptr::without_provenance_mut(index + 5),
                    original: ptr::null_mut(),
                });
            }
        }
        1
    }

    unsafe extern "C" fn initialize() -> u32 {
        1
    }

    unsafe extern "C" fn create(_: &Hook) -> u32 {
        record("create");
        1
    }

    unsafe extern "C" fn pin(_: *mut c_void) -> u32 {
        record("pin");
        1
    }

    unsafe extern "C" fn enable(target: *mut c_void) -> u32 {
        record("enable");
        u32::from(target.addr() == 1)
    }

    unsafe extern "C" fn disable(_: *mut c_void) {
        record("disable");
    }

    unsafe extern "C" fn remove(_: *mut c_void) {
        record("remove");
    }

    #[test]
    fn partially_exposed_hooks_disable_without_removing_trampolines() {
        let platform = Platform {
            discover,
            initialize,
            create,
            pin,
            enable,
            disable,
            remove,
        };
        assert!(matches!(install(&platform), Install::Retained));
        STEPS.with_borrow(|steps| {
            assert_eq!(
                *steps,
                [
                    "create", "create", "create", "create", "pin", "pin", "pin", "pin", "pin",
                    "enable", "enable", "disable", "disable", "disable", "disable"
                ],
            );
        });
    }
}
