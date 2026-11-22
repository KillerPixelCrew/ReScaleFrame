// SPDX-License-Identifier: MIT
mod engine_sharpening_abi;

use engine_sharpening_abi::*;
use rsf_game_sys::{RSF_SRVK_ABI_VERSION, rsf_sharpening_options, rsf_srvk_frame};
use std::ffi::{CStr, OsStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;

const _: () = {
    assert!(RSF_GODOT_ABI_VERSION == 3);
    assert!(RSF_GODOT_SETTINGS_ABI_VERSION == 2);
    assert!(RSF_UNITY_SETTINGS_ABI_VERSION == 2);
    assert!(RSF_SRVK_ABI_VERSION == 2);
    assert!(size_of::<rsf_godot_settings>() == 100);
    assert!(size_of::<rsf_unity_package_settings>() == 100);
    assert!(std::mem::offset_of!(rsf_godot_settings, sharpening) == 44);
    assert!(std::mem::offset_of!(rsf_unity_package_settings, sharpening) == 44);
    assert!(std::mem::offset_of!(rsf_srvk_frame, exposure) == 272);
    assert!(std::mem::offset_of!(rsf_srvk_frame, sharpening) == 312);
    assert!(std::mem::offset_of!(rsf_srvk_frame, bias_current_color) == 384);
    assert!(size_of::<rsf_srvk_frame>() == 424);
};

type Procedure = unsafe extern "system" fn() -> isize;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(path: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> Option<Procedure>;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

struct Library(*mut c_void);

impl Library {
    fn open(path: &OsStr) -> Result<Self, String> {
        let wide: Vec<_> = path.encode_wide().chain([0]).collect();
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(format!(
                "LoadLibraryW failed for {path:?}: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(module))
    }

    fn procedure(&self, name: &CStr) -> Result<Procedure, String> {
        unsafe { GetProcAddress(self.0, name.as_ptr()) }
            .ok_or_else(|| format!("Missing export {}", name.to_string_lossy()))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

fn sharpening(mode: u32) -> rsf_sharpening_options {
    rsf_sharpening_options {
        struct_size: size_of::<rsf_sharpening_options>() as u32,
        abi_version: 1,
        mode,
        sharpness: 0.713,
        mas: 1,
        motion_sharpness: -0.31,
        motion_threshold: 4.0,
        motion_scale_limit: 19.0,
        contrast_enabled: 1,
        contrast: 1.6,
        depth_scale: 128.0,
        depth_bias: 0.0037,
        clamp_output: 1,
        debug: 1,
    }
}

macro_rules! verify {
    ($name:ident, $settings:ident, $version:ident, $set:expr, $get:expr) => {
        fn $name(path: &OsStr) -> Result<(), String> {
            let module = Library::open(path)?;
            // These names are the DLL's C exports and the generated records use their C layout.
            let configure: unsafe extern "C" fn(*const $settings) -> i32 =
                unsafe { std::mem::transmute(module.procedure($set)?) };
            let get: unsafe extern "C" fn(*mut $settings) -> i32 =
                unsafe { std::mem::transmute(module.procedure($get)?) };
            let header = || $settings {
                struct_size: size_of::<$settings>() as u32,
                abi_version: $version,
                ..$settings::default()
            };
            let mut value = $settings {
                sr_backend: 7,
                quality: 1,
                fg_backend: 7,
                generated_frames: 1,
                gameplay: 1,
                ..header()
            };
            for mode in 0..=4 {
                value.sharpening = sharpening(mode);
                let mut readback = header();
                if unsafe { configure(&value) } == 0
                    || unsafe { get(&mut readback) } == 0
                    || readback != value
                {
                    return Err(format!(
                        "{path:?} did not round-trip sharpening mode {mode}"
                    ));
                }
            }
            type Invalid = (&'static str, fn(&mut $settings));
            let refused: [Invalid; 5] = [
                ("old ABI", |request| request.abi_version = 1),
                ("short record", |request| request.struct_size -= 1),
                ("NaN strength", |request| {
                    request.sharpening.sharpness = f32::NAN
                }),
                ("unknown mode", |request| request.sharpening.mode = 5),
                ("out-of-range depth scale", |request| {
                    request.sharpening.depth_scale = 601.0
                }),
            ];
            for (label, invalidate) in refused {
                let mut request = $settings { ..value };
                invalidate(&mut request);
                if unsafe { configure(&request) } != 0 {
                    return Err(format!("{path:?} accepted {label}"));
                }
            }
            let mut readback = header();
            if unsafe { get(&mut readback) } == 0 || readback != value {
                return Err(format!("{path:?} changed settings after a refused request"));
            }
            Ok(())
        }
    };
}

verify!(
    verify_godot,
    rsf_godot_settings,
    RSF_GODOT_SETTINGS_ABI_VERSION,
    c"rsfg_configure",
    c"rsfg_settings"
);
verify!(
    verify_unity,
    rsf_unity_package_settings,
    RSF_UNITY_SETTINGS_ABI_VERSION,
    c"rsfu_configure",
    c"rsfu_settings"
);

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [godot, unity] = args.as_slice() else {
        return ExitCode::FAILURE;
    };
    if let Err(message) = verify_godot(godot).and_then(|()| verify_unity(unity)) {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    println!(
        "Godot and Unity settings: all 12 options round trip in five modes; ABI/range refusal preserves settings."
    );
    ExitCode::SUCCESS
}
