// SPDX-License-Identifier: MIT
use crate::diagnostic_output::Output;
use crate::render_abi::rsf_backend_log_fn as Log;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

type Emit = unsafe extern "C" fn(Log, *mut c_void, *const c_void);
const SOURCES: usize = 64;
const LINE_LIMIT: u32 = 192;
const INSTALLED: u32 = 1;
const MODULE_MISSING: u32 = 2;
const HOOK_REFUSED: u32 = 3;

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
    installed: bool,
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
    installed: false,
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
    install: unsafe extern "C" fn() -> u32,
    text: Emit,
) {
    let Some(_output) = Output::enter() else {
        return;
    };
    {
        let mut audit = audit();
        audit.listener = Listener {
            log,
            user: user as usize,
        };
        audit.seen = [None; SOURCES];
        audit.lines = 0;
        audit.sync = None;
        if audit.installed {
            return;
        }
    }
    let status = unsafe { install() };
    let message = match status {
        INSTALLED => {
            audit().installed = true;
            c"Reflex API audit installed: observing mode, sync and marker callers without filtering"
        }
        MODULE_MISSING => c"Reflex API audit unavailable: NVAPI module/export absent",
        HOOK_REFUSED => c"Reflex API audit refused: existing entry cannot be chained safely",
        _ => return,
    };
    let listener = audit().listener;
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
    report.enabled = audit.sync.map_or(0, |prior| prior.enabled);
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
