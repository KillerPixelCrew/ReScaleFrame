// SPDX-License-Identifier: MIT
use crate::ffi_util::checked;
use crate::{
    RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_NO_SR, RSF_FRAME_SOURCE_RENDERED, RSF_SCREEN_LOADING,
    RSF_SCREEN_MENU, RSF_SCREEN_UNKNOWN, RSF_SCREEN_VIDEO, rsf_frame_record,
};
use core::ffi::c_int;

fn allows_sr(record: &rsf_frame_record) -> bool {
    record.flags & RSF_FRAME_FLAG_NO_SR == 0
        && !matches!(record.screen, RSF_SCREEN_VIDEO | RSF_SCREEN_LOADING)
}

/// Callers may pass just the size word of a short record.
///
/// # Safety
/// A non-null pointer must have an aligned, readable size word and, if it advertises a
/// complete record, point to a readable, aligned frame record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_frame_has_rendered_source(record: *const rsf_frame_record) -> c_int {
    let Some(record) = (unsafe { checked(record) }) else {
        return 0;
    };
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
    // Not `is_some_and(allows_sr)`: that instance lands in a shared generic codegen unit, which
    // then pulls these exports from every Rust archive a DLL links (LNK2005).
    let Some(record) = (unsafe { checked(record) }) else {
        return 0;
    };
    c_int::from(allows_sr(record))
}

/// # Safety
/// See [`rsf_frame_has_rendered_source`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_frame_allows_fg(record: *const rsf_frame_record) -> c_int {
    let Some(record) = (unsafe { checked(record) }) else {
        return 0;
    };
    // Menus hold still and then jump. Unknown screens have no verified interpolation policy.
    c_int::from(
        allows_sr(record)
            && record.flags & RSF_FRAME_FLAG_NO_FG == 0
            && !matches!(record.screen, RSF_SCREEN_MENU | RSF_SCREEN_UNKNOWN),
    )
}
