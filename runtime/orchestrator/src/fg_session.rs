// SPDX-License-Identifier: MIT
use crate::fg_leases::{FenceState, Retirement, rsf_d3d12_fence_completed};
use crate::frame_sequencer::Ledger;
use crate::sr_session::has_capacity;
use crate::sys::*;
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NOT_READY, NOT_SUPPORTED,
};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_RESET, RSF_FRAME_SOURCE_INPUT,
    RSF_FRAME_SOURCE_RENDERED, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_allows_fg,
    rsf_frame_has_rendered_source,
};
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;

type FenceCompleted = unsafe extern "C" fn(*mut c_void) -> u64;

struct Session {
    host: Host,
    // The switching thread takes the state out while draining the host and
    // changing chains. A caller arriving during the transition gets NOT_READY.
    state: Mutex<Option<State>>,
}

struct State {
    active: Option<Context>,
    ledger: Ledger,
    prepared: u64,
    status: rsf_fg_session_status,
    options: rsf_fg_options,
}

struct Context {
    provider: Provider,
    handle: *mut c_void,
}

impl Context {
    fn create(
        provider: Provider,
        setup: *const rsf_generation_setup,
    ) -> Result<(Self, *mut c_void), i32> {
        let mut context = Self {
            provider,
            handle: ptr::null_mut(),
        };
        let mut chain = ptr::null_mut();
        let result = unsafe { (provider.create)(setup, &mut context.handle, &mut chain) };
        if result != 0 {
            return Err(result);
        }
        if context.handle.is_null() {
            return Err(NOT_READY);
        }
        Ok((context, chain))
    }

    fn status(&self) -> Result<rsf_fg_status, i32> {
        let mut status = rsf_fg_status {
            struct_size: size_of::<rsf_fg_status>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let result = unsafe { (self.provider.status)(self.handle, &mut status) };
        if result != 0 { Err(result) } else { Ok(status) }
    }

    fn retired(&self, completed: FenceCompleted) -> i32 {
        let mut retirement = rsf_fg_retirement {
            struct_size: size_of::<rsf_fg_retirement>() as u32,
            fence: ptr::null_mut(),
            value: 0,
        };
        let result = unsafe { (self.provider.retirement)(self.handle, &mut retirement) };
        if result != 0 {
            return result;
        }
        match unsafe { Retirement::read(&retirement) } {
            None => INVALID_ARGUMENT,
            Some(Retirement::Immediate) => 0,
            Some(Retirement::Fenced { fence, value }) => {
                match FenceState::check(unsafe { completed(fence.as_ptr()) }, value) {
                    FenceState::Pending => NOT_READY,
                    FenceState::Complete => 0,
                    FenceState::DeviceRemoved => FEATURE_FAILED,
                }
            }
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (self.provider.destroy)(self.handle) };
        }
    }
}

#[derive(Clone, Copy)]
struct Provider {
    create: unsafe extern "C" fn(
        arg1: *const rsf_generation_setup,
        session: *mut *mut ::core::ffi::c_void,
        present_chain: *mut *mut ::core::ffi::c_void,
    ) -> rsf_backend_result,
    configure: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        options: *const rsf_fg_options,
    ) -> rsf_backend_result,
    begin_frame: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        frame_id: u64,
    ) -> rsf_backend_result,
    marker: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        marker: rsf_latency_marker,
        frame_id: u64,
        controller_input: u32,
    ) -> rsf_backend_result,
    prepare: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        command_list: *mut ::core::ffi::c_void,
        frame: *const rsf_fg_frame,
    ) -> rsf_backend_result,
    after_present: unsafe extern "C" fn(session: *mut ::core::ffi::c_void) -> rsf_backend_result,
    status: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        status: *mut rsf_fg_status,
    ) -> rsf_backend_result,
    retirement: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        retirement: *mut rsf_fg_retirement,
    ) -> rsf_backend_result,
    destroy: unsafe extern "C" fn(session: *mut ::core::ffi::c_void),
    abort_frame: unsafe extern "C" fn(
        session: *mut ::core::ffi::c_void,
        frame_id: u64,
    ) -> rsf_backend_result,
}

