// SPDX-License-Identifier: MIT
//! The contact-shadow correction at the engine's pixel shader factory.
use super::{Guard, Renderer, copy, hook, installed, original, read};
use crate::sys::{RSF_AC7_CONTACT_PATCHED, RSF_AC7_CONTACT_REFUSED, rsf_ac7_contact_shader_fn};
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

unsafe extern "C" {
    fn rsf_ac7_contact_shadow_correct(
        bytes: *const u8,
        size: usize,
        offset: usize,
        use_shader: rsf_ac7_contact_shader_fn,
        user: *mut c_void,
    ) -> i32;
}

struct Factory<'a> {
    original: Create,
    owner: &'a Renderer,
    rhi: *mut c_void,
    result: *mut c_void,
    created: *mut c_void,
    failed: bool,
}

unsafe extern "C" fn use_shader(
    user: *mut c_void,
    transformed: *const u8,
    size: usize,
    crc: u32,
) -> bool {
    let factory = unsafe { &mut *user.cast::<Factory<'_>>() };
    let replacement = NativeShaderCode {
        data: transformed,
        count: size as i32,
        capacity: size as i32,
    };
    // native_renderer_platform.c calls the factory, containing a C++ exception as the former
    // catch(...) did. False means one was thrown and `created` is unchanged.
    let called = super::native()
        .and_then(|calls| calls.shader_factory)
        .is_some_and(|call| unsafe {
            call(
                factory.original as *mut c_void,
                factory.rhi,
                factory.result,
                (&raw const replacement).cast(),
                &mut factory.created,
            )
        });
    if !called {
        factory.failed = true;
        factory
            .owner
            .log(c"AC7 contact-shadow correction refused: native shader factory failed");
        return false;
    }
    let (mut shader, mut native) = (0u64, 0u64);
    let accepted = copy(
        (&mut shader as *mut u64).cast(),
        factory.result,
        size_of::<u64>(),
    ) && read(shader, 0xa0, &mut native)
        && native != 0;
    if factory.owner.contact_reports.fetch_add(1, Ordering::SeqCst) < 8 {
        factory.owner.log_bounded(
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
    // The matched native factory reads ResourceTableBits and five uint32 arrays, then DXBC.
    // It consumes the borrowed bytes synchronously and retains only shader/table metadata.
    let mut offset = 4usize;
    for _ in 0..5 {
        let mut count = 0u32;
        if offset + 4 > total
            || !copy(
                (&mut count as *mut u32).cast(),
                data.wrapping_add(offset).cast(),
                4,
            )
            || count > 65536
            || count as usize * 4 > total - offset - 4
        {
            return forward();
        }
        offset += 4 + count as usize * 4;
    }
    let (mut optional, mut dxbc_size, mut magic) = (0u32, 0u32, [0u8; 4]);
    if offset + 32 > total
        || !copy(
            magic.as_mut_ptr().cast(),
            data.wrapping_add(offset).cast(),
            4,
        )
        || &magic != b"DXBC"
        || !copy(
            (&mut optional as *mut u32).cast(),
            data.wrapping_add(total - 4).cast(),
            4,
        )
        || optional < 4
        || optional as usize > total - offset
        || !copy(
            (&mut dxbc_size as *mut u32).cast(),
            data.wrapping_add(offset + 24).cast(),
            4,
        )
        || dxbc_size as usize != total - offset - optional as usize
    {
        return forward();
    }
    let mut factory = Factory {
        original: create,
        owner,
        rhi,
        result,
        created: null_mut(),
        failed: false,
    };
    let patched = unsafe {
        rsf_ac7_contact_shadow_correct(
            data,
            total,
            offset,
            Some(use_shader),
            (&mut factory as *mut Factory<'_>).cast(),
        )
    };
    if patched == RSF_AC7_CONTACT_PATCHED as i32 {
        return factory.created;
    }
    if patched == RSF_AC7_CONTACT_REFUSED as i32
        && !factory.failed
        && owner.contact_reports.fetch_add(1, Ordering::SeqCst) < 8
    {
        owner.log(c"AC7 contact-shadow correction refused: shader fingerprint matched but transform validation failed");
    }
    forward()
}
