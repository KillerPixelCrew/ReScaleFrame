// SPDX-License-Identifier: MIT
//! Queued render scopes of the native renderer (ac7_render_scope.h): scope admission, capacity and
//! identity, scope serials, the begin/end stack with its state and pass callbacks, lease
//! resolve/release order, and quiesce, idle and destroy.
//!
//! A ticket is two private commands appended to one of the engine's RHI command lists: begin when
//! the scope opens and end when it closes. The engine executes them in list order on its RHI
//! stream, which is where the stack changes and the callbacks run. The list and command layouts
//! below are the verified ones of Ace7Game.exe build 9855922, moved unchanged from
//! render_scope.cpp. Research: docs/research/ac7-native-renderer-refactor-20261001.md,
//! docs/research/ac7-dlss-fg-20261003.md and docs/research/rust-ac7-render-scopes.md.
//!
//! The two C create calls pass `rsf_ac7_scope_calls` from render_scope_entry.c, whose SEH leaves call
//! consumer callbacks and lease functions with any C++ exception contained. Everything else is here,
//! including `execute`, the function the engine's command iterator calls.
use crate::sys::{rsf_ac7_render_scope, rsf_ac7_render_scope_fn};
use crate::sys::{rsf_ac7_scope_calls, rsf_ac7_scope_lease};
use rsf_game_sys::ffi_util::try_box;
use rsf_game_sys::{
    RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_TEXTURE_BINDING, RSF_GAME_RENDER_WINDOW,
};
use std::ffi::{c_int, c_void};
use std::mem::{offset_of, zeroed};
use std::ptr::{addr_of, addr_of_mut, null, null_mut};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Most tickets one owner admits at once.
const CAPACITY_LIMIT: u32 = 4096;

// FRHICommandListBase: root command +0, tail link +8 (the address the next command pointer is
// stored to), executing flag +0x10, NumCommands +0x14, UID +0x18 (untouched). The native iterator
// saves Next before invoking execute, which lets the end command free its own ticket. Engine
// reset owns only its memory stack, not these private records.
const LIST_TAIL: usize = 0x08;
const LIST_EXECUTING: usize = 0x10;
const LIST_COUNT: usize = 0x14;

type Execute = unsafe extern "C" fn(command_list: *mut c_void, command: *mut Command);
type Notify = unsafe extern "C" fn(
    rsf_ac7_render_scope_fn,
    *mut c_void,
    *mut c_void,
    *const rsf_ac7_render_scope,
    u32,
) -> bool;
type Resolve = unsafe extern "C" fn(*const rsf_ac7_scope_lease, *mut rsf_ac7_render_scope) -> bool;
type Release = unsafe extern "C" fn(*const rsf_ac7_scope_lease) -> bool;

/// One queued marker. The engine reads `next` and `execute` (FRHICommandBase); the rest is ours.
#[repr(C)]
struct Command {
    next: *mut Command,
    #[allow(dead_code)] // read by the engine's command iterator
    execute: Option<Execute>,
    ticket: *mut Ticket,
    begin: bool,
}
const _: () = assert!(offset_of!(Command, next) == 0 && offset_of!(Command, execute) == 8);

impl Command {
    const fn new(begin: bool) -> Self {
        Self {
            next: null_mut(),
            execute: Some(execute),
            ticket: null_mut(),
            begin,
        }
    }
}

/// `rsf_ac7_render_ticket`: a copied scope, its lease and both markers. It lives from a successful
/// open until its end marker executes. Fields are reached through raw pointers only, because the
/// RHI stream may run begin while the recording thread closes the ticket.
#[repr(C)]
pub(crate) struct Ticket {
    owner: *const Scopes,
    recording_list: *mut c_void,
    scope: rsf_ac7_render_scope,
    lease: rsf_ac7_scope_lease,
    begin: Command,
    end: Command,
    closed: bool,
}

const NO_LEASE: rsf_ac7_scope_lease = rsf_ac7_scope_lease {
    object: null_mut(),
    resolve: None,
    release: None,
};

#[derive(Clone, Copy)]
struct Calls {
    notify: Notify,
    resolve: Resolve,
    release: Release,
}

impl Calls {
    fn new(calls: &rsf_ac7_scope_calls) -> Option<Self> {
        Some(Self {
            notify: calls.notify?,
            resolve: calls.resolve?,
            release: calls.release?,
        })
    }
}

