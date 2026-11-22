// SPDX-License-Identifier: MIT
use super::{TestResult, check};
use crate::overlay_settings::{
    rsf_overlay_fill_sharpening_stats, rsf_overlay_requested_sharpening,
};
use crate::settings_store::{
    read_sharpening, rsf_settings_store_read, rsf_settings_store_read_layout,
    rsf_settings_store_write, rsf_settings_store_write_sharpening, set_sharpening, sharpening_of,
    wide,
};
use crate::sharpening_policy::{defaults, sanitize, valid, vendor_sharpness};
use crate::sys::{
    RSF_RENDER_FRONTEND_ABI_VERSION, rsf_render_settings as Settings,
    rsf_sharpening_options as Options,
};
use crate::sys::{rsf_overlay_intent as Intent, rsf_overlay_stats as Stats};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileSectionW, GetPrivateProfileStringW, WritePrivateProfileStringW,
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;

#[repr(C)]
struct Services {
    set_sharpening: unsafe extern "C" fn(*const Options) -> i32,
    ffx_fields: unsafe extern "C" fn(f32, *mut u32, *mut f32),
    decimal: unsafe extern "C" fn(f32, *mut u16, usize) -> i32,
}

const _: () = assert!(size_of::<Services>() == 3 * size_of::<usize>());

struct Ini {
    directory: PathBuf,
    path: Vec<u16>,
}

impl Ini {
    fn new() -> TestResult<Self> {
        let start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory =
            std::env::temp_dir().join(format!("rsf-sharpening-{}-{start}", std::process::id()));
        fs::create_dir(&directory)?;
        let path = directory
            .join("settings.ini")
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect();
        Ok(Self { directory, path })
    }

    fn get(&self, section: &[u16], key: &str) -> String {
        let mut text = [0; 128];
        let len = unsafe {
            GetPrivateProfileStringW(
                section.as_ptr(),
                wide(key).as_ptr(),
                wide("<missing>").as_ptr(),
                text.as_mut_ptr(),
                text.len() as u32,
                self.path.as_ptr(),
            )
        };
        String::from_utf16_lossy(&text[..len as usize])
    }

    fn set(&self, section: &[u16], key: &str, value: &str) -> TestResult {
        let wrote = unsafe {
            WritePrivateProfileStringW(
                section.as_ptr(),
                wide(key).as_ptr(),
                wide(value).as_ptr(),
                self.path.as_ptr(),
            )
        };
        check(wrote != 0, &format!("write profile key {key}"))
    }

    fn write(&self, section: &[u16], settings: &Settings) -> TestResult {
        check(
            unsafe { rsf_settings_store_write(self.path.as_ptr(), section.as_ptr(), settings) }
                == OK,
            "write shared render settings",
        )
    }

    fn read(&self, section: &[u16], fallback: &Settings) -> TestResult<Settings> {
        let mut output = blank();
        check(
            unsafe {
                rsf_settings_store_read(
                    self.path.as_ptr(),
                    section.as_ptr(),
                    fallback,
                    &defaults(),
                    &mut output,
                )
            } == OK,
            "read shared render settings",
        )?;
        Ok(output)
    }
}

impl Drop for Ini {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.directory.join("settings.ini"));
        let _ = fs::remove_dir(&self.directory);
    }
}

fn blank() -> Settings {
    let mut settings: Settings = unsafe { std::mem::zeroed() };
    settings.struct_size = size_of::<Settings>() as u32;
    settings.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    settings
}

fn same(a: &Options, b: &Options) -> bool {
    // C and the generated ABI assert a 56-byte record consisting of fourteen
    // consecutive 4-byte fields. Compare bits, including the sign of zero.
    const _: () = assert!(size_of::<Options>() == 14 * 4);
    let bytes = |o: &Options| {
        let raw = ptr::from_ref(o).cast::<[u8; size_of::<Options>()]>();
        unsafe { *raw }
    };
    bytes(a) == bytes(b)
}

