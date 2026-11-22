// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::{
    RSF_ERROR_ABI_MISMATCH as ABI_MISMATCH, RSF_GAME_ABI_VERSION,
    RSF_GAME_RECOGNIZED as RECOGNIZED, RSF_GAME_UNKNOWN as UNKNOWN, rsf_game_host_services,
};
use std::ptr;

fn probe(executable: &CStr, sha256: &CStr) -> rsf_game_probe {
    rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: layout::AMD64,
        reserved: 0,
        executable_name_utf8: executable.as_ptr(),
        sha256_hex: sha256.as_ptr(),
    }
}

const SHA: &CStr = c"B56C9548F670654A9B73BF25DEF3CD73AF12E269F6E47DBA28A34079ADAF465E";

#[test]
fn detection_matches_only_the_fingerprinted_executable() {
    let mut value = probe(c"rdr2.EXE", SHA);
    assert_eq!(unsafe { detect(&value) }, RECOGNIZED);
    value.pe_machine = 0x14c;
    assert_eq!(unsafe { detect(&value) }, UNKNOWN);
    value.pe_machine = layout::AMD64;
    value.sha256_hex = c"unknown".as_ptr();
    assert_eq!(unsafe { detect(&value) }, UNKNOWN);
    value.sha256_hex = SHA.as_ptr();
    value.executable_name_utf8 = ptr::null();
    assert_eq!(unsafe { detect(&value) }, UNKNOWN);
    value = probe(c"RDR2.exe", SHA);
    value.struct_size -= 1;
    assert_eq!(unsafe { detect(&value) }, UNKNOWN);
    assert_eq!(unsafe { detect(ptr::null()) }, UNKNOWN);
}

fn published() -> rsf_game_plugin_api {
    // The API is plain data with optional callbacks; zero is a valid empty value.
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    api.struct_size = size_of::<rsf_game_plugin_api>() as u32;
    assert_eq!(
        unsafe { rsf_get_game_plugin_api(RSF_GAME_ABI_VERSION, &mut api) },
        OK
    );
    api
}

#[test]
fn publication_checks_size_and_abi() {
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { rsf_get_game_plugin_api(RSF_GAME_ABI_VERSION, &mut api) },
        INVALID_ARGUMENT
    );
    api.struct_size = size_of::<rsf_game_plugin_api>() as u32;
    assert_eq!(
        unsafe { rsf_get_game_plugin_api(RSF_GAME_ABI_VERSION + 1, &mut api) },
        ABI_MISMATCH
    );
    let api = published();
    assert_eq!(api.info.rendering_ready, 0);
    assert_eq!(unsafe { CStr::from_ptr(api.info.id) }, c"rdr2");
    assert_eq!(
        unsafe { CStr::from_ptr(api.info.version) }.to_bytes(),
        env!("CARGO_PKG_VERSION").as_bytes()
    );
}

// The callee reads these records through the raw pointer it was given.
#[allow(unused_assignments)]
#[test]
fn lifecycle_refuses_before_validated_native_preparation() {
    let api = published();
    let hooks = api.hooks;
    let (prepare, start, quiesce, stop, status) = (
        hooks.prepare.unwrap(),
        hooks.start.unwrap(),
        hooks.quiesce.unwrap(),
        hooks.stop.unwrap(),
        hooks.status.unwrap(),
    );
    let mut renderer: rsf_game_renderer_status = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { status(&mut renderer) }, INVALID_ARGUMENT);
    renderer.struct_size = size_of::<rsf_game_renderer_status>() as u32;
    renderer.abi_version = RSF_GAME_ABI_VERSION;
    renderer.prepared = 1;
    renderer.active = 1;
    assert_eq!(unsafe { status(&mut renderer) }, OK);
    assert_eq!(
        (renderer.prepared, renderer.active, renderer.rendering_ready),
        (0, 0, 0)
    );
    assert!(!renderer.reason.is_null());

    let start_args = rsf_game_start_args {
        struct_size: size_of::<rsf_game_start_args>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
    };
    assert_eq!(unsafe { start(&start_args) }, NOT_READY);
    // The contract fixture's host: no upscale, submit or Present services.
    let mut services: rsf_game_host_services = unsafe { std::mem::zeroed() };
    services.struct_size = size_of::<rsf_game_host_services>() as u32;
    services.abi_version = RSF_GAME_ABI_VERSION;
    services.session_id = 9;
    let mut prepare_args = rsf_game_prepare_args {
        struct_size: size_of::<rsf_game_prepare_args>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        host: &services,
    };
    assert_eq!(unsafe { prepare(&prepare_args) }, NOT_READY);
    services.session_id = 0;
    assert_eq!(unsafe { prepare(&prepare_args) }, INVALID_ARGUMENT);
    services.abi_version += 1;
    assert_eq!(unsafe { prepare(&prepare_args) }, ABI_MISMATCH);
    prepare_args.host = ptr::null();
    assert_eq!(unsafe { prepare(&prepare_args) }, INVALID_ARGUMENT);
    assert_eq!(unsafe { prepare(ptr::null()) }, INVALID_ARGUMENT);
    let control = rsf_game_control_args {
        struct_size: size_of::<rsf_game_control_args>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
    };
    assert_eq!(unsafe { quiesce(&control) }, OK);
    assert_eq!(unsafe { stop(&control) }, OK);
    let short = rsf_game_control_args {
        struct_size: size_of::<rsf_game_control_args>() as u32 - 1,
        abi_version: RSF_GAME_ABI_VERSION,
    };
    assert_eq!(unsafe { stop(&short) }, INVALID_ARGUMENT);
}
