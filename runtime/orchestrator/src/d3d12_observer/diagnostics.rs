// SPDX-License-Identifier: MIT
use super::Handle;
use crate::win32_diagnostics::module_location;
use std::ffi::{CStr, CString, c_char};
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;

const WARNING: u32 = 2;
const ERROR: u32 = 1;
const STACK_DEPTH: usize = 16;
const MESSAGE_IDS: usize = 4096;
const BREADCRUMB_LIMIT: u32 = 64;

type Log = unsafe extern "C" fn(Handle, *const c_char);
type ReadDred = unsafe extern "C" fn(
    Handle,
    u32,
    unsafe extern "C" fn(Handle, &Breadcrumb),
    unsafe extern "C" fn(Handle, u64),
    Handle,
);

#[repr(C)]
struct Breadcrumb {
    list: Handle,
    queue: Handle,
    completed: u32,
    count: u32,
    next_operation: u32,
    name: *const c_char,
}

const _: () = assert!(size_of::<Breadcrumb>() == 40);

#[derive(Clone, Copy)]
struct Sink {
    log: Log,
    user: Handle,
}

// The diagnostics entry borrows a process-lifetime context that the caller
// supplies for invocation by device and queue callbacks on other threads.
unsafe impl Send for Sink {}

impl Sink {
    fn say(self, message: &CStr) {
        unsafe { (self.log)(self.user, message.as_ptr()) };
    }

    fn line(self, message: String) {
        if let Ok(message) = CString::new(message) {
            self.say(&message);
        }
    }
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();
static REMOVAL_REPORTED: AtomicBool = AtomicBool::new(false);
static COUNTS: [AtomicU8; MESSAGE_IDS] = [const { AtomicU8::new(0) }; MESSAGE_IDS];

fn sink() -> Option<Sink> {
    SINK.get()
        .map(|sink| *sink.lock().unwrap_or_else(PoisonError::into_inner))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer12_diagnostics_enable(
    log: Option<Log>,
    user: Handle,
    enable: unsafe extern "C" fn() -> i32,
) -> i32 {
    let Some(log) = log else {
        return -1;
    };
    let selected = Sink { log, user };
    *SINK
        .get_or_init(|| Mutex::new(selected))
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = selected;
    if unsafe { enable() } == 0 {
        selected.say(c"D3D12 diagnostics: debug layer unavailable; DRED requested.");
        return -2;
    }
    selected.say(c"D3D12 diagnostics: validation layer and DRED requested before device creation.");
    0
}

pub(super) fn enabled() -> bool {
    SINK.get().is_some()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer12_debug_message(
    severity: u32,
    id: u32,
    description: *const c_char,
) {
    if severity > WARNING || SINK.get().is_none() {
        return;
    }
    let Some(counter) = COUNTS.get(id as usize) else {
        return;
    };
    let Ok(previous) = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        (count < 16).then(|| count + 1)
    }) else {
        return;
    };
    if severity == WARNING && previous >= 4 {
        return;
    }
    let Some(sink) = sink() else {
        return;
    };
    let description = unsafe { message(description) };
    sink.line(format!(
        "D3D12 validation severity={severity} id={id}: {description}"
    ));
    if severity <= ERROR && previous < 2 {
        let mut stack = [null_mut(); STACK_DEPTH];
        let count = unsafe {
            RtlCaptureStackBackTrace(1, STACK_DEPTH as u32, stack.as_mut_ptr(), null_mut())
        };
        for (i, address) in stack.into_iter().take(usize::from(count)).enumerate() {
            if let Some((module, offset)) = module_location(address.addr()) {
                sink.line(format!(
                    "D3D12 validation stack[{i}] {module}+0x{offset:x}."
                ));
            }
        }
    }
}

unsafe fn message<'a>(text: *const c_char) -> std::borrow::Cow<'a, str> {
    if text.is_null() {
        "".into()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_observer12_check_removal() -> i32 {
    i32::from(SINK.get().is_some() && !REMOVAL_REPORTED.load(Ordering::Relaxed))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer12_report_removal(
    device: Handle,
    reason: i32,
    read_dred: ReadDred,
) {
    if reason >= 0 {
        return;
    }
    let Some(sink) = sink() else {
        return;
    };
    if REMOVAL_REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    sink.line(format!(
        "D3D12 device removal reason=0x{:08x}.",
        reason as u32
    ));
    unsafe {
        read_dred(
            device,
            BREADCRUMB_LIMIT,
            breadcrumb,
            page_fault,
            ptr::from_ref(&sink).cast_mut().cast(),
        )
    };
}

unsafe extern "C" fn breadcrumb(user: Handle, row: &Breadcrumb) {
    let sink = unsafe { *user.cast::<Sink>() };
    let name = unsafe { message(row.name) };
    sink.line(format!(
        "DRED list={:p} queue={:p} completed={}/{} next_operation={} name={name}.",
        row.list, row.queue, row.completed, row.count, row.next_operation
    ));
}

unsafe extern "C" fn page_fault(user: Handle, address: u64) {
    let sink = unsafe { *user.cast::<Sink>() };
    sink.line(format!("DRED page fault address=0x{address:x}."));
}