struct State {
    serial: u64,
    outstanding: u32,
    accepting: bool,
    /// Executed begin scopes, innermost last. Reserved for `capacity` at creation.
    stack: Vec<rsf_ac7_render_scope>,
}

/// `rsf_ac7_render_scopes`. Everything outside `state` is fixed at creation.
pub(crate) struct Scopes {
    session: u64,
    capacity: u32,
    callback: rsf_ac7_render_scope_fn,
    pass_callback: rsf_ac7_render_scope_fn,
    user: *mut c_void,
    calls: Calls,
    state: Mutex<State>,
}

impl Scopes {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing here panics while holding the lock; a poisoned state is still consistent.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Scene work needs its family and view. Slate window and texture-binding work has no scene view:
/// its queued ownership is the source, game viewport and window instead, and its resources
/// resolve from the lease at execution, so they need not exist while recording.
fn valid_identity(scope: &rsf_ac7_render_scope) -> bool {
    match scope.role {
        RSF_GAME_RENDER_FRAME => scope.source_frame_id != 0,
        RSF_GAME_RENDER_WINDOW | RSF_GAME_RENDER_TEXTURE_BINDING => {
            scope.source_frame_id != 0
                && scope.viewport_key != 0
                && scope.window_key != 0
                && scope.pass_key != 0
                && (scope.role != RSF_GAME_RENDER_WINDOW || scope.rhi_viewport_key != 0)
        }
        _ => scope.family_key != 0 && scope.view_key != 0,
    }
}

/// Links one command or a prelinked pair at the tail of a recording native list. Refuses an
/// executing list, a missing or occupied tail link, and insufficient count space.
///
/// # Safety
/// `native` is a nonnull command list owned by the calling thread. The markers remain live
/// until execution. When distinct, `first` already links to `last`.
unsafe fn append(native: *mut c_void, first: *mut Command, last: *mut Command) -> bool {
    let added = if first == last { 1 } else { 2 };
    let bytes = native.cast::<u8>();
    let tail_at = unsafe { bytes.add(LIST_TAIL) }.cast::<*mut *mut Command>();
    let count_at = unsafe { bytes.add(LIST_COUNT) }.cast::<u32>();
    let tail = unsafe { tail_at.read_unaligned() };
    let count = unsafe { count_at.read_unaligned() };
    if unsafe { bytes.add(LIST_EXECUTING).read() } != 0
        || tail.is_null()
        || unsafe { !tail.read().is_null() }
        || count > u32::MAX - added
    {
        return false;
    }
    unsafe {
        tail.write(first);
        tail_at.write_unaligned(addr_of_mut!((*last).next));
        count_at.write_unaligned(count + added);
    }
    true
}

/// The engine-visible command function of both markers, run by the engine's RHI command iterator.
/// Nothing in it panics or unwinds: consumer code is reached through the exception leaves.
///
/// # Safety
/// `command` is the begin or end marker of a live ticket whose owner has not been destroyed.
unsafe extern "C" fn execute(command_list: *mut c_void, command: *mut Command) {
    let (ticket, begin) = unsafe { ((*command).ticket, (*command).begin) };
    let owner_ptr = unsafe { (*ticket).owner };
    let owner = unsafe { &*owner_ptr };
    let scope = unsafe { addr_of_mut!((*ticket).scope) };
    let lease = unsafe { addr_of!((*ticket).lease) };
    // No enclosing scope is reported as a zeroed scope.
    let mut selected: rsf_ac7_render_scope = unsafe { zeroed() };
    let mut notify = false;
    // Output allocation may be published after begin was queued. End is appended only after
    // publication, so resolve again before notifying the matching pass consumer. A refused
    // resolve leaves no pass to deliver, but an end still retires its stack entry.
    let id = unsafe { (*scope).scope_id };
    let resolved = unsafe { (*lease).resolve.is_none() || (owner.calls.resolve)(lease, scope) };
    unsafe { (*scope).scope_id = id };
    if begin && resolved {
        let mut state = owner.lock();
        // The reserved capacity covers every outstanding ticket in a normal stream.
        if state.stack.try_reserve(1).is_ok() {
            let copy = unsafe { scope.read() };
            state.stack.push(copy);
            selected = copy;
            notify = true;
        }
    } else if !begin {
        let mut state = owner.lock();
        if let Some(index) = state.stack.iter().rposition(|item| item.scope_id == id) {
            // An out-of-order close cannot complete a pass or restore graphics state. It
            // still retires the copied scope before dropping the lease that backed it.
            notify = index + 1 == state.stack.len();
            state.stack.remove(index);
            if notify && let Some(top) = state.stack.last() {
                selected = *top;
            }
        }
    }
    let phase = u32::from(begin);
    if notify {
        // A consumer refusal must not suppress the scope-state restoration, and neither callback
        // can interrupt native work or ticket retirement.
        if resolved && owner.pass_callback.is_some() {
            unsafe {
                (owner.calls.notify)(owner.pass_callback, owner.user, command_list, scope, phase)
            };
        }
        if owner.callback.is_some() {
            unsafe {
                (owner.calls.notify)(owner.callback, owner.user, command_list, &selected, phase)
            };
        }
    }
    if begin {
        return;
    }
    if unsafe { (*lease).release.is_some() } {
        unsafe { (owner.calls.release)(lease) };
    }
    // The ticket is freed before the owner may report idle: after the decrement only the unlock
    // and return still run in this module.
    unsafe { drop(Box::from_raw(ticket)) };
    let mut state = unsafe { (*owner_ptr).lock() };
    state.outstanding = state.outstanding.wrapping_sub(1);
}

/// rsf_ac7_render_scopes_create_passes with the exception leaves of render_scope_entry.c.
///
/// # Safety
/// `out` is null or writable; `calls` is null or lives as long as the owner.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_scope_owner_create(
    session_id: u64,
    capacity: u32,
    state_callback: rsf_ac7_render_scope_fn,
    pass_callback: rsf_ac7_render_scope_fn,
    user: *mut c_void,
    calls: *const rsf_ac7_scope_calls,
    out: *mut *mut Scopes,
) -> c_int {
    if out.is_null() || session_id == 0 || capacity == 0 || capacity > CAPACITY_LIMIT {
        return 0;
    }
    unsafe { out.write(null_mut()) };
    let Some(calls) = unsafe { calls.as_ref() }.and_then(Calls::new) else {
        return 0;
    };
    let mut stack = Vec::new();
    if stack.try_reserve_exact(capacity as usize).is_err() {
        return 0;
    }
    let Some(owner) = try_box(Scopes {
        session: session_id,
        capacity,
        callback: state_callback,
        pass_callback,
        user,
        calls,
        state: Mutex::new(State {
            serial: 0,
            outstanding: 0,
            accepting: true,
            stack,
        }),
    }) else {
        return 0;
    };
    unsafe { out.write(Box::into_raw(owner)) };
    1
}

