// SPDX-License-Identifier: MIT
use super::wide;
use crate::sys::*;
use rsf_game_sys::{
    RSF_CALLBACK_HOST_ABI_VERSION, RSF_CALLBACK_HOST_ABI_VERSION_V2, rsf_callback_host_setup,
    rsf_callback_host_setup_v2,
};
use rsf_game_sys::{
    RSF_ERROR_ABI_MISMATCH, RSF_ERROR_INVALID_ARGUMENT, RSF_ERROR_NOT_READY, RSF_OK,
};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, RSF_GAME_LATENCY_MODE, RSF_GAME_RENDER_FEEDBACK_CAMERA_JITTER_INVALID,
    rsf_game_host_services, rsf_game_latency_event, rsf_game_probe, rsf_game_render_feedback,
};
use rsf_game_sys::{RSF_RENDER_HOST_ABI_VERSION, rsf_render_host_setup};
use std::cell::Cell;
use std::ffi::CStr;
use std::fs;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW, WritePrivateProfileStringW,
};
use windows_sys::core::w;

mod window;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rsf-{name}-{}", std::process::id()));
        fs::create_dir(&path).expect("create fixture scratch directory");
        Self(path)
    }

    fn path(&self, leaf: &str) -> Vec<u16> {
        wide(self.0.join(leaf).as_os_str())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for leaf in ["host.log", "preferences.ini", "ReScaleFrame.ini"] {
            match fs::remove_file(self.0.join(leaf)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("remove fixture file {leaf}: {error}"),
            }
        }
        fs::remove_dir(&self.0).expect("remove fixture scratch directory");
    }
}

thread_local! {
    static POLICY: Cell<(i32, u32, u32, u32, u32)> = const { Cell::new((1, 0, 99, 99, 99)) };
}

