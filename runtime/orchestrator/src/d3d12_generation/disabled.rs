// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::backend_result::NOT_COMPILED;

unsafe extern "C" fn create(setup: *const Setup, out: *mut Raw, chain: *mut Raw) -> i32 {
    let result = unsafe { admission::rsf_core_fg12_setup_header(setup, out, chain) };
    if result == OK { NOT_COMPILED } else { result }
}

extern "C" fn configure(_: Raw, _: *const Options) -> i32 {
    NOT_COMPILED
}
extern "C" fn frame(_: Raw, _: u64) -> i32 {
    NOT_COMPILED
}
extern "C" fn marker(_: Raw, _: u32, _: u64, _: u32) -> i32 {
    NOT_COMPILED
}
extern "C" fn prepare(_: Raw, _: Raw, _: *const Frame) -> i32 {
    NOT_COMPILED
}
extern "C" fn after(_: Raw) -> i32 {
    NOT_COMPILED
}
extern "C" fn status(_: Raw, _: *mut Status) -> i32 {
    NOT_COMPILED
}
extern "C" fn retirement(_: Raw, _: *mut Retirement) -> i32 {
    NOT_COMPILED
}
extern "C" fn destroy(_: Raw) {}

pub(super) static DISABLED_PROVIDER: Provider = Provider {
    struct_size: size_of::<Provider>() as u32,
    create: Some(create),
    configure: Some(configure),
    begin_frame: Some(frame),
    marker: Some(marker),
    prepare: Some(prepare),
    after_present: Some(after),
    status: Some(status),
    retirement: Some(retirement),
    destroy: Some(destroy),
    abort_frame: Some(frame),
    destroy_checked: None,
};
