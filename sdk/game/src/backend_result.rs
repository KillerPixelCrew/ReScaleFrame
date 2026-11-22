// SPDX-License-Identifier: MIT
//! `rsf_backend_result` values from `rescaleframe/backend.h`. The header spells them as cast
//! macros, which bindgen does not evaluate, so they are written once here.

pub const OK: i32 = 0;
pub const INVALID_ARGUMENT: i32 = -1;
pub const ABI_MISMATCH: i32 = -2;
pub const NOT_COMPILED: i32 = -3;
pub const LOAD_FAILED: i32 = -4;
pub const MISSING_ENTRY_POINT: i32 = -5;
pub const INIT_FAILED: i32 = -6;
pub const NOT_SUPPORTED: i32 = -7;
pub const NOT_READY: i32 = -8;
pub const FEATURE_FAILED: i32 = -9;
pub const STALE_RESOURCES: i32 = -10;
pub const NEEDS_RESTART: i32 = -11;
pub const WRONG_API: i32 = -12;