fn format_check(
    api: &Services,
    ini: &Ini,
    section: &[u16],
    settings: &mut Settings,
    value: f32,
) -> TestResult {
    settings.sharpening_contrast = value;
    ini.write(section, settings)?;
    let mut text = [0u16; 64];
    let size = unsafe { (api.decimal)(value, text.as_mut_ptr(), text.len()) };
    let length = usize::try_from(size)?;
    check(
        length < text.len(),
        "CRT formatting exceeded the destination",
    )?;
    let expected = String::from_utf16_lossy(&text[..length]);
    let actual = ini.get(section, "Contrast");
    check(
        actual == expected,
        &format!("decimal {value:?}: store {actual:?}, CRT {expected:?}"),
    )?;
    if value.is_finite() {
        check(
            ini.read(section, settings)?.sharpening_contrast.to_bits() == value.to_bits(),
            "decimal round-trip preserves bits",
        )?;
    }
    Ok(())
}

fn store_format(api: &Services, ini: &Ini) -> TestResult {
    let section = wide("Format");
    ini.set(&section, "OverlayLayout", "9")?;
    ini.set(&section, "sharpness", "0.1")?;
    let mut settings = Settings {
        sr_enabled: 1,
        sr_backend: 3,
        dlss_preset: 2,
        fg_enabled: 1,
        fg_backend: 4,
        generated_frames: 3,
        latency: 2,
        frame_limit_us: 16667,
        status_overlay: 1,
        sr_quality: 4,
        ..blank()
    };
    set_sharpening(
        &mut settings,
        &Options {
            mode: 2,
            mas: 1,
            contrast_enabled: 1,
            clamp_output: 1,
            debug: 1,
            ..defaults()
        },
    );
    ini.write(&section, &settings)?;
    for (key, value) in [
        ("SR", "1"),
        ("SRBackend", "3"),
        ("DLSSPreset", "2"),
        ("FG", "1"),
        ("FGBackend", "4"),
        ("FGGeneratedFrames", "3"),
        ("Latency", "2"),
        ("FrameLimitUs", "16667"),
        ("StatusOverlay", "1"),
        ("SRQuality", "4"),
        ("SharpeningMode", "2"),
        ("Sharpness", "0.400000006"),
        ("MAS", "1"),
        ("MotionSharpness", "0.200000003"),
        ("MotionThreshold", "0"),
        ("MotionScaleLimit", "10"),
        ("ContrastEnabled", "1"),
        ("Contrast", "0"),
        ("DADepthScale", "35"),
        ("DADepthBias", "0.00100000005"),
        ("DAClampOutput", "1"),
        ("SharpeningDebug", "1"),
    ] {
        check(ini.get(&section, key) == value, &format!("owned key {key}"))?;
    }
    check(
        ini.get(&section, "OverlayLayout") == "9"
            && unsafe { rsf_settings_store_read_layout(ini.path.as_ptr(), section.as_ptr()) } == 9,
        "layout survives settings write",
    )?;
    let mut lines = [0; 4096];
    let count = unsafe {
        GetPrivateProfileSectionW(
            section.as_ptr(),
            lines.as_mut_ptr(),
            lines.len() as u32,
            ini.path.as_ptr(),
        )
    } as usize;
    let prefix = "Sharpness=";
    let sharpness_count = lines[..count]
        .split(|&ch| ch == 0)
        .filter(|line| {
            String::from_utf16_lossy(line)
                .get(..prefix.len())
                .is_some_and(|key| key.eq_ignore_ascii_case(prefix))
        })
        .count();
    check(sharpness_count == 1, "replace differently cased owned key")?;

    let edge = [
        0.0,
        -0.0,
        1e-5,
        1e-4,
        123_456_792.0,
        1_234_567_936.0,
        f32::MAX,
        f32::MIN_POSITIVE,
    ];
    let mut random = 0x1234567u32;
    for i in 0..1600 {
        let value = match i {
            0..512 => 1.0 + i as f32 / 512.0,
            512..520 => edge[i - 512],
            _ => {
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let bits = if (random >> 23) & 0xff == 0xff {
                    random ^ (1 << 23)
                } else {
                    random
                };
                f32::from_bits(bits)
            }
        };
        format_check(api, ini, &section, &mut settings, value)?;
    }
    for value in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, -f32::NAN] {
        format_check(api, ini, &section, &mut settings, value)?;
    }
    let missing = ini.read(&wide("Empty"), &blank())?;
    check(
        same(&sharpening_of(&missing, &defaults()), &defaults()),
        "missing keys use sharpening defaults",
    )?;
    let large = wide("Large");
    ini.set(&large, "Padding", &"x".repeat(40_000))?;
    check(
        unsafe { rsf_settings_store_write(ini.path.as_ptr(), large.as_ptr(), &settings) }
            == INVALID_ARGUMENT
            && ini.get(&large, "SR") == "<missing>",
        "oversized section refused unchanged",
    )?;
    let bad = Options {
        depth_scale: 601.0,
        ..defaults()
    };
    check(
        unsafe { rsf_settings_store_write_sharpening(ini.path.as_ptr(), section.as_ptr(), &bad) }
            == INVALID_ARGUMENT
            && ini.get(&section, "DADepthScale") == "35",
        "invalid write refused unchanged",
    )?;
    let short = Options {
        struct_size: size_of::<Options>() as u32 - 4,
        ..defaults()
    };
    check(
        unsafe { rsf_settings_store_write_sharpening(ini.path.as_ptr(), section.as_ptr(), &short) }
            == INVALID_ARGUMENT,
        "store refuses short sharpening record",
    )?;
    check(
        unsafe { rsf_settings_store_write(ini.path.as_ptr(), ptr::null(), &settings) }
            == INVALID_ARGUMENT
            && unsafe {
                rsf_settings_store_write(ini.path.as_ptr(), section.as_ptr(), ptr::null())
            } == INVALID_ARGUMENT,
        "store refuses missing arguments",
    )
}

