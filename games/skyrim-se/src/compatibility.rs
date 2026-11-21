// SPDX-License-Identifier: MIT
//! Exact Community Shaders 1.9.1 profile and Skyrim HDR caller provenance.

use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::slice;

use crate::layout::{OWNER, OWNER_DEVICE};

const SHA256: &[u8; 64] = b"10ba95e6b1cd6b675ce74d1d681fae41e667bdd8b7b5ba8c362e657511097827";
const TIMESTAMP: u32 = 0x6ab5_1a9f;
const IMAGE_SIZE: u32 = 0x144_b000;
const MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;
const NT_HEADERS: usize = 264;
const SECTION: usize = 40;
const SECTION_EXECUTE: u32 = 0x2000_0000;

const CONFIGURE: usize = 0x3a_4b30;
const INITIALIZE: usize = 0x63_1800;
const HDR_PROCESS: usize = 0x45_cbd0;
const CALLER: usize = 0x153_adec;
const HDR_CALL: usize = 0x45_cc07;
const HDR_RETURN: usize = 0x45_cc0d;
const HDR_CALLEE: usize = 0x13f_ec40;
const PROCESS_MAIN: usize = 0x14e_dbf0;
const UPSCALING: usize = 0x13d_cf40;
const HDR_DISPLAY: usize = 0x13d_cda0;
const STREAMLINE: usize = 0xfd_9160;
const LOADED: usize = 8;
const HDR_CALL_BYTES: [u8; 6] = [0xff, 0x15, 0x33, 0x20, 0xfa, 0x00];

const GUARDS: [(usize, [u8; 19]); 6] = [
    (
        CONFIGURE,
        [
            0x48, 0x89, 0x54, 0x24, 0x10, 0x48, 0x89, 0x4c, 0x24, 0x08, 0x53, 0x56, 0x57, 0x41,
            0x54, 0x41, 0x55, 0x41, 0x56,
        ],
    ),
    (
        0x4f_9380,
        [
            0x48, 0x83, 0xec, 0x48, 0x48, 0x8d, 0x05, 0xad, 0x61, 0x21, 0x00, 0x48, 0xc7, 0x44,
            0x24, 0x28, 0x1d, 0x00, 0x00,
        ],
    ),
    (
        0x4f_8ed0,
        [
            0x40, 0x55, 0x53, 0x56, 0x57, 0x41, 0x56, 0x48, 0x8b, 0xec, 0x48, 0x81, 0xec, 0x80,
            0x00, 0x00, 0x00, 0x48, 0x8d,
        ],
    ),
    (
        0x4f_7030,
        [
            0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x48, 0x83, 0xec,
            0x60, 0x80, 0x3d, 0x1d, 0x21,
        ],
    ),
    (
        INITIALIZE,
        [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56,
            0x41, 0x57, 0x48, 0x8d, 0xac,
        ],
    ),
    (
        HDR_PROCESS,
        [
            0x40, 0x53, 0x55, 0x56, 0x57, 0x48, 0x83, 0xec, 0x38, 0x48, 0x8b, 0xe9, 0x49, 0x8b,
            0xd9, 0x48, 0x8d, 0x0d, 0xba,
        ],
    ),
];

type Read = unsafe extern "C" fn(usize, *mut c_void, usize) -> bool;

fn read_native<const N: usize>(read: Read, address: usize) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    unsafe { read(address, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
}

fn part(bytes: &[u8], start: usize, count: usize) -> Option<&[u8]> {
    bytes.get(start..start.checked_add(count)?)
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(part(bytes, offset, 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(part(bytes, offset, 4)?.try_into().ok()?))
}

fn validate(image: &[u8]) -> Option<()> {
    let dos = part(image, 0, 64)?;
    if u16_at(dos, 0)? != 0x5a4d {
        return None;
    }
    let nt = usize::try_from(i32::from_le_bytes(dos[0x3c..0x40].try_into().ok()?)).ok()?;
    let header = part(image, nt, NT_HEADERS)?;
    let count = usize::from(u16_at(header, 6)?);
    if u32_at(header, 0)? != 0x0000_4550
        || u16_at(header, 4)? != 0x8664
        || u32_at(header, 8)? != TIMESTAMP
        || u16_at(header, 20)? != 240
        || u16_at(header, 24)? != 0x20b
        || u32_at(header, 80)? != IMAGE_SIZE
        || !(1..=96).contains(&count)
    {
        return None;
    }
    let sections = part(image, nt.checked_add(NT_HEADERS)?, count * SECTION)?;
    for (rva, expected) in &GUARDS {
        let mut found = false;
        for section in sections.chunks_exact(SECTION) {
            let address = u32_at(section, 12)? as usize;
            let size = u32_at(section, 16)? as usize;
            let Some(delta) = rva.checked_sub(address) else {
                continue;
            };
            if delta > size || expected.len() > size - delta {
                continue;
            }
            let raw = (u32_at(section, 20)? as usize).checked_add(delta)?;
            if part(image, raw, expected.len())? != expected
                || u32_at(section, 36)? & SECTION_EXECUTE == 0
            {
                return None;
            }
            found = true;
            break;
        }
        if !found {
            return None;
        }
    }
    Some(())
}

/// The native verifier lends the hex digest while its read-only file handle is held.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_hash_matches(bytes: *const c_char, len: usize) -> bool {
    len == SHA256.len()
        && !bytes.is_null()
        && unsafe { slice::from_raw_parts(bytes.cast::<u8>(), len) } == SHA256
}

/// The caller keeps the read-only file mapping live. The optional loaded image is read through a guarded leaf.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_profile(
    image: *const u8,
    len: usize,
    module: usize,
    read: Read,
) -> bool {
    if image.is_null()
        || !(1..=MAX_IMAGE_BYTES).contains(&len)
        || validate(unsafe { slice::from_raw_parts(image, len) }).is_none()
    {
        return false;
    }
    module == 0
        || GUARDS.iter().all(|(rva, expected)| {
            module
                .checked_add(*rva)
                .and_then(|address| read_native::<19>(read, address))
                .as_ref()
                == Some(expected)
        })
}

