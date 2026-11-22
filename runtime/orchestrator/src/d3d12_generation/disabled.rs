// SPDX-License-Identifier: MIT
use super::*;
use crate::render_abi::{RSF_BACKEND_ABI_VERSION, rsf_fg_swapchain_desc};

const NOT_COMPILED: i32 = -3;

unsafe extern "C" fn create(setup: *const Setup, out: *mut Raw, chain: *mut Raw) -> i32 {
    for output in [out, chain] {
        if !output.is_null() {
            unsafe { output.write(null_mut()) };
        }
    }
    if out.is_null() || chain.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if (setup.chain.struct_size as usize) < size_of::<rsf_fg_swapchain_desc>() {
        return INVALID;
    }
    if setup.abi_version != RSF_FG_ABI_VERSION || setup.chain.abi_version != RSF_BACKEND_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    NOT_COMPILED
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
