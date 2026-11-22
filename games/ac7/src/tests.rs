// SPDX-License-Identifier: MIT
use super::*;
use std::ptr::null;

fn probe(machine: u16, executable: &CStr, digest: &CStr) -> rsf_game_probe {
    rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: machine,
        reserved: 0,
        executable_name_utf8: executable.as_ptr(),
        sha256_hex: digest.as_ptr(),
    }
}

#[test]
fn detection_needs_the_exact_amd64_build() {
    let digest = c"C7DA97F5F8A807D4F1264ADBB074146FCFFE9BDC2FFA98791B822CD28E558F4F";
    let accepted = probe(AMD64, c"ace7game.EXE", digest);
    assert_eq!(unsafe { detect(&accepted) }, RECOGNIZED);
    for refused in [
        probe(0x14c, EXECUTABLE, EXECUTABLE_SHA256),
        probe(AMD64, c"Ace7Game.exe.bak", EXECUTABLE_SHA256),
        probe(AMD64, c"Ace7Game", EXECUTABLE_SHA256),
        probe(AMD64, EXECUTABLE, c"c7da97f5"),
        probe(AMD64, EXECUTABLE, c""),
    ] {
        assert_eq!(unsafe { detect(&refused) }, UNKNOWN);
    }
    let mut short = accepted;
    short.struct_size -= 1;
    assert_eq!(unsafe { detect(&short) }, UNKNOWN);
    let mut unnamed = accepted;
    unnamed.executable_name_utf8 = null();
    assert_eq!(unsafe { detect(&unnamed) }, UNKNOWN);
    let mut unhashed = accepted;
    unhashed.sha256_hex = null();
    assert_eq!(unsafe { detect(&unhashed) }, UNKNOWN);
    assert_eq!(unsafe { detect(null()) }, UNKNOWN);
}

#[test]
fn validation_orders_size_before_abi() {
    let mut args = rsf_game_start_args {
        struct_size: size_of::<rsf_game_start_args>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
    };
    assert_eq!(unsafe { validate(&args) }, RSF_OK);
    args.abi_version += 1;
    assert_eq!(unsafe { validate(&args) }, RSF_ERROR_ABI_MISMATCH);
    args.struct_size -= 1;
    assert_eq!(unsafe { validate(&args) }, RSF_ERROR_INVALID_ARGUMENT);
    assert_eq!(
        unsafe { validate(null::<rsf_game_start_args>()) },
        RSF_ERROR_INVALID_ARGUMENT
    );
}

#[test]
fn publication_refuses_before_retaining_the_table() {
    let empty = rsf_game_info {
        id: null(),
        name: null(),
        version: null(),
        rendering_ready: 0,
        status: null(),
    };
    let mut api = rsf_game_plugin_api {
        struct_size: 0,
        abi_version: 0,
        info: empty,
        detect: None,
        hooks: rsf_game_hooks_api {
            prepare: None,
            start: None,
            quiesce: None,
            stop: None,
            status: None,
        },
    };
    assert_eq!(
        unsafe { rsf_ac7_publish_api(RSF_GAME_ABI_VERSION, null_mut()) },
        RSF_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_ac7_publish_api(RSF_GAME_ABI_VERSION, &mut api) },
        RSF_ERROR_INVALID_ARGUMENT
    );
    api.struct_size = size_of::<rsf_game_plugin_api>() as u32;
    assert_eq!(
        unsafe { rsf_ac7_publish_api(RSF_GAME_ABI_VERSION + 1, &mut api) },
        RSF_ERROR_ABI_MISMATCH
    );
    assert!(api.detect.is_none());
}

#[test]
fn identity_matches_the_former_plugin() {
    assert_eq!(ID, c"ac7");
    assert_eq!(NAME, c"Ace Combat 7: Skies Unknown");
    assert_eq!(
        STATUS,
        c"Native SR graph and view producer built and synthetic-tested. Game validation is pending."
    );
    assert!(VERSION.ends_with('\0') && VERSION.len() > 1);
}