/// `open_leased` without a lease. `out` receives the ticket, or null on refusal.
///
/// # Safety
/// As `open_leased`; `out` is null or writable.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_render_scope_open(
    scopes: *mut Scopes,
    native_command_list: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    out: *mut *mut Ticket,
) -> c_int {
    if out.is_null() {
        return 0;
    }
    let ticket = unsafe { open_leased(scopes, native_command_list, scope, null()) };
    unsafe { out.write(ticket.unwrap_or(null_mut())) };
    c_int::from(ticket.is_some())
}

/// Copies `scope` with the next serial, takes the lease and appends the begin marker, returning
/// the open ticket. Takes lease ownership on success only.
///
/// # Safety
/// `scopes` is null or a live owner; `native_command_list` is null or a recording native list
/// owned by the calling thread; `scope` is null or a caller scope of at least its struct_size;
/// `lease` is null or valid.
pub(crate) unsafe fn open_leased(
    scopes: *mut Scopes,
    native_command_list: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    lease: *const rsf_ac7_scope_lease,
) -> Option<*mut Ticket> {
    unsafe { queue_scope(scopes, native_command_list, scope, lease, false) }
}

/// Takes the lease only when both adjacent markers can be queued together.
///
/// # Safety
/// As open_leased, without a ticket output.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_render_scope_enqueue_leased(
    scopes: *mut Scopes,
    native_command_list: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    lease: *const rsf_ac7_scope_lease,
) -> c_int {
    c_int::from(unsafe { queue_scope(scopes, native_command_list, scope, lease, true) }.is_some())
}

