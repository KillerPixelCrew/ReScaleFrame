// SPDX-License-Identifier: MIT
use rsf_game_sys::{rsf_frame_record, rsf_game_log_fn};
use std::ffi::{c_char, c_void};

type Configure =
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, rsf_game_log_fn, *mut c_void);
type Request = unsafe extern "C" fn();
type Requested = unsafe extern "C" fn() -> i32;
type Begin = unsafe extern "C" fn(&rsf_frame_record, u32) -> i32;
type Texture = unsafe extern "C" fn(
    *mut c_void,
    &rsf_frame_record,
    *const c_char,
    *const c_char,
    *mut c_void,
    u32,
    f32,
) -> i32;
type Complete = unsafe extern "C" fn(&rsf_frame_record, i32);
type Cancel = unsafe extern "C" fn(u64);
type ReportResult = unsafe extern "C" fn(&rsf_frame_record, *const c_char, u32, i32);

#[derive(Clone, Copy)]
pub(crate) struct DumpApi {
    pub configure: Configure,
    pub request: Request,
    pub poll: Request,
    pub requested: Requested,
    pub begin_record: Begin,
    pub texture_record: Texture,
    pub complete: Complete,
    pub cancel: Cancel,
    pub result: Option<ReportResult>,
}

impl DumpApi {
    #[cfg(windows)]
    pub fn bind() -> Option<Self> {
        use crate::runtime_module::{export, rsf_anomaly_runtime_module};
        let module = rsf_anomaly_runtime_module();
        macro_rules! export {
            ($name:expr, $signature:ty) => {
                unsafe { export::<$signature>(module, $name.to_bytes_with_nul()) }
            };
        }
        // The carrier keeps the loaded Runtime alive. Signatures come from input_dump.h.
        Some(Self {
            configure: export!(c"rsf_input_dump_configure", Configure)?,
            request: export!(c"rsf_input_dump_request", Request)?,
            poll: export!(c"rsf_input_dump_poll", Request)?,
            requested: export!(c"rsf_input_dump_requested", Requested)?,
            begin_record: export!(c"rsf_input_dump_begin_record", Begin)?,
            texture_record: export!(c"rsf_input_dump_texture_record", Texture)?,
            complete: export!(c"rsf_input_dump_complete", Complete)?,
            cancel: export!(c"rsf_input_dump_cancel", Cancel)?,
            result: export!(c"rsf_input_dump_result", ReportResult),
        })
    }

    #[cfg(not(windows))]
    pub fn bind() -> Option<Self> {
        None
    }
}