fn overlay(options: &Options) -> TestResult {
    let mut stats: Stats = unsafe { std::mem::zeroed() };
    rsf_overlay_fill_sharpening_stats(&mut stats, options);
    let mut intent: Intent = unsafe { std::mem::zeroed() };
    intent.sharpening_mode = stats.sharpening_mode;
    intent.sharpening_sharpness = stats.sharpening_sharpness;
    intent.sharpening_mas = stats.sharpening_mas;
    intent.sharpening_motion_sharpness = stats.sharpening_motion_sharpness;
    intent.sharpening_motion_threshold = stats.sharpening_motion_threshold;
    intent.sharpening_motion_scale_limit = stats.sharpening_motion_scale_limit;
    intent.sharpening_contrast_enabled = stats.sharpening_contrast_enabled;
    intent.sharpening_contrast = stats.sharpening_contrast;
    intent.sharpening_depth_scale = stats.sharpening_depth_scale;
    intent.sharpening_depth_bias = stats.sharpening_depth_bias;
    intent.sharpening_clamp_output = stats.sharpening_clamp_output;
    intent.sharpening_debug = stats.sharpening_debug;
    let readback = rsf_overlay_requested_sharpening(&intent);
    check(
        stats.sharpening_available != 0 && same(options, &readback),
        "D3D12 stats/intent mapping",
    )
}

