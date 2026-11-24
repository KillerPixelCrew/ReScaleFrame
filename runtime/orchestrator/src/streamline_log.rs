// SPDX-License-Identifier: MIT
use crate::diagnostic_output::Output;
use crate::sync::lock;
use crate::sys::rsf_backend_log_fn as Log;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy)]
struct Sink {
    owner: usize,
    log: Log,
    user: usize,
    verbose: bool,
}

// One sink per Streamline owner: legacy D3D11 SR and the D3D12 host.
static SINKS: Mutex<[Option<Sink>; 2]> = Mutex::new([None; 2]);

fn sinks() -> MutexGuard<'static, [Option<Sink>; 2]> {
    lock(&SINKS)
}

pub(crate) fn set(slot: u32, owner: usize, log: Log, user: *mut c_void, verbose: bool) {
    let _output = Output::enter();
    if let Some(value) = sinks().get_mut(slot as usize) {
        *value = log.map(|_| Sink {
            owner,
            log,
            user: user as usize,
            verbose,
        });
    }
}

pub(crate) fn clear(slot: u32, owner: usize) {
    // Stop invoking the sink before returning its context to the caller.
    let _output = Output::enter();
    if let Some(value) = sinks().get_mut(slot as usize)
        && value.is_some_and(|sink| sink.owner == owner)
    {
        *value = None;
    }
}

/// # Safety
/// `text` is a NUL-terminated string valid for the call.
pub(crate) unsafe fn emit(slot: u32, important: bool, text: *const c_char) {
    // SDK worker diagnostics must not wait for an application callback.
    let Some(_output) = Output::try_enter() else {
        return;
    };
    // Copy before invoking the sink. The output guard keeps its context alive.
    let sink = sinks().get(slot as usize).copied().flatten();
    if let Some(sink) = sink.filter(|sink| important || sink.verbose)
        && let Some(log) = sink.log
    {
        unsafe { log(sink.user as *mut c_void, text) };
    }
}
