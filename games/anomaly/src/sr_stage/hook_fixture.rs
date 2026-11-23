// SPDX-License-Identifier: MIT
//! SR hook admission against synthetic images of the two production profiles. The C11 launcher lends the production
//! leaf table and a real x64 target for the shared rmNear detour; the profile records are sr_profile.rs.
use super::native::{BUILDS, Build, Native, rsf_anomaly_sr_stage_configure_native};
use super::{STAGE, rsf_anomaly_sr_stage_frame_end, rsf_anomaly_sr_stage_report};
use crate::engine_guards::{NativeSite, UniqueGuard};
use crate::sr_profile::{FirstPersonSite, PROFILES as SR};
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use windows_sys::Win32::System::WindowsProgramming::WritePrivateProfileStringW;
use windows_sys::core::w;

const COMBINE: usize = 1;
const BLUR: usize = 3;
const NEAR: usize = 4;
const FAR: usize = 5;
const SET_P: usize = 6;
const MIP_BIAS: usize = 9;
const PARTICLE: usize = 13;
const POINT: usize = 14;
const SUN: usize = 15;

fn bytes(text: &CStr) -> Vec<u8> {
    text.to_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let nibble = |byte: u8| char::from(byte).to_digit(16).unwrap() as u8;
            (nibble(pair[0]) << 4) | nibble(pair[1])
        })
        .collect()
}

fn put(image: &mut [u8], rva: u32, bytes: &[u8]) {
    let rva = rva as usize;
    image[rva..rva + bytes.len()].copy_from_slice(bytes);
}

fn flip(image: &mut [u8], rva: u32, offset: usize, mask: u8) {
    image[rva as usize + offset] ^= mask;
}

fn rel_call(image: &mut [u8], at: u32, target: u32) {
    put(image, at, &[0xe8]);
    let displacement = i32::try_from(i64::from(target) - (i64::from(at) + 5)).unwrap();
    put(image, at + 1, &displacement.to_le_bytes());
}

struct Ini(std::path::PathBuf);

impl Ini {
    fn new() -> Self {
        let ini =
            Self(std::env::temp_dir().join(format!("rsf-sr-hooks-{}.ini", std::process::id())));
        std::fs::write(&ini.0, "[Anomaly]\r\n").unwrap();
        ini
    }

    fn path(&self) -> Vec<u16> {
        self.0.as_os_str().encode_wide().chain([0]).collect()
    }

    fn set_stage(&self, value: u32) {
        let value = rsf_game_sys::ffi_util::wide(&value.to_string());
        assert_ne!(
            unsafe {
                WritePrivateProfileStringW(
                    w!("Anomaly"),
                    w!("SRStage"),
                    value.as_ptr(),
                    self.path().as_ptr(),
                )
            },
            0
        );
    }
}

impl Drop for Ini {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

thread_local! {
    static LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

unsafe extern "C" fn collect(_: *mut c_void, line: *const c_char) {
    let line = unsafe { CStr::from_ptr(line) }
        .to_string_lossy()
        .into_owned();
    LINES.with_borrow_mut(|lines| lines.push(line));
}

fn check_line(expected: &str) {
    LINES.with_borrow(|lines| {
        assert!(
            lines.iter().any(|line| line.contains(expected)),
            "expected {expected:?} in {lines:?}"
        );
    });
}

struct Fixture<'a> {
    image: Vec<u8>,
    functions: &'a [NativeSite; 17],
    unique: &'a [UniqueGuard],
    calls: &'a [NativeSite; 6],
    first_person: &'a [FirstPersonSite; 6],
    reconstruction: &'a [UniqueGuard; 4],
    ssao: (u32, Vec<u8>),
    native: &'static Native,
    build: usize,
}

impl Fixture<'_> {
    fn fill(&mut self) {
        for site in self.functions {
            put(&mut self.image, site.rva, &site.expected);
        }
        for guard in self.unique {
            put(
                &mut self.image,
                guard.rva,
                &bytes(unsafe { CStr::from_ptr(guard.expected_hex) }),
            );
        }
        for site in self.calls {
            put(&mut self.image, site.rva, &site.expected);
        }
        for guard in self.reconstruction {
            put(
                &mut self.image,
                guard.rva,
                &bytes(unsafe { CStr::from_ptr(guard.expected_hex) }),
            );
        }
        put(&mut self.image, self.ssao.0, &self.ssao.1);
    }