fn verify(api: &Services) -> TestResult {
    let default = defaults();
    let short = Options {
        struct_size: default.struct_size - 1,
        ..default
    };
    let old = Options {
        abi_version: default.abi_version + 1,
        ..default
    };
    check(
        unsafe { (api.set_sharpening)(&short) } == INVALID_ARGUMENT,
        "appended D3D11 setter refuses short options",
    )?;
    check(
        unsafe { (api.set_sharpening)(&old) } == ABI_MISMATCH,
        "D3D11 setter refuses mismatched ABI",
    )?;
    check(
        unsafe { (api.set_sharpening)(&default) } == OK && valid(&default),
        "D3D11 setter accepts valid defaults before graphics startup",
    )?;
    let ini = Ini::new()?;
    let section = wide("Sharpening");
    let mut settings = blank();
    set_sharpening(&mut settings, &default);
    ini.set(&section, "Foreign", "kept")?;
    for mode in 0..=4 {
        let options = Options {
            mode,
            sharpness: 0.73,
            mas: 1,
            motion_sharpness: -0.17,
            motion_threshold: 2.0,
            motion_scale_limit: 17.0,
            contrast_enabled: 1,
            contrast: 1.3,
            depth_scale: 105.0,
            depth_bias: 0.0037,
            clamp_output: 1,
            debug: 1,
            ..default
        };
        set_sharpening(&mut settings, &options);
        ini.write(&section, &settings)?;
        let stored = ini.read(&section, &settings)?;
        check(
            same(&sharpening_of(&stored, &default), &options),
            "round-trip all sharpening fields",
        )?;
        for backend in 0..=6 {
            let mut enabled = 0;
            let mut amount = 0.0;
            unsafe {
                (api.ffx_fields)(
                    vendor_sharpness(&options, backend),
                    &mut enabled,
                    &mut amount,
                )
            };
            let native = mode == 4 && (2..=4).contains(&backend);
            check(
                enabled == u32::from(native) && amount == if native { 0.73 } else { 0.0 },
                "FFX descriptor fields per mode and backend",
            )?;
        }
    }
    let bad = Options {
        mode: 99,
        sharpness: 2.0,
        mas: 7,
        motion_sharpness: -5.0,
        motion_threshold: f32::NAN,
        motion_scale_limit: -1.0,
        contrast_enabled: 4,
        contrast: 8.0,
        depth_scale: 900.0,
        depth_bias: -1.0,
        clamp_output: 2,
        debug: 3,
        ..default
    };
    let clamped = Options {
        sharpness: 1.0,
        motion_sharpness: -1.0,
        motion_scale_limit: 0.01,
        contrast: 2.0,
        depth_scale: 600.0,
        depth_bias: 0.0,
        ..default
    };
    check(
        !valid(&bad) && valid(&clamped) && same(&sanitize(bad), &clamped),
        "sanitize every option",
    )?;
    check(
        ini.get(&section, "Foreign") == "kept",
        "foreign key preserved",
    )?;
    ini.set(&section, "Sharpness", "nan")?;
    ini.set(&section, "MotionScaleLimit", "-8")?;
    let stored = ini.read(&section, &settings)?;
    let normalized = sanitize(sharpening_of(&stored, &default));
    check(
        normalized.sharpness == 0.4 && normalized.motion_scale_limit == 0.01,
        "sanitize invalid INI values",
    )?;
    let rendering = wide("Rendering");
    ini.set(&rendering, "EngineQuality", "untouched")?;
    for mode in 0..=4 {
        let options = Options {
            mode,
            sharpness: 0.64,
            motion_sharpness: -0.1,
            depth_bias: 0.0000003,
            mas: 1,
            debug: 1,
            ..default
        };
        check(
            unsafe {
                rsf_settings_store_write_sharpening(ini.path.as_ptr(), rendering.as_ptr(), &options)
            } == OK,
            "callback host writes shared keys",
        )?;
        let stored = unsafe { read_sharpening(ini.path.as_ptr(), rendering.as_ptr()) };
        check(
            same(&stored, &options),
            "callback host round-trip preserves precision",
        )?;
        overlay(&options)?;
    }
    check(
        ini.get(&rendering, "EngineQuality") == "untouched",
        "engine-owned key preserved",
    )?;
    store_format(api, &ini)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_settings_fixture(api: &Services) -> i32 {
    if let Err(error) = verify(api) {
        eprintln!("sharpening_settings: {error}");
        return 1;
    }
    println!("sharpening_settings: PASS");
    0
}
