// SPDX-License-Identifier: MIT
use std::ffi::{CStr, c_char, c_void};

#[repr(C)]
struct Image {
    base: *const u8,
    size: u32,
    read: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool,
}

const _: () = {
    assert!(size_of::<Image>() == 24);
    assert!(std::mem::offset_of!(Image, read) == 16);
};

impl Image {
    fn address(&self, rva: u32, length: usize) -> Option<*const u8> {
        if self.base.is_null() || length == 0 {
            return None;
        }
        let end = (rva as usize).checked_add(length)?;
        if end > self.size as usize {
            return None;
        }
        self.base.addr().checked_add(end - 1)?;
        Some(self.base.wrapping_add(rva as usize))
    }

    fn copy<const N: usize>(&self, rva: u32) -> Option<[u8; N]> {
        let address = self.address(rva, N)?;
        let mut bytes = [0; N];
        // Native owns the exception guard for mapped executable memory.
        unsafe { (self.read)(bytes.as_mut_ptr().cast(), address.cast(), N) }.then_some(bytes)
    }

    fn relative_call(&self, rva: u32, target: u32) -> bool {
        target < self.size
            && self.copy::<5>(rva).is_some_and(|[opcode, a, b, c, d]| {
                opcode == 0xe8
                    && i64::from(rva) + 5 + i64::from(i32::from_le_bytes([a, b, c, d]))
                        == i64::from(target)
            })
    }

    fn matches_hex(&self, rva: u32, hex: &[u8]) -> bool {
        let pairs = hex.chunks_exact(2);
        if !pairs.remainder().is_empty() || self.address(rva, pairs.len()).is_none() {
            return false;
        }
        pairs.enumerate().all(|(offset, pair)| {
            let (Some(high), Some(low)) = (digit(pair[0]), digit(pair[1])) else {
                return false;
            };
            self.copy(rva + offset as u32) == Some([(high << 4) | low])
        })
    }
}

fn digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_guard_site(image: &Image, rva: u32, expected: &[u8; 16]) -> bool {
    image.copy(rva).as_ref() == Some(expected)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_guard_hex(image: &Image, rva: u32, hex: *const c_char) -> bool {
    // The native profile supplies a static NUL-terminated guard.
    !hex.is_null() && image.matches_hex(rva, unsafe { CStr::from_ptr(hex) }.to_bytes())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_guard_first_person(
    image: &Image,
    set_p_call: u32,
    rm_near_call: u32,
    set_p: u32,
    rm_near: u32,
) -> bool {
    // The verified rmNear site calls slot 55 or the profiled function shortly after set_P.
    const RM_NEAR_SLOT: [u8; 6] = [0xff, 0x90, 0xb8, 0x01, 0x00, 0x00];
    rm_near_call
        .checked_sub(set_p_call)
        .is_some_and(|distance| (1..0x120).contains(&distance))
        && image.relative_call(set_p_call, set_p)
        && (image.copy(rm_near_call) == Some(RM_NEAR_SLOT)
            || image.relative_call(rm_near_call, rm_near))
}
