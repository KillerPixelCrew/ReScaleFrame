// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn relay_is_absolute_indirect_jump() {
    assert_eq!(RELAY_LENGTH, 14);
}

#[test]
fn negative_displacement_wraps_backwards() {
    assert_eq!(rel32_target(0x1000, (-0x10i32).to_le_bytes()), 0xff0);
}

#[test]
fn cold_owner_requires_readable_checked_addresses() {
    // Unit tests never discover the owner global, so the image owner is unknown.
    assert_eq!(before_device(0, |_| panic!()), None);
    assert_eq!(before_device(0x1_4000_0000, |_| panic!()), None);
    assert_eq!(cold_owner(1, |_| None), None);
    assert_eq!(cold_owner(1, |_| Some(0)), Some(true));
    assert_eq!(cold_owner(1, |_| Some(usize::MAX)), None);
    assert_eq!(cold_owner(1, |address| (address == 1).then_some(100)), None);
    assert_eq!(
        cold_owner(1, |address| Some(if address == 1 { 100 } else { 1 })),
        Some(false)
    );
}
