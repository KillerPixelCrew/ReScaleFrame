// SPDX-License-Identifier: MIT
use super::{ABI_MISMATCH, INVALID_ARGUMENT, Module, NATIVE_REFUSED, NOT_READY, OK, run};
use crate::sys::{RSF_ANOMALY_OBSERVATION_ABI, rsf_anomaly_native_observation as Observation};
use rsf_game_sys::*;
use std::ffi::CStr;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;

type Entry = unsafe extern "C" fn(u32, *mut rsf_game_plugin_api) -> i32;

fn entry(module: &Module) -> Entry {
    let name = CStr::from_bytes_with_nul(RSF_GAME_ENTRY_POINT).expect("GameSDK export name");
    unsafe { module.export(name) }.expect("Anomaly GameSDK export")
}

fn plugin(module: &Module) {
    for (ordinal, name) in [
        (1, c"rsf_anomaly_observation"),
        (2, c"rsf_anomaly_start"),
        (3, c"rsf_get_game_plugin_api"),
    ] {
        let named = unsafe { GetProcAddress(module.0, name.as_ptr().cast()) };
        // GetProcAddress interprets a low-word argument as an export ordinal.
        let by_ordinal = unsafe { GetProcAddress(module.0, std::ptr::without_provenance(ordinal)) };
        assert!(named.is_some(), "missing export {name:?}");
        assert_eq!(named.map(|f| f as usize), by_ordinal.map(|f| f as usize));
    }
    let query = entry(module);
    let version = RSF_GAME_ABI_VERSION;
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { query(version, null_mut()) }, INVALID_ARGUMENT);
    assert_eq!(unsafe { query(version, &mut api) }, INVALID_ARGUMENT);
    api.struct_size = size_of_val(&api) as u32;
    assert_eq!(unsafe { query(version + 1, &mut api) }, ABI_MISMATCH);
    assert_eq!(unsafe { query(version, &mut api) }, OK);
    assert!(!api.info.id.is_null());
    assert_eq!(unsafe { CStr::from_ptr(api.info.id) }, c"anomaly");
    assert_eq!(api.info.rendering_ready, 0);
    let detect = api.detect.expect("Anomaly detection callback");

    let standard = c"AnomalyDX11.exe";
    let avx = c"AnomalyDX11AVX.exe";
    let standard_hash = c"c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e";
    let avx_hash = c"2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12";
    let mut probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: 0x8664,
        reserved: 0,
        executable_name_utf8: avx.as_ptr(),
        sha256_hex: avx_hash.as_ptr(),
    };
    for (name, hash, expected) in [
        (standard, standard_hash, 1),
        (avx, standard_hash, 0),
        (avx, avx_hash, 1),
        (standard, avx_hash, 0),
        (
            c"ANOMALYDX11AVX.EXE",
            c"2032ACFC041E215508692EF4079EF504C7BBABF1793D5668084099F734B92F12",
            1,
        ),
        (c"AnomalyDX10AVX.exe", avx_hash, 0),
        (c"VerifiedDX11.exe", avx_hash, 0),
    ] {
        probe.executable_name_utf8 = name.as_ptr();
        probe.sha256_hex = hash.as_ptr();
        assert_eq!(unsafe { detect(&probe) }, expected, "{name:?}, {hash:?}");
    }
    probe.executable_name_utf8 = avx.as_ptr();
    probe.pe_machine = 0x14c;
    assert_eq!(unsafe { detect(&probe) }, 0, "x86 probe");
    probe.pe_machine = 0x8664;
    probe.struct_size -= 1;
    assert_eq!(unsafe { detect(&probe) }, 0, "short probe");
    probe.struct_size += 1;
    probe.sha256_hex = c"2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f1z".as_ptr();
    assert_eq!(unsafe { detect(&probe) }, 0, "malformed digest");
    probe.sha256_hex = null();
    assert_eq!(unsafe { detect(&probe) }, 0, "null digest");
    assert_eq!(unsafe { detect(null()) }, 0, "null probe");
    println!("Anomaly exact-build contract: PASS");
}

