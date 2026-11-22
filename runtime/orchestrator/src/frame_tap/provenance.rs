// SPDX-License-Identifier: MIT
use super::*;
use crate::constant_buffer_read::USAGE_DYNAMIC;
use crate::d3d11_output_abi::RSF_D3D11_USAGE_DEFAULT;
use std::thread::{self, ThreadId};

struct Tracked {
    context: Handle,
    context1: Option<ComRef>,
    buffer: ComRef,
    pixel_slot: u32,
    width: u32,
    usage: u32,
    owner: ThreadId,
    callback: rsf_frame_tap_constant_observer_fn,
    user: Handle,
}

struct State {
    watch: Option<Tracked>,
    serial: u64,
    committed: u64,
    mapped: Handle,
}

// Only the registering render thread uses the callback, context or mapped
// address. Retained COM refs may be released after the hook drain.
unsafe impl Send for State {}

static ARMED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State {
    watch: None,
    serial: 0,
    committed: 0,
    mapped: null_mut(),
});

thread_local! {
    static UPLOAD: Cell<u64> = const { Cell::new(0) };
    static DRAW: Cell<u64> = const { Cell::new(0) };
}

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    fn invalidate(&mut self) {
        self.serial = self.serial.saturating_add(1);
        self.committed = 0;
        self.mapped = null_mut();
    }

    fn tracks(&self, resource: Handle) -> bool {
        self.watch
            .as_ref()
            .is_some_and(|watch| watch.buffer.as_ptr() == resource)
    }

    fn owner(&self, context: Handle) -> bool {
        self.watch
            .as_ref()
            .is_some_and(|watch| watch.context == context && watch.owner == thread::current().id())
    }
}

fn replace(watch: Option<Tracked>) {
    let mut state = state();
    state.invalidate();
    ARMED.store(watch.is_some(), Release);
    let previous = std::mem::replace(&mut state.watch, watch);
    drop(state);
    drop(previous);
}

pub(super) fn retire() {
    if ARMED.load(Acquire) {
        replace(None);
    }
}

pub(super) fn invalidate() {
    if ARMED.load(Acquire) {
        state().invalidate();
    }
}

pub(super) fn invalidate_context(context: Handle) {
    if observed(context) {
        invalidate();
    }
}

pub(super) fn admit(context: Handle) -> Option<Admission> {
    if ARMED.load(Acquire) {
        Admission::enter(context)
    } else {
        None
    }
}

pub(super) fn destination(context: Handle, resource: Handle) -> u64 {
    if !ARMED.load(Acquire) {
        return 0;
    }
    let mut state = state();
    if !state.tracks(resource) {
        return 0;
    }
    state.invalidate();
    if state.owner(context) && state.serial != u64::MAX {
        state.serial
    } else {
        0
    }
}

pub(super) fn mapped(serial: u64, data: Handle) {
    if serial == 0 || data.is_null() {
        return;
    }
    let mut state = state();
    if serial == state.serial
        && state
            .watch
            .as_ref()
            .is_some_and(|watch| watch.usage == USAGE_DYNAMIC)
    {
        state.mapped = data;
    }
}

pub(super) fn unmapping(context: Handle, resource: Handle, subresource: u32) -> (u64, Handle) {
    if !ARMED.load(Acquire) {
        return (0, null_mut());
    }
    let mut state = state();
    if !state.tracks(resource) {
        return (0, null_mut());
    }
    if !state.owner(context) || subresource != 0 || state.mapped.is_null() {
        state.invalidate();
        return (0, null_mut());
    }
    (
        state.serial,
        std::mem::replace(&mut state.mapped, null_mut()),
    )
}

fn in_scope<R>(cell: &Cell<u64>, serial: u64, call: impl FnOnce() -> R) -> R {
    struct Restore<'a>(&'a Cell<u64>, u64);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            self.0.set(self.1);
        }
    }
    let _restore = Restore(cell, cell.replace(serial));
    call()
}

// Dedicated read-only observer, called after any existing watch modified the
// forwarded bytes and before their callback lease ends.
pub(super) unsafe fn upload(
    serial: u64,
    resource: Handle,
    data: *const c_void,
    width: Option<u32>,
) -> bool {
    if serial == 0 || data.is_null() || !HOOK_COVERAGE.load(Acquire) {
        return false;
    }
    let (callback, user, bytes) = {
        let state = state();
        if serial != state.serial || !state.tracks(resource) {
            return false;
        }
        let watch = state.watch.as_ref().unwrap();
        if width.is_some_and(|width| width != watch.width || watch.usage != RSF_D3D11_USAGE_DEFAULT)
        {
            return false;
        }
        (watch.callback, watch.user, watch.width)
    };
    let _guard = Reentry::enter();
    UPLOAD.with(|cell| {
        in_scope(cell, serial, || unsafe {
            rsf_frame_tap_call_constant_observer(callback, user, resource, data, bytes);
        })
    });
    true
}

