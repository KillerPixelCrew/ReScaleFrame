// SPDX-License-Identifier: MIT
use super::*;
use std::ptr::dangling_mut;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

unsafe extern "C" fn log(_: Handle, message: *const c_char) {
    eprintln!(
        "[dlss] {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

fn setup(path: &CStr) -> Setup {
    let mut setup: Setup = unsafe { std::mem::zeroed() };
    setup.struct_size = size_of::<Setup>() as u32;
    setup.abi_version = RSF_DLSS_ABI_VERSION;
    setup.interposer_path_utf8 = path.as_ptr();
    setup.engine = 1; // Unreal metadata used in the original fixture.
    setup.engine_version_utf8 = c"4.18".as_ptr();
    setup.project_id_utf8 = c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr();
    setup.log = Some(log);
    setup
}

fn support() -> Support {
    let mut support: Support = unsafe { std::mem::zeroed() };
    support.struct_size = size_of::<Support>() as u32;
    support
}

fn device_query(directory: &str) -> bool {
    let Ok(path) = CString::new(format!("{directory}\\sl.interposer.dll")) else {
        return false;
    };
    let Ok(directory) = CString::new(directory) else {
        return false;
    };
    let mut setup = setup(&path);
    setup.plugin_directory_utf8 = directory.as_ptr();
    let loaded = unsafe { rsf_dlss_load(&setup) };
    if loaded != OK {
        eprintln!("streamline did not initialise here (result {loaded}), skipping");
        return true;
    }
    let mut device = None;
    let created = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    };
    let Some(device) = device.filter(|_| created.is_ok()) else {
        eprintln!("no hardware device available, skipping the support query");
        unsafe { rsf_dlss_shutdown() };
        return true;
    };
    let mut passed = unsafe { rsf_dlss_set_device(device.as_raw()) } == OK;
    let mut support = support();
    let queried = unsafe { rsf_dlss_query_support(&mut support) };
    eprintln!(
        "DLSS supported: {} (result {queried}), driver {}.{} requires {}.{}",
        support.supported,
        support.detected_driver_major,
        support.detected_driver_minor,
        support.required_driver_major,
        support.required_driver_minor
    );
    if queried == OK {
        let mut plan: Plan = unsafe { std::mem::zeroed() };
        plan.struct_size = size_of::<Plan>() as u32;
        plan.output_width = 2048;
        plan.output_height = 1152;
        plan.quality = 3; // Performance
        passed &= unsafe { rsf_dlss_plan_render_size(&mut plan) } == OK;
        passed &= plan.render_width > 0 && plan.render_width < plan.output_width;
        eprintln!(
            "performance quality renders {}x{} for 2048x1152",
            plan.render_width, plan.render_height
        );
    }
    passed & (unsafe { rsf_dlss_shutdown() } == OK)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_backend() -> i32 {
    let compiled = rsf_dlss_available() != 0;
    eprintln!("streamline headers compiled in: {compiled}");
    let mut passed = true;
    let mut check = |condition: bool, message: &str| {
        if !condition {
            eprintln!("{message}");
            passed = false;
        }
    };
    let mut setup = setup(c"Z:\\definitely\\not\\here\\sl.interposer.dll");
    let size = setup.struct_size;
    setup.abi_version += 1;
    check(
        unsafe { rsf_dlss_load(&setup) } == ABI_MISMATCH,
        "Incompatible setup ABI",
    );
    setup.abi_version -= 1;
    setup.struct_size = 0;
    check(unsafe { rsf_dlss_load(&setup) } == INVALID, "Short setup");
    setup.struct_size = size;
    check(unsafe { rsf_dlss_load(null()) } == INVALID, "Null setup");
    check(
        unsafe { rsf_dlss_set_device(null_mut()) } == INVALID,
        "Null device",
    );
    check(
        unsafe { rsf_dlss_query_support(&mut support()) }
            == if compiled { NOT_READY } else { NOT_COMPILED },
        "Support queried before load",
    );
    let sentinel = dangling_mut::<u8>().cast();
    let mut frame: Frame = unsafe { std::mem::zeroed() };
    frame.struct_size = size_of::<Frame>() as u32;
    frame.abi_version = RSF_DLSS_ABI_VERSION;
    check(
        unsafe { rsf_dlss_evaluate(null_mut(), &frame) } == INVALID,
        "Null context",
    );
    frame.abi_version += 1;
    check(
        unsafe { rsf_dlss_evaluate(sentinel, &frame) } == ABI_MISMATCH,
        "Incompatible frame ABI",
    );
    frame.abi_version -= 1;
    if compiled {
        check(
            unsafe { rsf_dlss_evaluate(sentinel, &frame) } == INVALID,
            "Missing required resources",
        );
        frame.color_in = sentinel;
        frame.color_out = sentinel;
        frame.depth = sentinel;
        frame.motion = sentinel;
        for viewport in [2, 7] {
            frame.viewport = viewport;
            check(
                unsafe { rsf_dlss_evaluate(sentinel, &frame) } == INVALID
                    && unsafe { rsf_dlss_release_viewport(viewport) } == INVALID,
                "Unsupported viewport",
            );
        }
        check(
            unsafe { rsf_dlss_load(&setup) } == LOAD_FAILED,
            "Missing interposer",
        );
        setup.require_signature = 1;
        check(
            unsafe { rsf_dlss_load(&setup) } == LOAD_FAILED,
            "Unverifiable signature",
        );
    } else {
        check(
            unsafe { rsf_dlss_load(&setup) } == NOT_COMPILED,
            "SDK-free load",
        );
    }
    if compiled
        && let Ok(directory) = std::env::var("RSF_STREAMLINE_BIN")
        && !directory.is_empty()
    {
        passed &= device_query(&directory);
    }
    i32::from(!passed)
}
