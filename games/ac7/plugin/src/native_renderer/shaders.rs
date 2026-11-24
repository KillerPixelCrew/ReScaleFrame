// SPDX-License-Identifier: MIT
//! The contact-shadow correction at the engine's pixel shader factory.
use super::{Guard, Renderer, copy, hook, installed, key, original, read};
use rsf_ac7::contact_shadow::{Correction, correct};
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr::null_mut;
use std::sync::atomic::Ordering;

/// The engine's shader code view (TArray<uint8>): data, count, capacity.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct NativeShaderCode {
    data: *const u8,
    count: i32,
    capacity: i32,
}

const _: () =
    assert!(size_of::<NativeShaderCode>() == 16 && offset_of!(NativeShaderCode, count) == 8);

type Create =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const NativeShaderCode) -> *mut c_void;

/// Creates the corrected shader through the engine factory `create`, into `created`. False when
/// the factory threw.
fn create_corrected(
    owner: &Renderer,
    create: Create,
    rhi: *mut c_void,
    result: *mut c_void,
    transformed: &[u8],
    crc: u32,
    created: &mut *mut c_void,
) -> bool {
    let replacement = NativeShaderCode {
        data: transformed.as_ptr(),
        count: transformed.len() as i32,
        capacity: transformed.len() as i32,
    };
    // native_renderer_platform.c calls the factory, containing a C++ exception as the former
    // catch(...) did. False means one was thrown and `created` is unchanged.
    let called = super::native()
        .and_then(|calls| calls.shader_factory)
        .is_some_and(|call| unsafe {
            call(
                create as *mut c_void,
                rhi,
                result,
                (&raw const replacement).cast(),
                created,
            )
        });
    if !called {
        owner.log(c"AC7 contact-shadow correction refused: native shader factory failed");
        return false;
    }
    let (mut shader, mut native) = (0u64, 0u64);
    let accepted = copy((&mut shader as *mut u64).cast(), result, size_of::<u64>())
        && read(shader, 0xa0, &mut native)
        && native != 0;
    if owner.contact_reports.fetch_add(1, Ordering::SeqCst) < 8 {
        owner.log_bounded(
            &format!(
                "AC7 contact-shadow correction: native factory resource={} RHI={:x} D3D11={:x} CRC={}; View noise phase and depth quantisation bias, native light/ray parameters preserved",
                u32::from(accepted),
                shader,
                native,
                crc
            ),
            288,
        );
    }
    true
}

pub(super) unsafe extern "C" fn hooked_create_pixel_shader(
    rhi: *mut c_void,
    result: *mut c_void,
    code: *const NativeShaderCode,
) -> *mut c_void {
    let _lifetime = Guard::outer();
    let create: Create = unsafe { original(hook::CREATE_PIXEL_SHADER) };
    let forward = || unsafe { create(rhi, result, code) };
    let mut input = NativeShaderCode {
        data: std::ptr::null(),
        count: 0,
        capacity: 0,
    };
    let Some(owner) = installed() else {
        return forward();
    };
    if owner.quiescing()
        || !copy(
            (&mut input as *mut NativeShaderCode).cast(),
            code.cast(),
            size_of::<NativeShaderCode>(),
        )
        || input.data.is_null()
        || input.count < 32
        || input.count > 8 * 1024 * 1024
        || input.capacity < input.count
    {
        return forward();
    }
    let total = input.count as usize;
    let data = input.data;
    let address = key(data.cast());
    // The matched native factory reads ResourceTableBits and five uint32 arrays, then DXBC.
    // It consumes the borrowed bytes synchronously and retains only shader/table metadata.
    let mut offset = 4usize;
    for _ in 0..5 {
        let mut count = 0u32;
        if offset + 4 > total
            || !read(address, offset, &mut count)
            || count > 65536
            || count as usize * 4 > total - offset - 4
        {
            return forward();
        }
        offset += 4 + count as usize * 4;
    }
    let (mut optional, mut dxbc_size, mut magic) = (0u32, 0u32, [0u8; 4]);
    if offset + 32 > total
        || !read(address, offset, &mut magic)
        || &magic != b"DXBC"
        || !read(address, total - 4, &mut optional)
        || optional < 4
        || optional as usize > total - offset
        || !read(address, offset + 24, &mut dxbc_size)
        || dxbc_size as usize != total - offset - optional as usize
    {
        return forward();
    }
    // The guarded reads above bound the engine's borrowed shader bytes, which the factory
    // consumes synchronously.
    let source = unsafe { std::slice::from_raw_parts(data, total) };
    let mut created = null_mut();
    let mut failed = false;
    let correction = correct(source, offset, |transformed, crc| {
        let made = create_corrected(owner, create, rhi, result, transformed, crc, &mut created);
        failed = !made;
        made
    });
    if correction == Correction::Patched {
        return created;
    }
    if correction == Correction::Refused
        && !failed
        && owner.contact_reports.fetch_add(1, Ordering::SeqCst) < 8
    {
        owner.log(c"AC7 contact-shadow correction refused: shader fingerprint matched but transform validation failed");
    }
    forward()
}
