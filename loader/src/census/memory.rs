// SPDX-License-Identifier: MIT
//! Reads of the host process's memory, through the platform's guarded reader.
use super::platform;

pub(super) fn plausible(address: u64) -> bool {
    (0x1_0000..0x0000_7fff_ffff_0000).contains(&address)
}

pub(super) fn read<const N: usize>(address: u64, offset: usize) -> Option<[u8; N]> {
    if !plausible(address) {
        return None;
    }
    let address = address.checked_add(offset as u64)?;
    let mut bytes = [0; N];
    unsafe { platform().read.unwrap()(address as _, bytes.as_mut_ptr().cast(), N) }.then_some(bytes)
}

/// A native structure read from the host process. The type must accept arbitrary bit patterns.
pub(super) unsafe fn read_as<T: Default>(address: u64) -> Option<T> {
    if !plausible(address) {
        return None;
    }
    let mut value = T::default();
    unsafe { platform().read.unwrap()(address as _, (&raw mut value).cast(), size_of::<T>()) }
        .then_some(value)
}

pub(super) fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

pub(super) fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
