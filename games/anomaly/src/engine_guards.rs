// SPDX-License-Identifier: MIT
mod motion;
mod sr;

use std::ffi::{CStr, c_char, c_void};
use std::ptr::with_exposed_provenance;

pub(super) type Read = unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool;

pub(super) fn read_bytes<const N: usize>(read: Read, address: usize) -> Option<[u8; N]> {
    if address == 0 {
        return None;
    }
    address.checked_add(N)?;
    let mut bytes = [0; N];
    // The native leaf guards the read and reports whether the complete span was copied.
    unsafe {
        read(
            bytes.as_mut_ptr().cast(),
            with_exposed_provenance(address),
            N,
        )
    }
    .then_some(bytes)
}

#[repr(C)]
pub(super) struct Image {
    base: *const u8,
    size: u32,
    pub(super) read: Read,
}
#[repr(C)]
pub(super) struct NativeSite {
    pub(super) name: *const c_char,
    pub(super) rva: u32,
    pub(super) expected: [u8; 16],
}

#[cfg(windows)]
pub(super) const fn site(name: &'static CStr, rva: u32, hex: &[u8; 32]) -> NativeSite {
    const fn digit(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("invalid native guard digit"),
        }
    }
    let mut expected = [0; 16];
    let mut i = 0;
    while i < expected.len() {
        expected[i] = digit(hex[2 * i]) << 4 | digit(hex[2 * i + 1]);
        i += 1;
    }
    NativeSite {
        name: name.as_ptr(),
        rva,
        expected,
    }
}

#[repr(C)]
pub(super) struct UniqueGuard {
    name: *const c_char,
    pub(super) rva: u32,
    pub(super) expected_hex: *const c_char,
}

#[cfg(windows)]
impl UniqueGuard {
    pub(super) const fn new(name: &'static CStr, rva: u32, expected: &'static CStr) -> Self {
        Self {
            name: name.as_ptr(),
            rva,
            expected_hex: expected.as_ptr(),
        }
    }
}

const _: () = {
    assert!(size_of::<Image>() == 24);
    assert!(std::mem::offset_of!(Image, read) == 16);
    assert!(size_of::<NativeSite>() == 32);
    assert!(std::mem::offset_of!(NativeSite, rva) == 8);
    assert!(std::mem::offset_of!(NativeSite, expected) == 12);
    assert!(size_of::<UniqueGuard>() == 24);
    assert!(std::mem::offset_of!(UniqueGuard, rva) == 8);
    assert!(std::mem::offset_of!(UniqueGuard, expected_hex) == 16);
};

impl Image {
    pub(super) fn new(base: *const u8, size: u32, read: Read) -> Self {
        Self { base, size, read }
    }

    pub(super) fn matches(&self, site: &NativeSite) -> bool {
        self.matches_site(site.rva, &site.expected)
    }

    pub(super) unsafe fn matches_sites(
        &self,
        sites: &[NativeSite],
        guards: &[UniqueGuard],
    ) -> bool {
        sites.iter().all(|site| {
            self.matches(site)
                && guards
                    .iter()
                    .filter(|guard| guard.rva == site.rva)
                    // The native profile lends static NUL-terminated guard strings.
                    .all(|guard| unsafe { self.matches_hex_guard(guard.rva, guard.expected_hex) })
        })
    }

    pub(super) fn address(&self, rva: u32, length: usize) -> Option<*const u8> {
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

    pub(super) fn copy<const N: usize>(&self, rva: u32) -> Option<[u8; N]> {
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

    fn matches_site(&self, rva: u32, expected: &[u8; 16]) -> bool {
        self.copy(rva).as_ref() == Some(expected)
    }

    pub(super) unsafe fn matches_hex_guard(&self, rva: u32, hex: *const c_char) -> bool {
        !hex.is_null() && self.matches_hex(rva, unsafe { CStr::from_ptr(hex) }.to_bytes())
    }

    fn matches_hex(&self, rva: u32, hex: &[u8]) -> bool {
        let Some(address) = self.address(rva, hex.len() / 2) else {
            return false;
        };
        let Some(expected) = decode_hex(hex) else {
            return false;
        };
        let mut bytes = vec![0u8; expected.len()];
        let copied = unsafe { (self.read)(bytes.as_mut_ptr().cast(), address.cast(), bytes.len()) };
        copied && bytes == expected
    }
}

pub(super) fn decode_hex(hex: &[u8]) -> Option<Vec<u8>> {
    let pairs = hex.chunks_exact(2);
    if !pairs.remainder().is_empty() {
        return None;
    }
    pairs
        .map(|pair| Some((digit(pair[0])? << 4) | digit(pair[1])?))
        .collect()
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
    image.matches_site(rva, expected)
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

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn copy(out: *mut c_void, input: *const c_void, size: usize) -> bool {
        unsafe { std::ptr::copy_nonoverlapping(input.cast::<u8>(), out.cast(), size) };
        true
    }

    #[test]
    fn site_admission_requires_the_unique_suffix() {
        let name = c"fixture".as_ptr();
        let sites = [NativeSite {
            name,
            rva: 0,
            expected: [0x90; 16],
        }];
        let guards = [UniqueGuard {
            name,
            rva: 0,
            expected_hex: c"9090909090909090909090909090909091".as_ptr(),
        }];
        let mut bytes = [0x90; 17];
        let matches = |bytes: &[u8; 17]| {
            let image = Image::new(bytes.as_ptr(), bytes.len() as u32, copy);
            unsafe { image.matches_sites(&sites, &guards) }
        };
        assert!(!matches(&bytes));
        bytes[16] = 0x91;
        assert!(matches(&bytes));
    }
}

#[cfg(test)]
mod range_tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static READ_LENGTH: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "C" fn record(out: *mut c_void, src: *const c_void, len: usize) -> bool {
        let previous = READ_LENGTH.replace(len);
        if previous != 0 {
            return false;
        }
        unsafe { std::ptr::copy_nonoverlapping(src.cast::<u8>(), out.cast::<u8>(), len) };
        true
    }

    #[test]
    fn hex_guard_checks_one_bounded_read() {
        let bytes = [0xff, 0x12, 0x34, 0xab];
        let image = Image::new(bytes.as_ptr(), bytes.len() as u32, record);
        assert!(image.matches_hex(1, b"1234ab"));
        assert_eq!(READ_LENGTH.replace(0), 3);
        assert!(!image.matches_hex(1, b"1234aa"));
        assert_eq!(READ_LENGTH.replace(0), 3);
        assert!(!image.matches_hex(2, b"1234ab"));
        assert!(!image.matches_hex(1, b"1234a"));
        assert!(!image.matches_hex(1, b"12z4ab"));
        assert_eq!(READ_LENGTH.get(), 0);
    }
}
