// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_SR_DLSS, RSF_SR_FSR4, RSF_SRVK_ABI_VERSION, rsf_fsr4vk_handoff as Handoff,
    rsf_srvk_fallback as Fallback, rsf_srvk_frame as Frame, rsf_srvk_setup as Setup,
};
use std::ffi::{c_char, c_void};
use std::ptr;

type Handle = *mut c_void;
use rsf_game_sys::backend_result::{ABI_MISMATCH, INIT_FAILED, INVALID_ARGUMENT as INVALID, NOT_READY};

mod routes;
use routes::{Route, Routes};

struct Session {
    route: &'static Route,
    owner: Handle,
    ready: bool,
    split: bool,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_session_open(
    routes: &'static Routes,
    setup: *const Setup,
    handoff: *const Handoff,
    split_directory: *const c_char,
    preset: u32,
    out: *mut *mut Session,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return INVALID;
    };
    *out = ptr::null_mut();
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let mut setup = unsafe { *setup };
    if setup.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let split = setup.backend == RSF_SR_FSR4;
    let route = if split {
        if handoff.is_null() {
            return NOT_READY;
        }
        setup.runtime_directory_utf8 = split_directory;
        &routes.split
    } else {
        &routes.native
    };
    let raw = allocate_handle(Session {
        route,
        owner: ptr::null_mut(),
        ready: false,
        split,
    });
    let Some(session) = (unsafe { raw.as_mut() }) else {
        return INIT_FAILED;
    };
    let result = unsafe { (route.open)(&setup, handoff, &mut session.owner) };
    if session.owner.is_null() {
        drop(unsafe { Box::from_raw(raw) });
        return if result == 0 { NOT_READY } else { result };
    }
    session.ready = result == 0;
    if session.ready && setup.backend == RSF_SR_DLSS {
        unsafe { (routes.preset)(session.owner, preset) };
    }
    *out = raw;
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_session_plan(
    session: *const Session,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    match unsafe { session.as_ref() }.filter(|session| session.ready) {
        Some(session) => unsafe { (session.route.plan)(session.owner, width, height) },
        None => NOT_READY,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_session_record(
    routes: &'static Routes,
    session: *mut Session,
    frame: *const Frame,
    fallback: *const Fallback,
    split_recording: u32,
    provider_result: *mut i32,
    continuation: *mut Handle,
) -> i32 {
    let (Some(provider), Some(next)) = (unsafe { provider_result.as_mut() }, unsafe {
        continuation.as_mut()
    }) else {
        return INVALID;
    };
    *next = ptr::null_mut();
    if !unsafe { has_capacity(fallback) } {
        return INVALID;
    }
    let mut fallback = unsafe { *fallback };
    if fallback.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    *next = fallback.command_buffer;
    if let Some(session) = unsafe { session.as_mut() } {
        *provider = if session.ready && (!session.split || split_recording != 0) {
            unsafe { (session.route.record)(session.owner, *next, frame, next) }
        } else {
            NOT_READY
        };
        if *provider == 0 && !(*next).is_null() {
            return 0;
        }
    }
    if (*next).is_null() {
        return NOT_READY;
    }
    fallback.command_buffer = *next;
    unsafe { (routes.fallback)(&fallback) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_session_retire(session: *mut Session, commands: Handle) {
    if let Some(session) = unsafe { session.as_mut() }.filter(|session| session.ready) {
        unsafe { (session.route.retire)(session.owner, commands) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_session_close(raw: *mut Session) -> i32 {
    let Some(session) = (unsafe { raw.as_mut() }) else {
        return 0;
    };
    let result = unsafe { (session.route.close)(session.owner) };
    if result == 0 {
        drop(unsafe { Box::from_raw(raw) });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMANDS: usize = 0x1234;
    const CONTINUATION: usize = 0x5678;
    const REFUSED: i32 = -9;

    unsafe extern "C" fn open(_: &Setup, _: *const Handoff, out: &mut Handle) -> i32 {
        *out = ptr::without_provenance_mut(1);
        0
    }

    unsafe extern "C" fn record(_: Handle, _: Handle, _: *const Frame, next: &mut Handle) -> i32 {
        *next = ptr::without_provenance_mut(CONTINUATION);
        REFUSED
    }

    unsafe extern "C" fn terminal(_: Handle, _: Handle, _: *const Frame, next: &mut Handle) -> i32 {
        *next = ptr::null_mut();
        REFUSED
    }

    unsafe extern "C" fn close(_: Handle) -> i32 {
        0
    }
    unsafe extern "C" fn plan(_: Handle, _: *mut u32, _: *mut u32) -> i32 {
        0
    }
    unsafe extern "C" fn retire(_: Handle, _: Handle) {}
    unsafe extern "C" fn preset(_: Handle, _: u32) {}
    unsafe extern "C" fn fallback(input: &Fallback) -> i32 {
        if input.command_buffer.addr() == CONTINUATION {
            0
        } else {
            INVALID
        }
    }

    const fn route(
        record: unsafe extern "C" fn(Handle, Handle, *const Frame, &mut Handle) -> i32,
    ) -> Route {
        Route {
            open,
            plan,
            record,
            retire,
            close,
        }
    }

    static ROUTES: Routes = Routes {
        native: route(terminal),
        split: route(record),
        preset,
        fallback,
    };

    #[test]
    fn failed_split_recovers_on_the_continuation_only() {
        let setup = Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            backend: RSF_SR_FSR4,
            // SAFETY: the remaining setup fields are nullable pointers and scalars.
            ..unsafe { std::mem::zeroed() }
        };
        let fallback = Fallback {
            struct_size: size_of::<Fallback>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            command_buffer: ptr::without_provenance_mut(COMMANDS),
            // SAFETY: the remaining fields are nullable pointers and scalars.
            ..unsafe { std::mem::zeroed() }
        };
        // SAFETY: the fake route ignores handoff and frame contents.
        let handoff: Handoff = unsafe { std::mem::zeroed() };
        let mut owner = ptr::null_mut();
        let mut provider = 0;
        let mut next = ptr::null_mut();
        unsafe {
            assert_eq!(
                rsf_core_srvk_session_open(&ROUTES, &setup, &handoff, ptr::null(), 0, &mut owner),
                0
            );
            assert_eq!(
                rsf_core_srvk_session_record(
                    &ROUTES,
                    owner,
                    ptr::null(),
                    &fallback,
                    1,
                    &mut provider,
                    &mut next
                ),
                0
            );
            assert_eq!((provider, next.addr()), (REFUSED, CONTINUATION));
            assert_eq!(rsf_core_srvk_session_close(owner), 0);

            let setup = Setup {
                backend: RSF_SR_DLSS,
                ..setup
            };
            assert_eq!(
                rsf_core_srvk_session_open(
                    &ROUTES,
                    &setup,
                    ptr::null(),
                    ptr::null(),
                    0,
                    &mut owner
                ),
                0
            );
            assert_eq!(
                rsf_core_srvk_session_record(
                    &ROUTES,
                    owner,
                    ptr::null(),
                    &fallback,
                    1,
                    &mut provider,
                    &mut next
                ),
                NOT_READY
            );
            assert_eq!((provider, next.addr()), (REFUSED, 0));
            assert_eq!(rsf_core_srvk_session_close(owner), 0);
        }
    }
}
