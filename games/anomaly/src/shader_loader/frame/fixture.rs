// SPDX-License-Identifier: MIT
use super::{FrameLayout, FrameStatus, decode_frame};
use crate::shader_loader::rsf_anomaly_shader_skin_mode;
use std::ffi::{CStr, c_char};

const IMAGE: usize = 0x1000_0000;
const STACK: usize = 0x1000;
const ROOT: usize = 0x5000;
const ENTRY: usize = 0x6000;
const MACROS: usize = 0x7000;
const SOURCE: &[u8] = b"#include \"common.h\"\nmain body";
const DEFINES: [(&[u8], &[u8]); 3] = [
    (b"SMAP_size\0", b"2048\0"),
    (b"SKIN_2\0", b"1\0"),
    (b"USE_SUNFILTER\0", b"1\0"),
];

fn caller_frame(layout: &FrameLayout, other: &FrameLayout) -> Result<(), &'static CStr> {
    let mut stack = vec![0u8; 0x2000];
    let mut macros = [0u8; 96];
    let write = |stack: &mut [u8], offset: u32, bytes: &[u8]| {
        let offset = offset as usize;
        stack[offset..offset + bytes.len()].copy_from_slice(bytes);
    };
    write(&mut stack, layout.source, &(ROOT as u64).to_le_bytes());
    write(
        &mut stack,
        layout.length,
        &(SOURCE.len() as u32).to_le_bytes(),
    );
    write(&mut stack, layout.entry, &(ENTRY as u64).to_le_bytes());
    write(&mut stack, layout.flags, &8u32.to_le_bytes());
    for (i, (name, value)) in DEFINES.into_iter().enumerate() {
        let offset = i * 32;
        macros[offset..offset + name.len()].copy_from_slice(name);
        macros[offset + 16..offset + 16 + value.len()].copy_from_slice(value);
        let pair = layout.defines + i as u32 * 16;
        write(&mut stack, pair, &((MACROS + offset) as u64).to_le_bytes());
        write(
            &mut stack,
            pair + 8,
            &((MACROS + offset + 16) as u64).to_le_bytes(),
        );
    }

    let decode = |profile, return_address, stack: &[u8], entry: &[u8], fault| {
        decode_frame(profile, IMAGE, return_address, STACK, |address, output| {
            if fault == Some(address) {
                return false;
            }
            for (base, bytes) in [
                (STACK, stack),
                (ROOT, SOURCE),
                (ENTRY, entry),
                (MACROS, &macros),
            ] {
                if let Some(offset) = address.checked_sub(base)
                    && let Some(end) = offset.checked_add(output.len())
                    && let Some(bytes) = bytes.get(offset..end)
                {
                    output.copy_from_slice(bytes);
                    return true;
                }
            }
            false
        })
    };
    let returns = layout
        .calls
        .map(|rva| IMAGE + rva as usize + layout.call_length as usize);
    for return_address in returns {
        let frame = decode(layout, return_address, &stack, b"main\0", None)
            .map_err(|_| c"both create_shader call sites must be accepted")?;
        if frame.source != SOURCE || frame.entry != b"main" || frame.flags != 8 {
            return Err(c"compiler frame source, entry, or flags differ");
        }
        if !frame
            .defines
            .iter()
            .map(|(name, value)| (name.as_slice(), value.as_slice()))
            .eq(DEFINES.map(|(name, value)| (&name[..name.len() - 1], &value[..value.len() - 1])))
            || frame.with_request(b"vs_5_0", &[], |request| unsafe {
                rsf_anomaly_shader_skin_mode(request.defines)
            }) != 2
        {
            return Err(c"compiler defines must retain engine order and SKIN_2");
        }
    }
    for (profile, address) in [
        (layout, IMAGE + 0x1234),
        (layout, returns[0] - layout.call_length as usize),
        (other, returns[0]),
    ] {
        if !matches!(
            decode(profile, address, &stack, b"main\0", None),
            Err(FrameStatus::UnknownCaller)
        ) {
            return Err(c"compiler call site outside its selected profile must be refused");
        }
    }

    let mut unterminated = stack.clone();
    for i in DEFINES.len() as u32..layout.define_capacity {
        let pair = layout.defines + i * 16;
        write(&mut unterminated, pair, &(MACROS as u64).to_le_bytes());
        write(
            &mut unterminated,
            pair + 8,
            &((MACROS + 16) as u64).to_le_bytes(),
        );
    }
    if !matches!(
        decode(layout, returns[0], &unterminated, b"main\0", None),
        Err(FrameStatus::BadDefines)
    ) {
        return Err(c"unterminated compiler define list must be refused");
    }
    let entry_fault = Some(STACK + layout.entry as usize);
    if !matches!(
        decode(layout, returns[1], &stack, b"main\0", entry_fault),
        Err(FrameStatus::Unreadable)
    ) {
        return Err(c"unreadable compiler frame must be refused");
    }
    if !matches!(
        decode(layout, returns[1], &stack, &[b'x'; 512], None),
        Err(FrameStatus::BadSource)
    ) {
        return Err(c"entry must terminate inside the guarded 512-byte window");
    }
    for length in [0u32, 16 * 1024 * 1024 + 1] {
        write(&mut stack, layout.length, &length.to_le_bytes());
        if !matches!(
            decode(layout, returns[1], &stack, b"main\0", None),
            Err(FrameStatus::BadSource)
        ) {
            return Err(c"empty and oversized source must be refused");
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_frame_fixture(
    standard: &FrameLayout,
    avx: &FrameLayout,
) -> *const c_char {
    caller_frame(standard, avx)
        .and_then(|()| caller_frame(avx, standard))
        .err()
        .map_or(std::ptr::null(), CStr::as_ptr)
}
