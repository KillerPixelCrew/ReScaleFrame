// SPDX-License-Identifier: MIT
mod d3d;
#[cfg(feature = "motion-device-fixture")]
pub(crate) mod deployed;
#[cfg(feature = "motion-binder-fixture")]
mod fixture;
mod frame;
mod manifest;
mod reflection;
mod runtime_pack;
#[cfg(any(feature = "motion-binder-fixture", feature = "motion-device-fixture"))]
mod tree;

pub(crate) use crate::sys::{
    rsf_anomaly_engine_include_ops as EngineIncludeOps,
    rsf_anomaly_engine_shader_profile as EngineShaderProfile,
};
#[cfg(feature = "motion-binder-fixture")]
pub(crate) use fixture::SHADER_CONTRACT_CASES;
#[cfg(feature = "motion-binder-fixture")]
pub(crate) use frame::run_frame_fixture;
pub(crate) use frame::{DecodedFrame, FrameLayout, Read, decode_frame};
pub(crate) use manifest::{Entry, Manifest};
use reflection::Compatibility;
pub(crate) use reflection::{
    Reflection, Snapshot, rsf_anomaly_shader_linkable, rsf_anomaly_shader_tess_linkable,
};
pub(crate) use runtime_pack::RuntimePack;
use std::ffi::CStr;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct Span<T> {
    data: *const T,
    len: usize,
}

impl<T> From<&[T]> for Span<T> {
    fn from(items: &[T]) -> Self {
        Self {
            data: items.as_ptr(),
            len: items.len(),
        }
    }
}

impl<T> Span<T> {
    // Borrow only while the native owner (or the synchronous Rust caller) keeps it alive.
    pub(crate) unsafe fn as_slice<'a>(&self) -> &'a [T] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.data, self.len) }
        }
    }
}

pub(crate) type Bytes = Span<u8>;

#[derive(Clone, Copy)]
#[repr(C)]
struct Define {
    name: Bytes,
    value: Bytes,
}

#[repr(C)]
pub(crate) struct Request {
    target: Bytes,
    entry: Bytes,
    flags: u32,
    defines: Span<Define>,
    source: Bytes,
    bytecode: Bytes,
}

const _: () = {
    assert!(size_of::<Request>() == 88);
    assert!(std::mem::offset_of!(Request, defines) == 40);
    assert!(std::mem::offset_of!(Request, bytecode) == 72);
};

unsafe fn rsf_anomaly_shader_skin_mode(defines: Span<Define>) -> i32 {
    unsafe { defines.as_slice() }
        .iter()
        .fold(-2, |mode, define| match unsafe { define.name.as_slice() } {
            b"SKIN_NONE" => -1,
            [b'S', b'K', b'I', b'N', b'_', digit @ b'0'..=b'4'] => i32::from(digit - b'0'),
            _ => mode,
        })
}

#[repr(u8)]
#[derive(Clone, Copy)]
enum Outcome {
    Replaced = 1,
    ReplacedFromCache = 2,
    #[cfg(any(feature = "motion-binder-fixture", feature = "motion-device-fixture"))]
    OriginalCompileFailed = 5,
    OriginalMismatch = 6,
    MotionCompileFailed = 7,
    ReflectionMismatch = 8,
}

pub(crate) const OUTCOME_NAMES: [&CStr; 10] = [
    c"not_allowlisted",
    c"replaced",
    c"replaced_from_cache",
    c"unknown_caller",
    c"unreadable",
    c"original_compile_failed",
    c"original_mismatch",
    c"motion_compile_failed",
    c"reflection_mismatch",
    c"create_failed",
];

pub(crate) fn outcome_name(value: u8) -> &'static str {
    outcome_label(value)
        .to_str()
        .expect("outcome names are ASCII")
}

fn outcome_label(value: u8) -> &'static CStr {
    OUTCOME_NAMES
        .get(usize::from(value))
        .copied()
        .unwrap_or(c"unknown")
}

fn runtime_refusal(status: i32) -> Option<Outcome> {
    use rsf_game_sys::{RSF_PATCH_OK, RSF_PATCH_ORIGINAL, RSF_PATCH_REFLECTION};
    match status {
        RSF_PATCH_OK => None,
        RSF_PATCH_ORIGINAL => Some(Outcome::OriginalMismatch),
        RSF_PATCH_REFLECTION => Some(Outcome::ReflectionMismatch),
        _ => Some(Outcome::MotionCompileFailed),
    }
}