impl Provider {
    fn checked(raw: &rsf_generation_provider) -> Option<Self> {
        Some(Self {
            create: raw.create?,
            configure: raw.configure?,
            begin_frame: raw.begin_frame?,
            marker: raw.marker?,
            prepare: raw.prepare?,
            after_present: raw.after_present?,
            status: raw.status?,
            retirement: raw.retirement?,
            destroy: raw.destroy?,
            abort_frame: raw.abort_frame?,
        })
    }
}

#[derive(Clone, Copy)]
struct Host {
    user: *mut c_void,
    quiesce: unsafe extern "C" fn(user: *mut ::core::ffi::c_void) -> rsf_backend_result,
    resume: unsafe extern "C" fn(user: *mut ::core::ffi::c_void),
    release: unsafe extern "C" fn(user: *mut ::core::ffi::c_void) -> rsf_backend_result,
    adopt: unsafe extern "C" fn(
        user: *mut ::core::ffi::c_void,
        chain: *mut ::core::ffi::c_void,
    ) -> rsf_backend_result,
    restore_plain: unsafe extern "C" fn(user: *mut ::core::ffi::c_void) -> rsf_backend_result,
}

impl Host {
    fn checked(raw: &rsf_fg_host) -> Option<Self> {
        Some(Self {
            user: raw.user,
            quiesce: raw.quiesce?,
            resume: raw.resume?,
            release: raw.release?,
            adopt: raw.adopt?,
            restore_plain: raw.restore_plain?,
        })
    }
}

struct Selection {
    provider: Provider,
    setup: *const rsf_generation_setup,
    options: *const rsf_fg_options,
}

impl Selection {
    fn probe(&self) -> i32 {
        // A capability query must never create a second physical chain.
        let mut setup = unsafe { self.setup.read() };
        setup.struct_size = size_of::<rsf_generation_setup>() as u32;
        setup.chain.hwnd = ptr::null_mut();
        let (probe, chain) = match Context::create(self.provider, &setup) {
            Ok(result) => result,
            Err(result) => return result,
        };
        let capability = match probe.status() {
            Ok(status) => status,
            Err(result) => return result,
        };
        if capability.supported == 0 || !chain.is_null() {
            return NOT_SUPPORTED;
        }
        let options = unsafe { &*self.options };
        if options.mode > RSF_FG_DYNAMIC
            || options.reflex_mode > RSF_REFLEX_BOOST
            || (options.mode == RSF_FG_FIXED && options.generated_frames == 0)
        {
            return INVALID_ARGUMENT;
        }
        if (options.mode == RSF_FG_DYNAMIC && capability.dynamic_supported == 0)
            || (options.mode == RSF_FG_AUTO && capability.auto_supported == 0)
            || (options.mode == RSF_FG_FIXED && capability.max_generated_frames == 0)
            || (options.reflex_mode != RSF_REFLEX_OFF
                && capability.pacing_owner != RSF_PACING_REFLEX)
            || (options.frame_limit_us != 0 && capability.pacing_owner == RSF_PACING_NONE)
        {
            return NOT_SUPPORTED;
        }
        0
    }

    fn adopt(&self, host: &Host) -> Result<Context, i32> {
        let (context, chain) = Context::create(self.provider, self.setup)?;
        if chain.is_null() {
            return Err(NOT_READY);
        }
        let result = unsafe { (self.provider.configure)(context.handle, self.options) };
        if result != 0 {
            return Err(result);
        }
        // The host only borrows the chain on success.
        let result = unsafe { (host.adopt)(host.user, chain) };
        if result != 0 {
            Err(result)
        } else {
            Ok(context)
        }
    }
}

impl Session {
    fn switch(&self, state: &mut State, next: Option<Selection>) -> i32 {
        if let Some(ref next) = next {
            let result = next.probe();
            if result != 0 {
                return result;
            }
        }
        if let Some(ref current) = state.active {
            let result = current.retired(rsf_d3d12_fence_completed);
            if result != 0 {
                return result;
            }
        }
        let result = unsafe { (self.host.release)(self.host.user) };
        if result != 0 {
            return result;
        }
        state.active = None;
        state.prepared = 0;
        state.status.has_provider = 0;
        state.status.provider = unsafe { std::mem::zeroed() };
        state.status.chain_generation = state.status.chain_generation.wrapping_add(1);
        state.status.plain_available = 0;

        let mut result = 0;
        if let Some(next) = next {
            match next.adopt(&self.host) {
                Ok(context) => {
                    state.active = Some(context);
                    state.status.has_provider = 1;
                    state.options = unsafe { *next.options };
                    return 0;
                }
                Err(code) => result = code,
            }
        }
        let fallback = unsafe { (self.host.restore_plain)(self.host.user) };
        state.status.plain_available = u32::from(fallback == 0);
        if fallback != 0 { fallback } else { result }
    }

