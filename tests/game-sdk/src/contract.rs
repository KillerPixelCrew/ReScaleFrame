// SPDX-License-Identifier: MIT
use super::{ABI_MISMATCH, INVALID_ARGUMENT, Module, NATIVE_REFUSED, NOT_READY, OK, run};
use rsf_game_sys::*;
use std::ffi::CStr;
use std::path::Path;
use std::ptr::null;

struct Fixture {
    id: &'static CStr,
    executable: &'static CStr,
    sha256: &'static CStr,
    prepare_result: i32,
    strict_lifecycle: bool,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        id: c"fallout4",
        executable: c"Fallout4.exe",
        sha256: c"fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8",
        prepare_result: NATIVE_REFUSED,
        strict_lifecycle: true,
    },
    Fixture {
        id: c"skyrim-se",
        executable: c"SkyrimSE.exe",
        sha256: c"846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f",
        prepare_result: NATIVE_REFUSED,
        strict_lifecycle: true,
    },
    Fixture {
        id: c"ac7",
        executable: c"Ace7Game.exe",
        sha256: c"c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f",
        prepare_result: NATIVE_REFUSED,
        strict_lifecycle: false,
    },
    Fixture {
        id: c"project-wingman",
        executable: c"ProjectWingman-Win64-Shipping.exe",
        sha256: c"092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a",
        prepare_result: NATIVE_REFUSED,
        strict_lifecycle: true,
    },
    Fixture {
        id: c"rdr2",
        executable: c"RDR2.exe",
        sha256: c"b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e",
        prepare_result: NOT_READY,
        strict_lifecycle: true,
    },
    Fixture {
        id: c"unity-mono",
        executable: c"DragNWash.exe",
        sha256: c"5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc",
        prepare_result: NOT_READY,
        strict_lifecycle: true,
    },
];

fn inactive(status: &rsf_game_renderer_status) -> bool {
    status.prepared == 0 && status.active == 0 && status.rendering_ready == 0
}

