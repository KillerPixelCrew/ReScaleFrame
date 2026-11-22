// SPDX-License-Identifier: MIT
use super::{Fallback, Frame, Handle, Handoff, Setup};

#[repr(C)]
pub(crate) struct Route {
    pub(crate) open: unsafe extern "C" fn(&Setup, *const Handoff, &mut Handle) -> i32,
    pub(crate) plan: unsafe extern "C" fn(Handle, *mut u32, *mut u32) -> i32,
    pub(crate) record: unsafe extern "C" fn(Handle, Handle, *const Frame, &mut Handle) -> i32,
    pub(crate) retire: unsafe extern "C" fn(Handle, Handle),
    pub(crate) close: unsafe extern "C" fn(Handle) -> i32,
}

#[repr(C)]
pub(crate) struct Routes {
    pub(crate) native: Route,
    pub(crate) split: Route,
    pub(crate) preset: unsafe extern "C" fn(Handle, u32),
    pub(crate) fallback: unsafe extern "C" fn(&Fallback) -> i32,
}
