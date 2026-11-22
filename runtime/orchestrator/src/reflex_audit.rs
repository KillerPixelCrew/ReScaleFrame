// SPDX-License-Identifier: MIT
use crate::diagnostic_output::Output;
use crate::sys::rsf_backend_log_fn as Log;
use crate::win32_diagnostics::module_location;
use std::arch::naked_asm;
use std::ffi::{CStr, CString, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows::core::w;
use windows_sys::Win32::{
    System::{
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW, GetModuleHandleW, GetProcAddress,
        },
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

// bindgen output: bitfield accessors.
#[allow(clippy::ptr_offset_with_cast)]
#[path = "reflex_nvapi_abi.rs"]
mod nvapi;
use nvapi::*;

type Query = unsafe extern "C" fn(u32) -> *mut c_void;
type SleepStatus = unsafe extern "C" fn(*mut c_void, &mut NV_GET_SLEEP_STATUS_PARAMS) -> i32;
// NvAPI_D3D_SetSleepMode, SetLatencyMarker, SetReflexSync and
// NvAPI_D3D12_SetAsyncFrameMarker all take an interface and a versioned record.
type Entry = unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32;
const SOURCES: usize = 64;
const LINE_LIMIT: u32 = 192;
const HOOKS: usize = 4;
const NVAPI_NO_IMPLEMENTATION: i32 = -3;

// Record sizes and versions pinned by the bindgen records from nvapi.h.
const _: () = {
    assert!(version::<NV_GET_SLEEP_STATUS_PARAMS>() == 0x10088);
    assert!(version::<NV_SET_SLEEP_MODE_PARAMS>() == 0x1002c);
    assert!(version::<NV_LATENCY_MARKER_PARAMS>() == 0x10058);
    assert!(version::<NV_SET_REFLEX_SYNC_PARAMS>() == 0x10030);
    assert!(version::<NV_ASYNC_FRAME_MARKER_PARAMS>() == 0x10058);
    assert!(std::mem::offset_of!(NV_GET_SLEEP_STATUS_PARAMS, sleepIntervalUs) == 8);
    assert!(std::mem::offset_of!(NV_GET_SLEEP_STATUS_PARAMS, dfgFrameTimeTargetUs) == 16);
    assert!(std::mem::offset_of!(NV_SET_SLEEP_MODE_PARAMS, minimumIntervalUs) == 8);
    assert!(std::mem::offset_of!(NV_SET_SLEEP_MODE_PARAMS, bUseMinQueueTime) == 13);
    assert!(std::mem::offset_of!(NV_LATENCY_MARKER_PARAMS, frameID) == 8);
    assert!(std::mem::offset_of!(NV_LATENCY_MARKER_PARAMS, markerType) == 16);
    assert!(std::mem::offset_of!(NV_SET_REFLEX_SYNC_PARAMS, vblankIntervalUs) == 8);
    assert!(std::mem::offset_of!(NV_SET_REFLEX_SYNC_PARAMS, timeInQueueUs) == 12);
    assert!(std::mem::offset_of!(NV_SET_REFLEX_SYNC_PARAMS, dfgTargetFps) == 24);
    assert!(std::mem::offset_of!(NV_ASYNC_FRAME_MARKER_PARAMS, frameID) == 8);
    assert!(std::mem::offset_of!(NV_ASYNC_FRAME_MARKER_PARAMS, markerType) == 16);
    assert!(std::mem::offset_of!(NV_ASYNC_FRAME_MARKER_PARAMS, presentFrameID) == 24);
    assert!(std::mem::offset_of!(NV_ASYNC_FRAME_MARKER_PARAMS, vendorInternal) == 32);
};

// MinHook's API is WINAPI, as d3d11_observer_hooks declares it.
unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;

// MinHook writes each trampoline before its hook is enabled.
static ORIGINALS: [AtomicPtr<c_void>; HOOKS] = [const { AtomicPtr::new(null_mut()) }; HOOKS];

// Each entry passes the actual native caller in r8 and its audit kind in r9.
// The two-argument NVAPI entries leave both registers and their shadow slots free.
macro_rules! detour {
    ($name:ident, $kind:literal) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name(_: *mut c_void, _: *mut c_void) -> i32 {
            naked_asm!("mov r8, [rsp]", "mov r9d, {kind}", "jmp {observe}", kind = const $kind, observe = sym observe);
        }
    };
}

detour!(observe_mode, 0);
detour!(observe_marker, 1);
detour!(observe_sync, 2);
detour!(observe_async, 3);

const DETOURS: [Entry; HOOKS] = [observe_mode, observe_marker, observe_sync, observe_async];

// Call the original trampoline once, then report what it saw and returned.
unsafe extern "C" fn observe(
    object: *mut c_void,
    params: *mut c_void,
    caller: usize,
    kind: u32,
) -> i32 {
    let original = ORIGINALS[kind as usize].load(Ordering::Acquire);
    if original.is_null() {
        return NVAPI_NO_IMPLEMENTATION;
    }
    let original = unsafe { std::mem::transmute::<*mut c_void, Entry>(original) };
    let result = unsafe { original(object, params) };
    unsafe { observe_nvapi(kind, caller, object, params, result) };
    result
}

// Borrow the module already owned by Streamline. The audit pins installed hook
// targets and never obtains a second driver loader reference.
fn query_interface() -> Option<Query> {
    let module = unsafe { GetModuleHandleW(w!("nvapi64.dll").as_ptr()) };
    if module.is_null() {
        return None;
    }
    let address = unsafe { GetProcAddress(module, c"nvapi_QueryInterface".as_ptr().cast()) }?;
    Some(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(address) })
}

