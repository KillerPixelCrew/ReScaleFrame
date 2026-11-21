// SPDX-License-Identifier: MIT
use crate::diagnostic_output::Output;
use crate::render_abi::rsf_backend_log_fn as Log;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError};

#[derive(Clone, Copy)]
struct Sink {
    owner: usize,
    log: Log,
    user: usize,
    verbose: bool,
}

// Legacy SR and the D3D12 host use different typed Streamline callbacks.
static SINKS: Mutex<[Option<Sink>; 2]> = Mutex::new([None; 2]);

fn sinks() -> MutexGuard<'static, [Option<Sink>; 2]> {
    SINKS.lock().unwrap_or_else(PoisonError::into_inner)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_streamline_log_set(
    slot: u32,
    owner: usize,
    log: Log,
    user: *mut c_void,
    verbose: u32,
) {
    let _output = Output::enter();
    if let Some(value) = sinks().get_mut(slot as usize) {
        *value = log.map(|_| Sink {
            owner,
            log,
            user: user as usize,
            verbose: verbose != 0,
        });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_streamline_log_clear(slot: u32, owner: usize) {
    // Stop invoking the sink before returning its context to the caller.
    let _output = Output::enter();
    if let Some(value) = sinks().get_mut(slot as usize)
        && value.is_some_and(|sink| sink.owner == owner)
    {
        *value = None;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_streamline_log_emit(
    slot: u32,
    important: u32,
    text: *const c_char,
    emit: unsafe extern "C" fn(Log, *mut c_void, *const c_char),
) {
    // SDK worker diagnostics must not wait for an application callback.
    let Some(_output) = Output::try_enter() else {
        return;
    };
    // Copy before invoking the sink. The output guard keeps its context alive.
    let sink = sinks().get(slot as usize).copied().flatten();
    if let Some(sink) = sink.filter(|sink| important != 0 || sink.verbose) {
        unsafe { emit(sink.log, sink.user as *mut c_void, text) };
    }
}