    fn fill_first_person(&mut self) {
        self.fill();
        for site in self.first_person {
            rel_call(&mut self.image, site.set_p_call, self.functions[SET_P].rva);
            if [self.functions[PARTICLE].rva, self.functions[POINT].rva]
                .contains(&site.function_rva)
            {
                rel_call(&mut self.image, site.rm_near_call, self.functions[NEAR].rva);
            } else {
                put(
                    &mut self.image,
                    site.rm_near_call,
                    &[0xff, 0x90, 0xb8, 1, 0, 0],
                );
            }
        }
    }

    fn configure(&mut self, ini: &Ini, stage: u32) {
        ini.set_stage(stage);
        assert_eq!(
            unsafe {
                rsf_anomaly_sr_stage_configure_native(
                    self.native,
                    ini.path().as_ptr(),
                    true,
                    Some(collect),
                    ptr::null_mut(),
                )
            },
            stage != 0
        );
    }

    fn install(&mut self, size: u32, outside: bool) -> bool {
        LINES.with_borrow_mut(Vec::clear);
        let selected = &BUILDS[self.build];
        let build = if outside {
            // A copy of the build whose first call site lies at the end of the image.
            let mut sr = *selected.sr;
            sr.call_sites[0].rva = selected.native.image_size;
            Box::leak(Box::new(Build {
                index: selected.index,
                native: selected.native,
                sr: Box::leak(Box::new(sr)),
            }))
        } else {
            selected
        };
        let installed = STAGE.install(
            self.image.as_mut_ptr().cast(),
            size,
            build.native.image_size,
            ptr::from_ref(build).cast(),
        );
        assert!(!STAGE.active());
        installed
    }

    fn refused(&mut self, reason: &str) {
        assert!(!self.install(self.image.len() as u32, false));
        check_line(reason);
    }