    fn with_state(&self, call: impl FnOnce(&mut State) -> i32) -> i32 {
        match self.state.lock().unwrap().as_mut() {
            Some(state) => call(state),
            None => NOT_READY,
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_create(
    host: *const rsf_fg_host,
    out: *mut *mut Session,
) -> i32 {
    if out.is_null() || !unsafe { has_capacity(host) } {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    let host = unsafe { *host };
    if host.abi_version != RSF_FG_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Some(host) = Host::checked(&host) else {
        return INVALID_ARGUMENT;
    };
    let status = rsf_fg_session_status {
        struct_size: size_of::<rsf_fg_session_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let session = crate::allocate_handle(Session {
        host,
        state: Mutex::new(Some(State {
            active: None,
            ledger: Ledger::new(8),
            prepared: 0,
            status,
            options: unsafe { std::mem::zeroed() },
        })),
    });
    if session.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(session) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_select(
    raw: *const Session,
    provider: *const rsf_generation_provider,
    setup: *const rsf_generation_setup,
    options: *const rsf_fg_options,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let selection = if provider.is_null() {
        None
    } else {
        if !unsafe { has_capacity(provider) && has_capacity(setup) && has_capacity(options) } {
            return INVALID_ARGUMENT;
        }
        let Some(provider) = Provider::checked(unsafe { &*provider }) else {
            return INVALID_ARGUMENT;
        };
        if unsafe {
            (*setup).abi_version != RSF_FG_ABI_VERSION
                || (*options).abi_version != RSF_FG_ABI_VERSION
        } {
            return ABI_MISMATCH;
        }
        Some(Selection {
            provider,
            setup,
            options,
        })
    };
    let mut state = {
        let mut slot = session.state.lock().unwrap();
        let Some(state) = slot.as_ref() else {
            return NOT_READY;
        };
        if state.ledger.busy() {
            return NOT_READY;
        }
        slot.take().unwrap()
    };
    let mut result = unsafe { (session.host.quiesce)(session.host.user) };
    if result == 0 {
        result = session.switch(&mut state, selection);
        unsafe { (session.host.resume)(session.host.user) };
    }
    state.status.last_switch = result;
    *session.state.lock().unwrap() = Some(state);
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_begin(raw: *const Session, id: u64) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    session.with_state(|state| {
        let result = state.ledger.begin(id, None);
        if result != 0 {
            return result;
        }
        if let Some(active) = &state.active {
            let result = unsafe { (active.provider.begin_frame)(active.handle, id) };
            if result != 0 {
                state.ledger.finish(id);
                return result;
            }
        }
        0
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_begin_rendered(
    raw: *const Session,
    id: u64,
    source: u64,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    session.with_state(|state| {
        let Some(active) = &state.active else {
            return NOT_READY;
        };
        let status = match active.status() {
            Ok(status) => status,
            Err(result) => return result,
        };
        if status.pacing_owner != RSF_PACING_NONE
            || state.options.reflex_mode != RSF_REFLEX_OFF
            || state.options.frame_limit_us != 0
        {
            return NOT_SUPPORTED;
        }
        state.ledger.begin(id, Some(source))
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_marker(
    raw: *const Session,
    id: u64,
    marker: u32,
    timestamp: u64,
    controller: u32,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if controller > 1 {
        return INVALID_ARGUMENT;
    }
    session.with_state(|state| {
        let result = state.ledger.marker(id, marker, timestamp);
        if result != 0 {
            return result;
        }
        if let Some(active) = &state.active
            && state.ledger.source(id) == Some(RSF_FRAME_SOURCE_INPUT)
        {
            let result = unsafe { (active.provider.marker)(active.handle, marker, id, controller) };
            if result != 0 {
                state.ledger.invalidate(id);
            }
            return result;
        }
        0
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_prepare(
    raw: *const Session,
    command: *mut c_void,
    frame: *const rsf_fg_frame,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(frame) && has_capacity((*frame).record) } {
        return INVALID_ARGUMENT;
    }
    let record = unsafe { &*(*frame).record };
    session.with_state(|state| {
        if record.abi_version != RSF_GAME_FRAME_ABI_VERSION {
            return ABI_MISMATCH;
        }
        let Some((provenance, timestamp)) = state.ledger.prepare_source(record.frame_id) else {
            return NOT_READY;
        };
        if state.prepared != 0 {
            return NOT_READY;
        }
        if provenance != record.source_provenance {
            return INVALID_ARGUMENT;
        }
        let interpolate = if unsafe { rsf_frame_allows_fg(record) } == 0
            || record.flags & (RSF_FRAME_FLAG_RESET | RSF_FRAME_FLAG_AMBIGUOUS_ID) != 0
        {
            0
        } else {
            unsafe { (*frame).interpolate }
        };
        if provenance == RSF_FRAME_SOURCE_RENDERED {
            if unsafe { rsf_frame_has_rendered_source(record) } == 0
                || record.source_qpc != timestamp
            {
                return INVALID_ARGUMENT;
            }
        } else if interpolate != 0 && record.input_qpc != timestamp {
            return INVALID_ARGUMENT;
        }
        let mut result = 0;
        if let Some(active) = &state.active {
            let mut submitted = std::mem::MaybeUninit::<rsf_fg_frame>::uninit();
            let copy = submitted.as_mut_ptr();
            result = unsafe {
                copy.copy_from_nonoverlapping(frame, 1);
                (*copy).struct_size = size_of::<rsf_fg_frame>() as u32;
                (*copy).interpolate = interpolate;
                (active.provider.prepare)(active.handle, command, copy)
            };
        }
        if result == 0 {
            state.prepared = record.frame_id;
        }
        state.status.last_frame = result;
        result
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_after_present(raw: *const Session, id: u64) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if id == 0 {
        return INVALID_ARGUMENT;
    }
    session.with_state(|state| {
        if state.prepared != id || state.ledger.failed(id) {
            return NOT_READY;
        }
        let mut result = state.ledger.finish(id);
        state.prepared = 0;
        if let Some(active) = &state.active {
            let vendor = unsafe { (active.provider.after_present)(active.handle) };
            if result == 0 {
                result = vendor;
            }
        }
        state.status.last_frame = result;
        result
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_abort(raw: *const Session, id: u64) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if id == 0 {
        return INVALID_ARGUMENT;
    }
    session.with_state(|state| {
        if state.ledger.source(id).is_none() {
            return NOT_READY;
        }
        if let Some(active) = &state.active {
            let result = unsafe { (active.provider.abort_frame)(active.handle, id) };
            if result != 0 {
                return result;
            }
        }
        state.ledger.finish(id);
        if state.prepared == id {
            state.prepared = 0;
        }
        0
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_retirement(
    raw: *const Session,
    out: *mut rsf_fg_retirement,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    session.with_state(|state| {
        unsafe {
            (*out).fence = ptr::null_mut();
            (*out).value = 0;
        }
        match &state.active {
            Some(active) => unsafe { (active.provider.retirement)(active.handle, out) },
            None => 0,
        }
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_get_status(
    raw: *const Session,
    out: *mut rsf_fg_session_status,
) -> i32 {
    let Some(session) = (unsafe { raw.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    session.with_state(|state| {
        if let Some(active) = &state.active {
            state.status.provider = match active.status() {
                Ok(status) => status,
                Err(result) => return result,
            };
        }
        unsafe { out.write(state.status) };
        0
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_session_destroy(raw: *mut Session) -> i32 {
    let result = unsafe { rsf_fg_session_select(raw, ptr::null(), ptr::null(), ptr::null()) };
    if result == 0 {
        // The presentation owner has retired callers of the handle.
        drop(unsafe { Box::from_raw(raw) });
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fg_get_provider(backend: u32) -> *const rsf_generation_provider {
    crate::d3d11_present::provider_for(backend)
}