unsafe extern "C" fn policy(spatial: u32, quality: u32) -> i32 {
    let (result, calls, _, _, _) = POLICY.get();
    let stop = unsafe { rsf_callback_host_stop() };
    POLICY.set((result, calls + 1, spatial, quality, stop));
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_callback_host_abi() -> i32 {
    let scratch = Scratch::new("callback-host-abi");
    let log = scratch.path("host.log");
    let preferences = scratch.path("preferences.ini");
    let missing_plugin = scratch.path("missing-plugin.dll");
    let unwritable = scratch.path("missing-directory/host.log");
    let relative = rsf_game_sys::ffi_util::wide("Adapter.dll");
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: 0x8664,
        executable_name_utf8: c"Game.exe".as_ptr(),
        sha256_hex: c"00".as_ptr(),
        reserved: 0,
    };
    let valid = rsf_callback_host_setup {
        struct_size: size_of::<rsf_callback_host_setup>() as u32,
        abi_version: RSF_CALLBACK_HOST_ABI_VERSION,
        plugin_path: missing_plugin.as_ptr(),
        log_path: log.as_ptr(),
        preferences_path: preferences.as_ptr(),
        probe: &probe,
        backend: RSF_SR_AUTO,
        quality: 2,
        dlss_preset: 0,
        sr_backend_choices: (1 << RSF_SR_DLSS) | (1 << RSF_SR_FSR3) | (1 << RSF_SR_SPATIAL),
        engine_type: 0,
        engine_version_utf8: c"fixture".as_ptr(),
        project_id_utf8: c"fixture".as_ptr(),
        streamline_directory_utf8: null(),
        fsr2_directory_utf8: null(),
        fsr3_directory_utf8: null(),
        fsr4_directory_utf8: null(),
        xess_directory_utf8: null(),
        set_engine_spatial: Some(policy),
    };
    let start = |setup: &rsf_callback_host_setup| unsafe { rsf_callback_host_start(setup) };
    assert_eq!(unsafe { rsf_callback_host_stop() }, 0);
    assert_eq!(unsafe { rsf_callback_host_start(null()) }, 1);
    assert_eq!(
        start(&rsf_callback_host_setup {
            struct_size: valid.struct_size - 8,
            ..valid
        }),
        1
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            abi_version: RSF_CALLBACK_HOST_ABI_VERSION_V2,
            ..valid
        }),
        1
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            abi_version: RSF_CALLBACK_HOST_ABI_VERSION_V2 + 1,
            ..valid
        }),
        1
    );
    let v2 = rsf_callback_host_setup_v2 {
        base: rsf_callback_host_setup {
            struct_size: size_of::<rsf_callback_host_setup_v2>() as u32,
            abi_version: RSF_CALLBACK_HOST_ABI_VERSION_V2,
            ..valid
        },
        request_policy: None,
        drained_generation: None,
        spatial_extent: None,
        spatial_name_utf8: null(),
    };
    assert_eq!(start(&v2.base), 1);
    assert_eq!(
        start(&rsf_callback_host_setup {
            probe: null(),
            ..valid
        }),
        1
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            set_engine_spatial: None,
            ..valid
        }),
        1
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            sr_backend_choices: 0,
            ..valid
        }),
        1
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            dlss_preset: 0x100,
            ..valid
        }),
        2
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            backend: RSF_SR_AUTO + 1,
            ..valid
        }),
        2
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            backend: RSF_SR_DLSS,
            quality: 5,
            ..valid
        }),
        2
    );
    assert_eq!(POLICY.get().1, 0);
    assert_eq!(
        start(&rsf_callback_host_setup {
            log_path: unwritable.as_ptr(),
            ..valid
        }),
        4
    );
    assert_eq!(POLICY.get().1, 0);

    POLICY.set((0, 0, 99, 99, 99));
    assert_eq!(start(&valid), 8);
    assert_eq!(
        POLICY.get(),
        (0, 1, 0, 2, 1),
        "Auto policy and reentrant stop"
    );
    POLICY.set((1, 1, 0, 2, 1));
    assert_eq!(
        start(&rsf_callback_host_setup {
            backend: RSF_SR_SPATIAL,
            quality: 5,
            ..valid
        }),
        10
    );
    assert_eq!(
        POLICY.get(),
        (1, 2, 1, 5, 1),
        "spatial policy and missing plugin"
    );
    assert_eq!(
        start(&rsf_callback_host_setup {
            plugin_path: relative.as_ptr(),
            ..valid
        }),
        10
    );
    assert_eq!(unsafe { rsf_callback_host_stop() }, 0);
    let text = fs::read_to_string(scratch.0.join("host.log")).expect("read callback host log");
    assert!(text.contains("Native renderer plugin preparation/activation refused.\r\n"));
    0
}

fn frontend_settings() -> rsf_render_settings {
    let mut settings = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    assert_eq!(unsafe { rsf_render_settings_get(&mut settings) }, RSF_OK);
    settings
}

fn apply(settings: &rsf_render_settings) {
    assert_eq!(unsafe { rsf_render_settings_apply(settings) }, RSF_OK);
}

