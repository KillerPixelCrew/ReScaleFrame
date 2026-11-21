// SPDX-License-Identifier: MIT
use super::Handle;
use super::attachment::{Query, query_interface};
use crate::com_ref::ComRef;
use crate::win32_diagnostics::module_location;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;

const WARNING: u32 = 2;
const ERROR: u32 = 1;
const STACK_DEPTH: usize = 16;
const MESSAGE_IDS: usize = 4096;
const BREADCRUMB_LIMIT: u32 = 64;

type Log = unsafe extern "C" fn(Handle, *const c_char);
type DebugQuery = unsafe extern "C" fn(&mut Handle) -> i32;

#[repr(C)]
#[derive(Default)]
struct Breadcrumb {
    next: *const c_void,
    list: Handle,
    queue: Handle,
    completed: u32,
    count: u32,
    name: *const c_char,
}

#[repr(C)]
struct DiagnosticSetup {
    query_settings: DebugQuery,
    enable_breadcrumbs: unsafe extern "C" fn(Handle),
    enable_page_faults: unsafe extern "C" fn(Handle),
    query_debug: DebugQuery,
    enable_debug: unsafe extern "C" fn(Handle) -> i32,
}

#[repr(C)]
struct Removal {
    queue_device: Query,
    removed_reason: unsafe extern "C" fn(Handle) -> i32,
    query_dred: Query,
    breadcrumb_head: unsafe extern "C" fn(Handle) -> *const c_void,
    breadcrumb: unsafe extern "C" fn(*const c_void, &mut Breadcrumb),
    operation: unsafe extern "C" fn(*const c_void, u32) -> u32,
    page_fault: unsafe extern "C" fn(Handle, &mut u64) -> i32,
}

const _: () = {
    assert!(size_of::<Breadcrumb>() == 40);
    assert!(std::mem::offset_of!(Breadcrumb, name) == 32);
    assert!(size_of::<DiagnosticSetup>() == 5 * size_of::<Handle>());
    assert!(size_of::<Removal>() == 7 * size_of::<Handle>());
};

fn query_debug(query: DebugQuery) -> Option<ComRef> {
    let mut object = null_mut();
    let result = unsafe { query(&mut object) };
    unsafe { ComRef::from_owned(object) }.filter(|_| result >= 0)
}

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
    platform: &DiagnosticSetup,
) -> i32 {
    let Some(log) = log else {
        return -1;
    };
    let selected = Sink { log, user };
    *SINK
        .get_or_init(|| Mutex::new(selected))
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = selected;
    let settings = query_debug(platform.query_settings);
    if let Some(settings) = &settings {
        unsafe {
            (platform.enable_breadcrumbs)(settings.as_ptr());
            (platform.enable_page_faults)(settings.as_ptr());
        }
    }
    let debug = query_debug(platform.query_debug);
    let enabled =
        debug.is_some_and(|debug| unsafe { (platform.enable_debug)(debug.as_ptr()) } != 0);
    drop(settings);
    if !enabled {
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
extern "C" fn rsf_core_observer12_report_removal(queue: Handle, platform: &Removal) {
    if !enabled() || REMOVAL_REPORTED.load(Ordering::Relaxed) {
        return;
    }
    let Some(device) = query_interface(platform.queue_device, queue) else {
        return;
    };
    let reason = unsafe { (platform.removed_reason)(device.as_ptr()) };
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
    report_dred(device.as_ptr(), platform, sink);
}

fn report_dred(device: Handle, platform: &Removal, sink: Sink) {
    let Some(dred) = query_interface(platform.query_dred, device) else {
        return;
    };
    let mut node = unsafe { (platform.breadcrumb_head)(dred.as_ptr()) };
    for _ in 0..BREADCRUMB_LIMIT {
        if node.is_null() {
            break;
        }
        let mut row = Breadcrumb::default();
        unsafe { (platform.breadcrumb)(node, &mut row) };
        let next_operation = if row.completed < row.count {
            unsafe { (platform.operation)(node, row.completed) }
        } else {
            u32::MAX
        };
        let name = unsafe { message(row.name) };
        sink.line(format!(
            "DRED list={:p} queue={:p} completed={}/{} next_operation={next_operation} name={name}.",
            row.list, row.queue, row.completed, row.count
        ));
        node = row.next;
    }
    let mut address = 0;
    if unsafe { (platform.page_fault)(dred.as_ptr(), &mut address) } >= 0 {
        sink.line(format!("DRED page fault address=0x{address:x}."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{add_ref, fake, raw};
    use std::cell::{Cell, RefCell};
    use std::ptr::without_provenance;

    thread_local! {
        static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static OPERATIONS: Cell<u32> = const { Cell::new(0) };
    }

    unsafe extern "C" fn query(object: Handle, out: &mut Handle) -> i32 {
        unsafe { add_ref(object.cast()) };
        *out = object;
        0
    }

    unsafe extern "C" fn reason(_: Handle) -> i32 {
        -1
    }

    unsafe extern "C" fn head(_: Handle) -> *const c_void {
        without_provenance(1)
    }

    unsafe extern "C" fn row(node: *const c_void, out: &mut Breadcrumb) {
        let first = node.addr() == 1;
        *out = Breadcrumb {
            next: without_provenance(if first { 2 } else { 1 }),
            completed: u32::from(!first),
            count: 1,
            name: c"fixture".as_ptr(),
            ..Breadcrumb::default()
        };
    }

    unsafe extern "C" fn operation(_: *const c_void, _: u32) -> u32 {
        OPERATIONS.set(OPERATIONS.get() + 1);
        9
    }

    unsafe extern "C" fn fault(_: Handle, out: &mut u64) -> i32 {
        *out = 0xabcd;
        0
    }

    unsafe extern "C" fn log(_: Handle, text: *const c_char) {
        LINES.with_borrow_mut(|lines| lines.push(unsafe { message(text) }.into_owned()));
    }

    #[test]
    fn dred_walk_is_bounded_and_skips_operations_at_the_end_of_a_list() {
        let device = fake();
        let platform = Removal {
            queue_device: query,
            removed_reason: reason,
            query_dred: query,
            breadcrumb_head: head,
            breadcrumb: row,
            operation,
            page_fault: fault,
        };
        let sink = Sink {
            log,
            user: null_mut(),
        };
        report_dred(raw(&device), &platform, sink);
        assert_eq!(device.refs.get(), 1);
        assert_eq!(OPERATIONS.get(), 32);
        LINES.with_borrow(|lines| {
            assert_eq!(lines.len(), 65);
            assert!(lines[0].ends_with("completed=0/1 next_operation=9 name=fixture."));
            assert!(lines[1].ends_with("completed=1/1 next_operation=4294967295 name=fixture."));
            assert_eq!(lines[64], "DRED page fault address=0xabcd.");
        });
    }
}