/// `paired` queues both markers, and the returned ticket only reports success. Otherwise it is
/// the caller's open ticket.
unsafe fn queue_scope(
    scopes: *mut Scopes,
    native_command_list: *mut c_void,
    scope: *const rsf_ac7_render_scope,
    lease: *const rsf_ac7_scope_lease,
    paired: bool,
) -> Option<*mut Ticket> {
    if scopes.is_null()
        || native_command_list.is_null()
        || scope.is_null()
        || unsafe { (*scope).struct_size } < size_of::<rsf_ac7_render_scope>() as u32
    {
        return None;
    }
    let owner = unsafe { &*scopes };
    let scope = unsafe { &*scope };
    if scope.session_id != owner.session || !valid_identity(scope) {
        return None;
    }
    let mut state = owner.lock();
    if !state.accepting || state.outstanding >= owner.capacity {
        return None;
    }
    let ticket = try_box(Ticket {
        owner: scopes,
        recording_list: native_command_list,
        scope: *scope,
        lease: unsafe { lease.as_ref() }.copied().unwrap_or(NO_LEASE),
        begin: Command::new(true),
        end: Command::new(false),
        closed: paired,
    })?;
    let ticket = Box::into_raw(ticket);
    let first = unsafe { addr_of_mut!((*ticket).begin) };
    let end = unsafe { addr_of_mut!((*ticket).end) };
    let last = if paired { end } else { first };
    state.serial = state.serial.wrapping_add(1);
    unsafe {
        (*ticket).scope.scope_id = state.serial;
        (*first).ticket = ticket;
        (*end).ticket = ticket;
        if paired {
            (*first).next = end;
        }
    }
    if !unsafe { append(native_command_list, first, last) } {
        unsafe { drop(Box::from_raw(ticket)) };
        return None;
    }
    state.outstanding += 1;
    Some(ticket)
}

/// Appends the end marker. A refused append leaves the ticket open for another attempt.
///
/// # Safety
/// `ticket` is null or an open ticket whose end has not executed; `native_command_list` is null or
/// a recording native list owned by the calling thread.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_render_scope_close(
    ticket: *mut Ticket,
    native_command_list: *mut c_void,
) -> c_int {
    if ticket.is_null()
        || unsafe { (*ticket).closed || (*ticket).recording_list != native_command_list }
    {
        return 0;
    }
    unsafe { (*ticket).closed = true };
    let end = unsafe { addr_of_mut!((*ticket).end) };
    if !unsafe { append(native_command_list, end, end) } {
        unsafe { (*ticket).closed = false };
        return 0;
    }
    1
}

/// The innermost executed scope. Zero when no identified engine scope is open.
///
/// # Safety
/// `scopes` is null or a live owner; `out` is null or a caller scope of at least its struct_size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_render_scope_current(
    scopes: *mut Scopes,
    out: *mut rsf_ac7_render_scope,
) -> c_int {
    if scopes.is_null()
        || out.is_null()
        || unsafe { (*out).struct_size } < size_of::<rsf_ac7_render_scope>() as u32
    {
        return 0;
    }
    let state = unsafe { &*scopes }.lock();
    match state.stack.last() {
        Some(top) => {
            unsafe { out.write(*top) };
            1
        }
        None => 0,
    }
}

/// Refuses further opens. Queued markers still execute and close.
///
/// # Safety
/// `scopes` is null or a live owner.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_render_scopes_quiesce(scopes: *mut Scopes) {
    if let Some(owner) = unsafe { scopes.as_ref() } {
        owner.lock().accepting = false;
    }
}

fn idle(state: &State) -> bool {
    !state.accepting && state.outstanding == 0 && state.stack.is_empty()
}

/// Quiesced with no ticket outstanding and no scope open; a null owner is idle.
///
/// # Safety
/// `scopes` is null or a live owner.
pub(crate) unsafe fn scopes_idle(scopes: *mut Scopes) -> bool {
    unsafe { scopes.as_ref() }.is_none_or(|owner| idle(&owner.lock()))
}

/// Frees an idle owner. Refuses while producers are accepted or markers or scopes remain.
///
/// # Safety
/// `scopes` is null or a live owner that no other thread uses once this succeeds.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_ac7_render_scopes_destroy(scopes: *mut Scopes) -> c_int {
    let Some(owner) = (unsafe { scopes.as_ref() }) else {
        return 1;
    };
    if !idle(&owner.lock()) {
        return 0;
    }
    unsafe { drop(Box::from_raw(scopes)) };
    1
}

#[cfg(test)]
mod tests;
