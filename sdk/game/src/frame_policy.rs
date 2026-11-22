// SPDX-License-Identifier: MIT
use crate::{
    RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_NO_SR, RSF_FRAME_SOURCE_RENDERED, RSF_SCREEN_LOADING,
    RSF_SCREEN_MENU, RSF_SCREEN_UNKNOWN, RSF_SCREEN_VIDEO, rsf_frame_record,
};
use core::ffi::c_int;

// Callers may pass just the size word of a short record.
unsafe fn complete(record: *const rsf_frame_record) -> bool {
    !record.is_null()
        && unsafe { record.cast::<u32>().read() } as usize >= size_of::<rsf_frame_record>()
}

fn allows_sr(record: &rsf_frame_record) -> bool {
    record.flags & RSF_FRAME_FLAG_NO_SR == 0
        && !matches!(record.screen, RSF_SCREEN_VIDEO | RSF_SCREEN_LOADING)
}

/// # Safety
/// A non-null pointer must have an aligned, readable size word and, if it advertises a
/// complete record, point to a readable, aligned frame record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_frame_has_rendered_source(record: *const rsf_frame_record) -> c_int {
    if !unsafe { complete(record) } {
        return 0;
    }
    let record = unsafe { &*record };
    c_int::from(
        record.source_provenance == RSF_FRAME_SOURCE_RENDERED
            && record.input_qpc == 0
            && record.source_qpc != 0
            && record.source_qpc_frequency != 0,
    )
}

/// # Safety
/// See [`rsf_frame_has_rendered_source`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_frame_allows_sr(record: *const rsf_frame_record) -> c_int {
    c_int::from(unsafe { complete(record) } && allows_sr(unsafe { &*record }))
}

/// # Safety
/// See [`rsf_frame_has_rendered_source`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_frame_allows_fg(record: *const rsf_frame_record) -> c_int {
    if !unsafe { complete(record) } {
        return 0;
    }
    let record = unsafe { &*record };
    // Menus hold still and then jump. Unknown screens have no verified interpolation policy.
    c_int::from(
        allows_sr(record)
            && record.flags & RSF_FRAME_FLAG_NO_FG == 0
            && !matches!(record.screen, RSF_SCREEN_MENU | RSF_SCREEN_UNKNOWN),
    )
}
