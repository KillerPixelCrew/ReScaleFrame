// SPDX-License-Identifier: MIT
//! Validate versioned GameSDK calls before borrowing the complete structure.

use crate::{
    RSF_ERROR_ABI_MISMATCH, RSF_ERROR_INVALID_ARGUMENT, RSF_GAME_ABI_VERSION, RSF_OK, rsf_result,
};

/// Checks size before ABI version and returns the GameSDK result code.
///
/// # Safety
/// `T` must be a GameSDK call structure beginning with `struct_size` and
/// `abi_version` as consecutive `u32` fields. A nonnull `value` must be
/// aligned and readable through its size field and through its version field
/// when it advertises at least `size_of::<T>()` bytes.
pub unsafe fn validate<T>(value: *const T) -> rsf_result {
    unsafe { validate_abi(value, RSF_GAME_ABI_VERSION) }
}

/// `validate` for a structure versioned by its own ABI number, such as a game adapter's export.
///
/// # Safety
/// As for `validate`.
pub unsafe fn validate_abi<T>(value: *const T, abi: u32) -> rsf_result {
    let header = value.cast::<u32>();
    if header.is_null() || unsafe { header.read() } < size_of::<T>() as u32 {
        RSF_ERROR_INVALID_ARGUMENT
    } else if unsafe { header.add(1).read() } != abi {
        RSF_ERROR_ABI_MISMATCH
    } else {
        RSF_OK
    }
}