    fn exercise(&mut self, size: u32) {
        let ini = Ini::new();
        self.configure(&ini, 0);
        assert!(self.install(size, false));
        self.configure(&ini, 1);
        self.refused("guard mismatch at CRenderTarget::phase_scene_prepare");
        assert!(!self.install(size - 0x1000, false));
        check_line("image size differs");

        self.fill();
        flip(&mut self.image, self.functions[BLUR].rva, 16, 0xff);
        self.refused("guard mismatch at CRenderTarget::phase_blur");
        self.fill();
        put(&mut self.image, self.calls[3].rva, &[0x90]);
        self.refused("guard mismatch at phase_combine -> phase_sunshafts");
        for offset in [0, 29] {
            self.fill();
            flip(&mut self.image, self.functions[SUN].rva, offset, 0xff);
            self.refused("guard mismatch at CRender::render_sun_cascades");
        }
        self.fill();
        flip(&mut self.image, self.calls[5].rva, 1, 0x01);
        self.refused("guard mismatch at CRender::Render -> render_sun_cascades");

        self.fill();
        self.image.extend_from_slice(&self.calls[0].expected);
        assert!(!self.install(size, true));
        check_line("guard mismatch at");
        check_line(
            unsafe { CStr::from_ptr(self.calls[0].name) }
                .to_str()
                .unwrap(),
        );
        self.image.truncate(size as usize);
        self.fill();
        self.refused("guarded detour installation failed");
        for site in self.functions {
            let rva = site.rva as usize;
            assert_eq!(self.image[rva..rva + 16], site.expected);
        }

        self.configure(&ini, 2);
        self.fill();
        flip(&mut self.image, self.ssao.0, self.ssao.1.len() - 4, 0x01);
        self.refused("guard mismatch at set_viewport");
        self.fill();
        flip(&mut self.image, self.functions[COMBINE].rva, 0, 0xff);
        self.refused("guard mismatch at CRenderTarget::phase_combine");
        self.fill();
        self.refused("guarded detour installation failed");
        LINES.with_borrow_mut(Vec::clear);
        rsf_anomaly_sr_stage_frame_end();
        unsafe { rsf_anomaly_sr_stage_report(Some(collect), ptr::null_mut()) };
        assert!(LINES.with_borrow(Vec::is_empty) && STAGE.quiesce());

        self.configure(&ini, 3);
        self.fill();
        flip(&mut self.image, self.functions[SET_P].rva, 20, 0x01);
        self.refused("guard mismatch at R_xforms::set_P");
        self.fill();
        flip(&mut self.image, self.functions[MIP_BIAS].rva, 12, 0x01);
        self.refused("guard mismatch at dx10SamplerStateCache::SetMipLODBias");
        self.fill_first_person();
        self.refused("guarded detour installation failed");
        self.fill_first_person();
        rel_call(
            &mut self.image,
            self.first_person[2].set_p_call,
            self.functions[SET_P].rva + 0x10,
        );
        self.refused("guard mismatch at r_dsgraph_render_sorted");
        self.fill_first_person();
        put(
            &mut self.image,
            self.first_person[0].rm_near_call + 2,
            &[0xc0],
        );
        self.refused("guard mismatch at r_dsgraph_render_hud ");
        self.fill_first_person();
        rel_call(
            &mut self.image,
            self.first_person[5].rm_near_call,
            self.functions[FAR].rva,
        );
        self.refused("guard mismatch at CRenderTarget::accum_point");

        for site in self.reconstruction {
            self.fill_first_person();
            let hex = unsafe { CStr::from_ptr(site.expected_hex) };
            flip(
                &mut self.image,
                site.rva,
                hex.to_bytes().len() / 2 - 1,
                0x01,
            );
            self.refused(unsafe { CStr::from_ptr(site.name) }.to_str().unwrap());
        }
        let uv = STAGE.jitter_uv();
        assert!(!STAGE.jitter().applied);
        assert_eq!(
            [uv.current_u, uv.current_v, uv.previous_u, uv.previous_v],
            [0.0; 4]
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_sr_hooks(
    argc: i32,
    argv: *const *const c_char,
    native: &'static Native,
    exercise_near: extern "C" fn(usize, usize) -> i32,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let build = unsafe { crate::fixture_profile::rsf_test_anomaly_select_build(argc, argv) };
        let identity = &crate::native_profile::PROFILES[build];
        println!(
            "build profile: {} ({})",
            identity.label.to_string_lossy(),
            identity.executable.to_string_lossy()
        );
        // The launcher redirects the geometry producer's rmNear site, which shares its bytes with the SR record.
        let geometry = &crate::geometry_profile::PROFILES[build].sites;
        let near = geometry
            .iter()
            .position(|site| unsafe { CStr::from_ptr(site.name) } == c"CRender::rmNear")
            .expect("geometry rmNear site");
        assert_eq!(
            geometry[near].expected,
            SR[build].function_sites[NEAR].expected
        );
        let failures = exercise_near(build, near) + guard_refusals(build, native);
        println!("{failures} failure(s)");
        i32::from(failures != 0)
    });
    result.unwrap_or(1)
}

fn guard_refusals(build: usize, native: &'static Native) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let profile = &SR[build];
        let image_size = crate::native_profile::PROFILES[build].image_size;
        let mut fixture = Fixture {
            image: vec![0; image_size as usize],
            functions: &profile.function_sites,
            unique: profile.unique_guards,
            calls: &profile.call_sites,
            first_person: &profile.first_person_sites,
            reconstruction: &profile.reconstruction_guards,
            ssao: (
                profile.ssao_set_viewport_rva,
                bytes(profile.ssao_set_viewport_code),
            ),
            native,
            build,
        };
        fixture.exercise(image_size);
    });
    i32::from(result.is_err())
}

#[repr(C)]
struct NearCalls {
    invoke: unsafe extern "C" fn(*mut c_void),
    set_callback: extern "C" fn(bool),
    disable: unsafe extern "C" fn(*mut c_void) -> bool,
    counts: unsafe extern "C" fn(*mut i32, *mut i32, *mut *mut c_void),
}

const _: () = assert!(size_of::<NearCalls>() == 32);

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_hook_near(calls: &NearCalls, hooks: *mut c_void) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let mut instance = 0_i32;
        let instance = ptr::from_mut(&mut instance).cast();
        let invoke = || {
            let (mut near, mut callbacks, mut seen) = (0, 0, ptr::null_mut());
            unsafe {
                (calls.invoke)(instance);
                (calls.counts)(&mut near, &mut callbacks, &mut seen);
            }
            (near, callbacks, seen)
        };
        assert_eq!(invoke(), (1, 0, ptr::null_mut()));
        (calls.set_callback)(true);
        assert_eq!(invoke(), (2, 1, instance));
        (calls.set_callback)(false);
        assert_eq!(invoke(), (3, 1, instance));
        assert!(unsafe { (calls.disable)(hooks) });
        (calls.set_callback)(true);
        assert_eq!(invoke(), (4, 1, instance));
        (calls.set_callback)(false);
    });
    i32::from(result.is_err())
}