fn sharpening_words(settings: &rsf_render_settings) -> [u32; 12] {
    [
        settings.sharpening_mode,
        settings.sharpening_sharpness.to_bits(),
        settings.sharpening_mas,
        settings.sharpening_motion_sharpness.to_bits(),
        settings.sharpening_motion_threshold.to_bits(),
        settings.sharpening_motion_scale_limit.to_bits(),
        settings.sharpening_contrast_enabled,
        settings.sharpening_contrast.to_bits(),
        settings.sharpening_depth_scale.to_bits(),
        settings.sharpening_depth_bias.to_bits(),
        settings.sharpening_clamp_output,
        settings.sharpening_debug,
    ]
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_render_frontend() -> i32 {
    let mut status = rsf_render_status {
        struct_size: size_of::<rsf_render_status>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    assert_eq!(
        unsafe { rsf_render_status_get(null_mut()) },
        RSF_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { rsf_render_status_get(&mut status) },
        RSF_ERROR_NOT_READY
    );
    let scratch = Scratch::new("render-frontend");
    let directory = wide(scratch.0.as_os_str());
    let ini = scratch.path("ReScaleFrame.ini");
    let section = w!("Fixture");
    let ini_value = |key| unsafe { GetPrivateProfileIntW(section, key, 0, ini.as_ptr()) };
    for (key, value) in [
        (w!("ForceD3D12"), w!("1")),
        (w!("UnrelatedSetting"), w!("preserved")),
    ] {
        assert_ne!(
            unsafe { WritePrivateProfileStringW(section, key, value, ini.as_ptr()) },
            0
        );
    }
    let mut host = rsf_game_host_services {
        struct_size: size_of::<rsf_game_host_services>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        session_id: 7,
        // The remaining ABI fields are zero values and optional callbacks.
        ..Default::default()
    };
    let setup = rsf_render_host_setup {
        struct_size: size_of::<rsf_render_host_setup>() as u32,
        abi_version: RSF_RENDER_HOST_ABI_VERSION,
        configuration_path: ini.as_ptr(),
        runtime_directory: directory.as_ptr(),
        configuration_section: section,
        // Auto is a choice bit like every provider (RSF_SR_CHOICES_ALL); this host limits the rest.
        sr_backend_choices: (1 << RSF_SR_DLSS)
            | (1 << RSF_SR_FSR3)
            | (1 << RSF_SR_FSR4)
            | (1 << RSF_SR_XESS)
            | (1 << RSF_SR_AUTO),
        engine_version_utf8: c"Fixture".as_ptr(),
        units_to_meters: 1.0,
        depth_inverted: 1,
        ..Default::default()
    };
    assert_eq!(unsafe { rsf_render_host_create(&setup, &mut host) }, RSF_OK);
    let latency = rsf_game_latency_event {
        struct_size: size_of::<rsf_game_latency_event>() as u32,
        marker: RSF_GAME_LATENCY_MODE,
        session_id: host.session_id,
        source_frame_id: 0,
        timestamp_qpc: 0,
        qpc_frequency: 0,
    };
    assert_eq!(
        unsafe { host.process_latency.expect("latency callback")(host.user, &latency) },
        RSF_OK
    );
    let settings = frontend_settings();
    assert_eq!(settings.sr_backend, RSF_SR_AUTO);
    assert_ne!(settings.status_overlay, 0);
    apply(&settings);
    assert_eq!(ini_value(w!("SRBackend")), RSF_SR_AUTO as i32);

    apply(&rsf_render_settings {
        sr_backend: RSF_SR_FSR3,
        fg_enabled: 1,
        fg_backend: 1,
        generated_frames: 3,
        latency: 2,
        sr_quality: 4,
        ..settings
    });
    let saved = frontend_settings();
    assert_eq!(
        (
            saved.sr_backend,
            saved.generated_frames,
            saved.latency,
            saved.sr_quality
        ),
        (RSF_SR_FSR3, 3, 2, 4)
    );
    for mode in 0..=4 {
        let sharp = rsf_render_settings {
            sharpening_mode: mode,
            sharpening_sharpness: 0.63,
            sharpening_mas: 1,
            sharpening_motion_sharpness: -0.3,
            sharpening_motion_threshold: 3.0,
            sharpening_motion_scale_limit: 20.0,
            sharpening_contrast_enabled: 1,
            sharpening_contrast: 1.1,
            sharpening_depth_scale: 87.0,
            sharpening_depth_bias: 0.004,
            sharpening_clamp_output: 1,
            sharpening_debug: 1,
            ..saved
        };
        apply(&sharp);
        assert_eq!(
            sharpening_words(&frontend_settings()),
            sharpening_words(&sharp)
        );
    }
    assert_eq!(
        unsafe {
            rsf_render_settings_apply(&rsf_render_settings {
                sharpening_sharpness: 9.0,
                ..saved
            })
        },
        RSF_ERROR_INVALID_ARGUMENT
    );
    apply(&rsf_render_settings {
        fg_enabled: 0,
        ..saved
    });
    assert_eq!(unsafe { rsf_render_status_get(&mut status) }, RSF_OK);
    assert_eq!(
        (
            status.fg_requested,
            status.fg_requested_backend,
            status.fg_effective_backend
        ),
        (0, 0, 0)
    );
    let remembered = frontend_settings();
    assert_eq!(
        (remembered.fg_backend, remembered.fg_enabled),
        (saved.fg_backend, 0)
    );
    apply(&saved);

    assert_eq!(
        unsafe {
            rsf_render_settings_apply(&rsf_render_settings {
                sr_quality: 5,
                ..saved
            })
        },
        RSF_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(ini_value(w!("SRQuality")), 4);
    let mut untouched = [0u16; 64];
    let len = unsafe {
        GetPrivateProfileStringW(
            section,
            w!("UnrelatedSetting"),
            w!(""),
            untouched.as_mut_ptr(),
            untouched.len() as u32,
            ini.as_ptr(),
        )
    };
    assert_eq!(
        String::from_utf16_lossy(&untouched[..len as usize]),
        "preserved"
    );
    assert_eq!(ini_value(w!("ForceD3D12")), 1);
    assert_eq!(
        unsafe {
            rsf_render_settings_apply(&rsf_render_settings {
                generated_frames: 0,
                ..settings
            })
        },
        RSF_ERROR_INVALID_ARGUMENT
    );
    assert_eq!(ini_value(w!("FGGeneratedFrames")), 3);

    let feedback = rsf_game_render_feedback {
        struct_size: size_of::<rsf_game_render_feedback>() as u32,
        reason: RSF_GAME_RENDER_FEEDBACK_CAMERA_JITTER_INVALID,
        session_id: host.session_id,
        native_frame: 123,
    };
    unsafe { host.render_feedback.expect("feedback callback")(host.user, &feedback) };
    assert_eq!(unsafe { rsf_render_status_get(&mut status) }, RSF_OK);
    assert_eq!(status.native_frame, 123);
    assert_eq!(
        unsafe { CStr::from_ptr(status.reason.as_ptr()) },
        c"Invalid camera or frozen jitter"
    );
    assert_eq!(status.output_fps_valid, 0);
    status.abi_version += 1;
    assert_eq!(
        unsafe { rsf_render_status_get(&mut status) },
        RSF_ERROR_ABI_MISMATCH
    );
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_render_links() -> i32 {
    let mut links = null_mut();
    let output = rsf_render_link {
        struct_size: size_of::<rsf_render_link>() as u32,
        abi_version: RSF_RENDER_LINK_ABI_VERSION,
        ..Default::default()
    };
    let (mut ticket, mut consumed) = (output, output);
    let source = (1 << 32) + 1;
    unsafe {
        assert_eq!(rsf_render_links_create(42, 2, &mut links), RSF_OK);
        assert!(!links.is_null());
        let mut short = rsf_render_link {
            struct_size: output.struct_size - 1,
            ..output
        };
        assert_eq!(
            rsf_render_links_lookup(links, 100, 8, &mut short),
            RSF_ERROR_INVALID_ARGUMENT
        );
        let mut future = rsf_render_link {
            abi_version: output.abi_version + 1,
            ..output
        };
        assert_eq!(
            rsf_render_links_bind(links, 100, source, 8, &mut future),
            RSF_ERROR_ABI_MISMATCH
        );
        assert_eq!(
            rsf_render_links_release(links, &future),
            RSF_ERROR_ABI_MISMATCH
        );
        assert_eq!(
            rsf_render_links_bind(links, 100, source, 8, &mut ticket),
            RSF_OK
        );
        assert_eq!(
            rsf_render_links_lookup(links, 100, 8, &mut consumed),
            RSF_OK
        );
        assert_eq!(
            (
                consumed.session_id,
                consumed.source_frame_id,
                consumed.submission_id
            ),
            (42, source, ticket.submission_id)
        );
        assert_eq!(rsf_render_links_release(links, &ticket), RSF_OK);
        assert_eq!(rsf_render_links_destroy(links), RSF_OK);
    }
    0
}