fn contract(fixture: &Fixture, module: &Module) {
    let name = CStr::from_bytes_with_nul(RSF_GAME_ENTRY_POINT).expect("GameSDK export name");
    type Entry = unsafe extern "C" fn(u32, *mut rsf_game_plugin_api) -> i32;
    let entry = unsafe { module.export::<Entry>(name) }.expect("plugin entry point");
    let version = RSF_GAME_ABI_VERSION;
    // Zero initializes the sized C records before setting their input fields.
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    unsafe {
        assert_eq!(entry(version, std::ptr::null_mut()), INVALID_ARGUMENT);
        assert_eq!(entry(version, &mut api), INVALID_ARGUMENT);
        api.struct_size = size_of_val(&api) as u32;
        assert_eq!(entry(version + 1, &mut api), ABI_MISMATCH);
        assert_eq!(entry(version, &mut api), OK);
    }

    let detect = api.detect.expect("detection callback");
    assert!(!api.info.id.is_null());
    assert_eq!(unsafe { CStr::from_ptr(api.info.id) }, fixture.id);
    assert!(!api.info.name.is_null() && !api.info.version.is_null() && !api.info.status.is_null());
    assert_eq!(api.info.rendering_ready, 0);
    let mut probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: 0x8664,
        reserved: 0,
        executable_name_utf8: fixture.executable.as_ptr(),
        sha256_hex: fixture.sha256.as_ptr(),
    };
    assert_eq!(
        unsafe { detect(&probe) },
        1,
        "exact fingerprint recognition"
    );

    let prepare = api.hooks.prepare.expect("prepare callback");
    let start = api.hooks.start.expect("start callback");
    let quiesce = api.hooks.quiesce.expect("quiesce callback");
    let stop = api.hooks.stop.expect("stop callback");
    let status = api.hooks.status.expect("status callback");
    let mut state: rsf_game_renderer_status = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { status(&mut state) }, INVALID_ARGUMENT);
    state.struct_size = size_of_val(&state) as u32;
    state.abi_version = version;
    let mut begin = rsf_game_start_args {
        struct_size: size_of::<rsf_game_start_args>() as u32,
        abi_version: version,
    };
    let mut host = rsf_game_sys::session::host_services(8, None);
    let host_ptr = &raw mut host;
    let mut prep = rsf_game_prepare_args {
        struct_size: size_of::<rsf_game_prepare_args>() as u32,
        abi_version: version,
        host: host_ptr,
    };
    let mut control = rsf_game_control_args {
        struct_size: size_of::<rsf_game_control_args>() as u32,
        abi_version: version,
    };
    unsafe {
        assert_eq!(status(&mut state), OK);
        assert!(inactive(&state));
        assert_eq!(start(&begin), NOT_READY);
        assert_eq!(prepare(&prep), fixture.prepare_result);
        assert_eq!(quiesce(&control), OK);
        assert_eq!(stop(&control), OK);
        assert_eq!(status(&mut state), OK);
        assert!(inactive(&state));
    }

    probe.pe_machine = 0x14c;
    assert_eq!(unsafe { detect(&probe) }, 0);
    probe.pe_machine = 0x8664;
    probe.struct_size -= 1;
    assert_eq!(unsafe { detect(&probe) }, 0);
    probe.struct_size += 1;
    probe.executable_name_utf8 = c"OtherGame.exe".as_ptr();
    assert_eq!(unsafe { detect(&probe) }, 0);
    probe.executable_name_utf8 = null();
    assert_eq!(unsafe { detect(&probe) }, 0);
    probe.executable_name_utf8 = fixture.executable.as_ptr();

    if fixture.strict_lifecycle {
        unsafe {
            assert_eq!(prepare(null()), INVALID_ARGUMENT);
            assert_eq!(start(null()), INVALID_ARGUMENT);
            assert_eq!(quiesce(null()), INVALID_ARGUMENT);
            assert_eq!(stop(null()), INVALID_ARGUMENT);
            assert_eq!(status(std::ptr::null_mut()), INVALID_ARGUMENT);

            prep.struct_size -= 1;
            begin.struct_size -= 1;
            control.struct_size -= 1;
            assert_eq!(prepare(&prep), INVALID_ARGUMENT);
            assert_eq!(start(&begin), INVALID_ARGUMENT);
            assert_eq!(quiesce(&control), INVALID_ARGUMENT);
            assert_eq!(stop(&control), INVALID_ARGUMENT);
            prep.struct_size += 1;
            begin.struct_size += 1;
            control.struct_size += 1;

            prep.abi_version += 1;
            begin.abi_version += 1;
            control.abi_version += 1;
            state.abi_version += 1;
            assert_eq!(prepare(&prep), ABI_MISMATCH);
            assert_eq!(start(&begin), ABI_MISMATCH);
            assert_eq!(quiesce(&control), ABI_MISMATCH);
            assert_eq!(stop(&control), ABI_MISMATCH);
            assert_eq!(status(&mut state), ABI_MISMATCH);
            prep.abi_version = version;
            begin.abi_version = version;
            control.abi_version = version;
            state.abi_version = version;

            prep.host = null();
            assert_eq!(prepare(&prep), INVALID_ARGUMENT);
            prep.host = host_ptr;
            (*host_ptr).struct_size -= 1;
            assert_eq!(prepare(&prep), INVALID_ARGUMENT);
            (*host_ptr).struct_size += 1;
            (*host_ptr).abi_version += 1;
            assert_eq!(prepare(&prep), ABI_MISMATCH);
            (*host_ptr).abi_version = version;
            (*host_ptr).session_id = 0;
            assert_eq!(prepare(&prep), INVALID_ARGUMENT);
            (*host_ptr).session_id = 9;
            assert_eq!(prepare(&prep), fixture.prepare_result);
            assert_eq!(start(&begin), NOT_READY);
            assert_eq!(quiesce(&control), OK);
            assert_eq!(stop(&control), OK);
            state.prepared = 1;
            state.active = 1;
            state.rendering_ready = 1;
            assert_eq!(status(&mut state), OK);
            assert!(inactive(&state) && !state.reason.is_null());
        }
    }
    probe.sha256_hex = c"unknown".as_ptr();
    assert_eq!(unsafe { detect(&probe) }, 0);
    probe.sha256_hex = null();
    assert_eq!(unsafe { detect(&probe) }, 0);
    assert_eq!(unsafe { detect(null()) }, 0);
}

/// The frame policy of rsf-game-sys, through the public C functions.
fn frame_policy() {
    let cases = [
        (RSF_SCREEN_FLIGHT, 0, true, true),
        (RSF_SCREEN_VIDEO, 0, false, false),
        (RSF_SCREEN_LOADING, 0, false, false),
        (RSF_SCREEN_MENU, 0, true, false),
        (RSF_SCREEN_UNKNOWN, 0, true, false),
        (RSF_SCREEN_FLIGHT, RSF_FRAME_FLAG_NO_FG, true, false),
        (RSF_SCREEN_FLIGHT, RSF_FRAME_FLAG_NO_SR, false, false),
    ];
    let mut record = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        ..Default::default()
    };
    for (screen, flags, sr, fg) in cases {
        record.screen = screen;
        record.flags = flags;
        let (allows_sr, allows_fg) = unsafe {
            (
                rsf_frame_allows_sr(&record) != 0,
                rsf_frame_allows_fg(&record) != 0,
            )
        };
        assert_eq!(
            (allows_sr, allows_fg),
            (sr, fg),
            "screen {screen} flags {flags}"
        );
    }
    // A record that advertises less than its full size allows nothing.
    record.flags = 0;
    record.struct_size = 8;
    assert!(unsafe { rsf_frame_allows_sr(&record) == 0 && rsf_frame_allows_fg(&record) == 0 });
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_plugin_contract() -> i32 {
    run(|| {
        let args: Vec<_> = std::env::args_os().collect();
        if args.len() != 3 {
            return 2;
        }
        let Some(fixture) = FIXTURES
            .iter()
            .find(|fixture| args[2] == fixture.id.to_str().expect("ASCII fixture ID"))
        else {
            return 2;
        };
        let module = Module::beside_executable(Path::new(&args[1]));
        contract(fixture, &module);
        frame_policy();
        OK
    })
}