#[derive(Clone, Copy)]
struct Hook {
    target: *mut c_void,
    detour: *mut c_void,
    original: *mut *mut c_void,
}

// Hook operations stay replaceable so the install order can be tested without a driver.
struct Platform {
    initialize: unsafe fn() -> bool,
    create: unsafe fn(&Hook) -> bool,
    pin: unsafe fn(*mut c_void) -> bool,
    enable: unsafe fn(*mut c_void) -> bool,
    disable: unsafe fn(*mut c_void),
    remove: unsafe fn(*mut c_void),
}

const MINHOOK: Platform = Platform {
    initialize: || {
        let result = unsafe { MH_Initialize() };
        result == MH_OK || result == MH_ERROR_ALREADY_INITIALIZED
    },
    create: |hook| unsafe { MH_CreateHook(hook.target, hook.detour, hook.original) == MH_OK },
    pin: |address| {
        let mut module = null_mut();
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                address.cast(),
                &mut module,
            ) != 0
        }
    },
    enable: |target| unsafe { MH_EnableHook(target) == MH_OK },
    disable: |target| unsafe {
        MH_DisableHook(target);
    },
    remove: |target| unsafe {
        MH_RemoveHook(target);
    },
};

fn describe(targets: [*mut c_void; HOOKS]) -> [Hook; HOOKS] {
    std::array::from_fn(|index| Hook {
        target: targets[index],
        detour: DETOURS[index] as *mut c_void,
        original: ORIGINALS[index].as_ptr(),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Install {
    Retry,
    Installed,
    Missing,
    Refused,
    Retained,
}

fn install(platform: &Platform) -> Install {
    let Some(query) = query_interface() else {
        return Install::Missing;
    };
    let targets =
        [0xac1c_a9e0, 0xd998_4c05, 0xb9f6_faff, 0x13c9_8f73].map(|id| unsafe { query(id) });
    install_hooks(platform, &describe(targets))
}

fn install_hooks(platform: &Platform, hooks: &[Hook; HOOKS]) -> Install {
    if !unsafe { (platform.initialize)() } {
        return Install::Retry;
    }
    let made = hooks
        .iter()
        .take_while(|hook| !hook.target.is_null() && unsafe { (platform.create)(hook) })
        .count();
    // Pinning the first detour keeps this module, and every detour, resident.
    let pinned = made == HOOKS
        && hooks
            .iter()
            .all(|hook| unsafe { (platform.pin)(hook.target) })
        && unsafe { (platform.pin)(hooks[0].detour) };
    if !pinned {
        for hook in &hooks[..made] {
            unsafe { (platform.remove)(hook.target) };
        }
        return Install::Refused;
    }
    if hooks
        .iter()
        .all(|hook| unsafe { (platform.enable)(hook.target) })
    {
        return Install::Installed;
    }
    // A failed enable can follow a reachable hook. Leave the pinned original
    // trampolines in place even if every target now disables successfully.
    for hook in hooks {
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

    fn emit(self, text: &CStr) {
        if let Some(log) = self.log {
            unsafe { log(self.user as *mut c_void, text.as_ptr()) };
        }
    }

    fn formatted(self, text: String) {
        if let Ok(text) = CString::new(text) {
            self.emit(&text);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Source {
    caller: usize,
    object: usize,
    kind: u32,
}

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

// Optional process-lifetime observation. It never changes arguments, return
// values or hook chains.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_reflex_audit_start(log: Log, user: *mut c_void) {
    start(log, user, &MINHOOK);
}

fn start(log: Log, user: *mut c_void, platform: &Platform) {
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
    listener.emit(message);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_reflex_audit_stop() {
    // If the listener calls stop, its own thread already owns the output lock.
    let _output = Output::enter();
    audit().listener = Listener::NONE;
}

fn event(source: Source, settings: u64, format: impl FnOnce() -> String) {
    let Some(_output) = Output::enter() else {
        return;
    };
    let listener = {
        let mut audit = audit();
        if audit.listener.log.is_none()
            || audit.lines >= LINE_LIMIT
            || !audit.changed(source, settings)
        {
            return;
        }
        audit.lines += 1;
        audit.listener
    };
    listener.formatted(format());
}

fn sync(mut report: SyncReport, enable: u32, disable: u32) {
    let mut audit = audit();
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

fn report_sync() {
    let Some(_output) = Output::enter() else {
        return;
    };
    let (listener, report) = {
        let audit = audit();
        (audit.listener, audit.sync)
    };
    if let Some(report) = report.filter(|report| report.caller != 0) {
        let origin = origin(report.caller);
        let enabled = match report.enabled {
            0 => "0",
            1 => "1",
            _ => "unknown",
        };
        listener.formatted(format!(
            "Reflex API sync caller={origin} device=0x{:x} enabled={enabled} queue_us={} target_us={} vblank_us={} fg={} dfg_max={} dfg_fps={} result={}",
            report.device, report.queue_us, report.target_us, report.vblank_us, report.fg,
            report.dfg_max, report.dfg_fps, report.result,
        ));
    }
}

fn origin(caller: usize) -> String {
    let (name, offset) = module_location(caller).unwrap_or_else(|| ("unknown".into(), caller));
    format!("{name}+0x{offset:x}")
}

const fn version<T>() -> u32 {
    size_of::<T>() as u32 | (1 << 16)
}

// Every intercepted SDK record starts with its version. Inspect only that word
// until the caller's record size and revision match the pinned declaration.
unsafe fn params<T>(raw: *const c_void) -> Option<*const T> {
    if raw.is_null() || unsafe { raw.cast::<u32>().read() } != version::<T>() {
        return None;
    }
    Some(raw.cast())
}

unsafe fn observe_nvapi(
    kind: u32,
    caller: usize,
    object: *mut c_void,
    raw: *const c_void,
    result: i32,
) {
    let source = |kind| Source {
        caller,
        object: object.addr(),
        kind,
    };
    let thread = || unsafe { GetCurrentThreadId() };
    match kind {
        0 => {
            let Some(p) = (unsafe { params::<NV_SET_SLEEP_MODE_PARAMS>(raw) }) else {
                return;
            };
            let p = unsafe { &*p };
            let settings = u64::from(p.minimumIntervalUs) << 32
                | u64::from(p.bLowLatencyMode)
                | u64::from(p.bLowLatencyBoost) << 1
                | u64::from(p.bUseMarkersToOptimize) << 2
                | u64::from(p.bUseMinQueueTime) << 3;
            event(source(0), settings, || {
                format!(
                    "Reflex API mode caller={} device={object:p} low_latency={} boost={} minimum_us={} marker_opt={} min_queue={} result={result} thread={}",
                    origin(caller),
                    p.bLowLatencyMode,
                    p.bLowLatencyBoost,
                    p.minimumIntervalUs,
                    p.bUseMarkersToOptimize,
                    p.bUseMinQueueTime,
                    thread(),
                )
            });
        }
        1 => {
            let Some(p) = (unsafe { params::<NV_LATENCY_MARKER_PARAMS>(raw) }) else {
                return;
            };
            let p = unsafe { &*p };
            let marker = p.markerType as u32;
            let high_bits = p.frameID >> 32;
            event(source(marker.wrapping_add(1)), high_bits, || {
                format!(
                    "Reflex API marker source caller={} device={object:p} marker={marker} frame={} high_bits={high_bits} result={result} thread={}",
                    origin(caller),
                    p.frameID,
                    thread(),
                )
            });
        }
        2 => {
            let Some(p) = (unsafe { params::<NV_SET_REFLEX_SYNC_PARAMS>(raw) }) else {
                return;
            };
            let p = unsafe { &*p };
            sync(
                SyncReport {
                    caller,
                    device: object.addr(),
                    queue_us: p.timeInQueueUs,
                    target_us: p.timeInQueueUsTarget,
                    vblank_us: p.vblankIntervalUs,
                    fg: p.fgMultiplier.into(),
                    dfg_max: p.dfgMaxMultiplier.into(),
                    dfg_fps: p.dfgTargetFps,
                    result,
                    enabled: 0,
                },
                p.bEnable(),
                p.bDisable(),
            );
        }
        3 => {
            let Some(p) = (unsafe { params::<NV_ASYNC_FRAME_MARKER_PARAMS>(raw) }) else {
                return;
            };
            let p = unsafe { &*p };
            let marker = p.markerType as u32;
            event(
                source(marker.wrapping_add(100)),
                p.vendorInternal.into(),
                || {
                    format!(
                        "Reflex API async source caller={} queue={object:p} marker={marker} frame={} present_frame={} vendor={} result={result} thread={}",
                        origin(caller),
                        p.frameID,
                        p.presentFrameID,
                        p.vendorInternal,
                        thread(),
                    )
                },
            );
        }
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_reflex_driver_sleep(
    device: *mut c_void,
    log: Log,
    user: *mut c_void,
) {
    let mut state = NV_GET_SLEEP_STATUS_PARAMS {
        version: version::<NV_GET_SLEEP_STATUS_PARAMS>(),
        ..Default::default()
    };
    let address = query_interface().map_or(null_mut(), |query| unsafe { query(0xaef9_6ca1) });
    let result = if address.is_null() {
        NVAPI_NO_IMPLEMENTATION
    } else {
        let status = unsafe { std::mem::transmute::<*mut c_void, SleepStatus>(address) };
        unsafe { status(device, &mut state) }
    };
    if log.is_some() {
        let mut foreground = 0;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground) };
        let foreground = u32::from(foreground == unsafe { GetCurrentProcessId() });
        Listener { log, user: user.addr() }.formatted(format!(
            "Reflex driver sleep status={result} low_latency={} sleep_interval_us={} app_called_sleep={} vrr={} cpl_vsync={} fullscreen_iflip={} fg_multiplier={} dfg={} dfg_target_us={} foreground={foreground}",
            state.bLowLatencyMode, state.sleepIntervalUs, state.bUseGameSleep, state.bFsVrr,
            state.bCplVsyncOn, state.bFullscreenIFlip, state.fgMultiplier, state.bDfgControl,
            state.dfgFrameTimeTargetUs,
        ));
    }
    report_sync();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ptr;

    thread_local! {
        static STEPS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn record(step: &'static str) {
        STEPS.with_borrow_mut(|steps| steps.push(step));
    }

    const RECORDING: Platform = Platform {
        initialize: || true,
        create: |_| {
            record("create");
            true
        },
        pin: |_| {
            record("pin");
            true
        },
        enable: |target| {
            record("enable");
            target.addr() == 1
        },
        disable: |_| record("disable"),
        remove: |_| record("remove"),
    };

    fn hooks() -> [Hook; HOOKS] {
        std::array::from_fn(|index| Hook {
            target: ptr::without_provenance_mut(index + 1),
            detour: ptr::without_provenance_mut(index + 5),
            original: ptr::null_mut(),
        })
    }

    #[test]
    fn detours_capture_their_own_original_slots() {
        let hooks = describe([ptr::without_provenance_mut(9); HOOKS]);
        for (index, hook) in hooks.iter().enumerate() {
            assert_eq!(hook.detour, DETOURS[index] as *mut c_void);
            assert_eq!(hook.original, ORIGINALS[index].as_ptr());
        }
    }

    #[test]
    fn partially_exposed_hooks_disable_without_removing_trampolines() {
        assert!(matches!(
            install_hooks(&RECORDING, &hooks()),
            Install::Retained
        ));
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