fn lifecycle(module: &Module) {
    let query = entry(module);
    type Observe = unsafe extern "C" fn(*mut Observation) -> i32;
    let observe: Observe =
        unsafe { module.export(c"rsf_anomaly_observation") }.expect("Anomaly observation export");
    let version = RSF_GAME_ABI_VERSION;
    let mut api: rsf_game_plugin_api = unsafe { std::mem::zeroed() };
    api.struct_size = size_of_val(&api) as u32;
    assert_eq!(unsafe { query(version, &mut api) }, OK);
    let hooks = api.hooks;
    let prepare = hooks.prepare.expect("prepare");
    let start = hooks.start.expect("start");
    let status = hooks.status.expect("status");
    let quiesce = hooks.quiesce.expect("quiesce");
    let stop = hooks.stop.expect("stop");

    assert_eq!(unsafe { observe(null_mut()) }, INVALID_ARGUMENT);
    let mut observation = Observation {
        struct_size: size_of::<Observation>() as u32 - 1,
        abi_version: RSF_ANOMALY_OBSERVATION_ABI,
        ..Default::default()
    };
    assert_eq!(unsafe { observe(&mut observation) }, INVALID_ARGUMENT);
    observation.struct_size += 1;
    observation.abi_version += 1;
    assert_eq!(unsafe { observe(&mut observation) }, ABI_MISMATCH);
    observation.abi_version -= 1;
    assert_eq!(unsafe { observe(&mut observation) }, OK);
    assert_eq!(observation.active, 0);
    assert_eq!(observation.render_frames, 0);
    assert_eq!(observation.resets, 0);
    assert_eq!(observation.camera_finite, 0);

    assert_eq!(unsafe { prepare(null()) }, INVALID_ARGUMENT);
    assert_eq!(unsafe { start(null()) }, INVALID_ARGUMENT);
    assert_eq!(unsafe { status(null_mut()) }, INVALID_ARGUMENT);
    let mut renderer = rsf_game_renderer_status {
        struct_size: size_of::<rsf_game_renderer_status>() as u32,
        abi_version: version,
        prepared: 0,
        active: 0,
        rendering_ready: 0,
        reason: null(),
    };
    assert_eq!(unsafe { status(&mut renderer) }, OK);
    assert_eq!(
        (renderer.active, renderer.prepared, renderer.rendering_ready),
        (0, 0, 0)
    );
    assert!(!renderer.reason.is_null());
    let start_args = rsf_game_start_args {
        struct_size: size_of::<rsf_game_start_args>() as u32,
        abi_version: version,
    };
    assert_eq!(unsafe { start(&start_args) }, NOT_READY);
    let host = rsf_game_sys::session::host_services(0, None);
    let mut prepare_args = rsf_game_prepare_args {
        struct_size: size_of::<rsf_game_prepare_args>() as u32,
        abi_version: version,
        host: &raw const host,
    };
    assert_eq!(unsafe { prepare(&prepare_args) }, NATIVE_REFUSED);
    assert_eq!(unsafe { status(&mut renderer) }, OK);
    assert_eq!((renderer.active, renderer.prepared), (0, 0));
    prepare_args.abi_version += 1;
    assert_eq!(unsafe { prepare(&prepare_args) }, ABI_MISMATCH);
    prepare_args.abi_version -= 1;
    let mut short_host = host;
    short_host.struct_size -= 1;
    prepare_args.host = &raw const short_host;
    assert_eq!(unsafe { prepare(&prepare_args) }, INVALID_ARGUMENT);
    let control = rsf_game_control_args {
        struct_size: size_of::<rsf_game_control_args>() as u32,
        abi_version: version,
    };
    assert_eq!(unsafe { quiesce(&control) }, OK);
    assert_eq!(unsafe { stop(&control) }, OK);
    println!("Anomaly foreign-process admission: PASS");
}

fn dll_contract(check: fn(&Module)) -> i32 {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next().filter(|_| args.next().is_none()) else {
        return 2;
    };
    run(|| {
        check(&Module::beside_executable(Path::new(&path)));
        0
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_anomaly_plugin() -> i32 {
    dll_contract(plugin)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_anomaly_lifecycle() -> i32 {
    dll_contract(lifecycle)
}