#[repr(C)]
struct Sites {
    configure: usize,
    initialize: usize,
    hdr_process: usize,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_sites() -> Sites {
    Sites {
        configure: CONFIGURE,
        initialize: INITIALIZE,
        hdr_process: HDR_PROCESS,
    }
}

fn before_device(game: usize, read: Read) -> Option<bool> {
    if game == 0 {
        return None;
    }
    let owner = usize::from_le_bytes(read_native(read, game.checked_add(OWNER)?)?);
    if owner == 0 {
        return Some(true);
    }
    let device = usize::from_le_bytes(read_native(read, owner.checked_add(OWNER_DEVICE)?)?);
    Some(device == 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_before_device(game: usize, read: Read) -> bool {
    before_device(game, read) == Some(true)
}

fn features_inactive(base: usize, check_hdr: bool, read: Read) -> Option<bool> {
    if base == 0 {
        return None;
    }
    let upscaling = read_native::<1>(read, base.checked_add(UPSCALING + LOADED)?)?;
    if upscaling != [0] || read_native::<3>(read, base.checked_add(STREAMLINE)?)? != [0; 3] {
        return Some(false);
    }
    Some(!check_hdr || read_native::<1>(read, base.checked_add(HDR_DISPLAY + LOADED)?)? == [0])
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_features_inactive(base: usize, check_hdr: bool, read: Read) -> bool {
    features_inactive(base, check_hdr, read) == Some(true)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_excluded_byte(base: usize, feature: usize) -> usize {
    if base != 0 && base.checked_add(UPSCALING) == Some(feature) {
        feature.checked_add(LOADED).unwrap_or(0)
    } else {
        0
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HdrCall {
    caller: usize,
    manager: usize,
    argument4: usize,
    input: u32,
    output: u32,
    argument5: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Scope {
    call: HdrCall,
    primary: bool,
    active: bool,
}

const _: () = {
    assert!(size_of::<Sites>() == 24);
    assert!(size_of::<HdrCall>() == 40 && std::mem::offset_of!(HdrCall, argument5) == 32);
    assert!(size_of::<Scope>() == 48);
};

thread_local! {
    static HDR_SCOPE: Cell<Option<Scope>> = const { Cell::new(None) };
}

/// The typed detour lends the actual native caller and restores the scope during unwind.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_cs_hdr_enter(game: usize, call: &HdrCall, saved: *mut Scope) {
    let prev = HDR_SCOPE.get();
    HDR_SCOPE.set(Some(Scope {
        call: *call,
        primary: prev.is_none() && game != 0 && game.checked_add(CALLER) == Some(call.caller),
        active: true,
    }));
    unsafe {
        saved.write(prev.unwrap_or(Scope {
            call: *call,
            primary: false,
            active: false,
        }))
    };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_hdr_leave(saved: &Scope) {
    HDR_SCOPE.set(saved.active.then_some(*saved));
}

fn allowed(game: usize, module: usize, excluded: bool, call: &HdrCall, read: Read) -> Option<()> {
    if game == 0 {
        return None;
    }
    let native_caller = game.checked_add(CALLER) == Some(call.caller);
    let Some(scope) = HDR_SCOPE.get() else {
        return native_caller.then_some(());
    };
    if !excluded
        || module == 0
        || !scope.primary
        || module.checked_add(HDR_RETURN) != Some(call.caller)
        || scope.call.manager != call.manager
        || scope.call.input != call.input
        || scope.call.output != call.output
        || scope.call.argument4 != call.argument4
        || scope.call.argument5 != call.argument5
        || read_native::<6>(read, module.checked_add(HDR_CALL)?)? != HDR_CALL_BYTES
    {
        return None;
    }
    let original = usize::from_le_bytes(read_native(read, module.checked_add(HDR_CALLEE)?)?);
    (game.checked_add(PROCESS_MAIN) == Some(original)).then_some(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_cs_postprocess_allowed(
    game: usize,
    module: usize,
    excluded: bool,
    call: &HdrCall,
    read: Read,
) -> bool {
    allowed(game, module, excluded, call, read).is_some()
}
