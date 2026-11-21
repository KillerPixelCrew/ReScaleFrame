// SPDX-License-Identifier: MIT
use super::*;

fn displacement(bytes: &[u8]) -> [u8; 4] {
    bytes.try_into().unwrap()
}

#[test]
fn game_call_reaches_thunk_and_thunk_reads_iat() {
    assert_eq!(CREATE_CALL + CREATE_CALL_BYTES.len(), CREATE_RETURN);
    assert_eq!(
        rel32_target(CREATE_RETURN, displacement(&CREATE_CALL_BYTES[1..])),
        CREATE_THUNK
    );
    assert_eq!(
        rel32_target(
            CREATE_THUNK + CREATE_THUNK_BYTES.len(),
            displacement(&CREATE_THUNK_BYTES[2..])
        ),
        0x17c_92e0
    );
}

#[test]
fn display_invoke_reads_retained_slot_and_returns_at_profile_return() {
    // mov r10,[rip+disp32] is seven bytes; call r10 ends two bytes before the guarded tail.
    assert_eq!(
        rel32_target(
            DISPLAY_INVOKE + 7,
            displacement(&DISPLAY_INVOKE_BYTES[3..7])
        ),
        DISPLAY_RETAINED
    );
    assert_eq!(
        DISPLAY_INVOKE_BYTES[DISPLAY_INVOKE_BYTES.len() - 5..DISPLAY_INVOKE_BYTES.len() - 2],
        [0x41, 0xff, 0xd2]
    );
    assert_eq!(
        DISPLAY_INVOKE + DISPLAY_INVOKE_BYTES.len() - 2,
        DISPLAY_RETURN
    );
}

#[test]
fn relay_is_absolute_indirect_jump() {
    assert_eq!(RELAY_LENGTH, 14);
    assert!(DISPLAY_TWEAKS_SHA256.iter().all(u8::is_ascii_hexdigit));
}

#[test]
fn negative_displacement_wraps_backwards() {
    assert_eq!(rel32_target(0x1000, (-0x10i32).to_le_bytes()), 0xff0);
}