pub(super) fn commit(serial: u64) {
    if serial != 0 && HOOK_COVERAGE.load(Acquire) {
        let mut state = state();
        if serial == state.serial {
            state.committed = serial;
        }
    }
}

// Sample the real pixel slot just before forwarding, after the research
// pre-draw callback has finished. The callback record still has its old layout.
pub(super) fn draw(context: Handle) -> u64 {
    if !ARMED.load(Acquire) {
        return 0;
    }
    let (serial, buffer, context1, index, width) = {
        let state = state();
        if state.committed == 0 || !state.owner(context) {
            return 0;
        }
        let watch = state.watch.as_ref().unwrap();
        (
            state.committed,
            watch.buffer.as_ptr(),
            watch.context1.as_ref().map(ComRef::as_ptr),
            watch.pixel_slot,
            watch.width,
        )
    };
    let mut bound = null_mut();
    let mut first = u32::MAX;
    let mut count = 0;
    unsafe {
        if let Some(context1) = context1 {
            // ID3D11DeviceContext1::PSGetConstantBuffers1, SDK 10.0.26100.0.
            type Get = unsafe extern "system" fn(Handle, u32, u32, *mut Handle, *mut u32, *mut u32);
            let get: Get = std::mem::transmute(slot(context1, 129));
            get(context1, index, 1, &mut bound, &mut first, &mut count);
        } else {
            type Get = unsafe extern "system" fn(Handle, u32, u32, *mut Handle);
            let get: Get = std::mem::transmute(slot(context, 77));
            get(context, index, 1, &mut bound);
            first = 0;
            count = width / 16;
        }
        drop(ComRef::from_owned(bound));
    }
    if bound == buffer && first == 0 && count >= width / 16 {
        serial
    } else {
        0
    }
}

pub(super) fn report(serial: u64, call: impl FnOnce()) {
    if serial == 0 {
        call();
    } else {
        DRAW.with(|cell| in_scope(cell, serial, call));
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_tap_track_pixel_constant(
    context: Handle,
    index: u32,
    buffer: Handle,
    callback: rsf_frame_tap_constant_observer_fn,
    user: Handle,
) -> i32 {
    let Some(_admission) = Admission::enter(context) else {
        return NOT_INSTALLED;
    };
    if state()
        .watch
        .as_ref()
        .is_some_and(|watch| watch.owner != thread::current().id())
    {
        return INVALID;
    }
    if buffer.is_null() && callback.is_none() {
        replace(None);
        return OK;
    }
    if index >= STAGE_CONSTANTS as u32
        || buffer.is_null()
        || callback.is_none()
        || unsafe { crate::d3d11_output_abi::rsf_d3d11_output_is_immediate(context) } == 0
    {
        return INVALID;
    }
    let Some((width, flags, usage)) =
        (unsafe { crate::constant_buffer_read::resource_buffer_info(buffer) })
    else {
        return INVALID;
    };
    if flags != BIND_CONSTANT_BUFFER
        || width == 0
        || width > 4096
        || !width.is_multiple_of(16)
        || !matches!(usage, RSF_D3D11_USAGE_DEFAULT | USAGE_DYNAMIC)
    {
        return INVALID;
    }
    let device = unsafe {
        ComRef::from_owned(crate::d3d11_output_abi::rsf_d3d11_output_child_device(
            context,
        ))
    };
    if !device
        .is_some_and(|device| unsafe { crate::d3d11_output::same_device(device.as_ptr(), buffer) })
    {
        return INVALID;
    }
    let context1 = unsafe { super::platform::context1_ref(context) };
    // The install only patches the observed interface table. A separate
    // Context1 table could write independently of the base interface detours.
    if context1
        .as_ref()
        .is_some_and(|extended| extended.as_ptr() != context)
    {
        return INVALID;
    }
    replace(Some(Tracked {
        context,
        context1,
        buffer: unsafe { ComRef::retain(buffer) },
        pixel_slot: index,
        width,
        usage,
        owner: thread::current().id(),
        callback,
        user,
    }));
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_constant_generation(
    context: Handle,
    buffer: Handle,
    phase: u32,
) -> u64 {
    if !TAP.installed.load(Acquire) || !ARMED.load(Acquire) {
        return 0;
    }
    if phase == RSF_FRAME_TAP_CONSTANT_CURRENT {
        refresh(super::platform::platform());
    }
    if !HOOK_COVERAGE.load(Acquire) {
        return 0;
    }
    let state = state();
    if !state.owner(context) || !state.tracks(buffer) {
        return 0;
    }
    match phase {
        RSF_FRAME_TAP_CONSTANT_UPLOAD => matching(UPLOAD.with(Cell::get), state.serial),
        RSF_FRAME_TAP_CONSTANT_DRAW => matching(DRAW.with(Cell::get), state.committed),
        RSF_FRAME_TAP_CONSTANT_CURRENT => state.committed,
        _ => 0,
    }
}

fn matching(candidate: u64, current: u64) -> u64 {
    if candidate == current { candidate } else { 0 }
}
