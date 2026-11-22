// SPDX-License-Identifier: MIT
//! SR hook admission against synthetic images of the two production profiles.
use crate::sr_stage_abi::{rsf_anomaly_sr_jitter_uv as JitterUv, rsf_anomaly_sr_log as Log};
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::slice;
use windows_sys::Win32::System::WindowsProgramming::WritePrivateProfileStringW;
use windows_sys::core::w;

#[repr(C)]
struct Site {
    name: *const c_char,
    rva: u32,
    expected: [u8; 16],
}

#[repr(C)]
struct Guard {
    name: *const c_char,
    rva: u32,
    expected_hex: *const c_char,
}

#[repr(C)]
struct FirstPerson {
    name: *const c_char,
    function_rva: u32,
    set_p_call: u32,
    rm_near_call: u32,
}

#[repr(C)]
struct Profile {
    functions: *const [Site; 17],
    unique: *const Guard,
    unique_count: usize,
    calls: *const [Site; 6],
    first_person: *const [FirstPerson; 6],
    reconstruction: *const [Guard; 4],
    ssao_code: *const c_char,
    ssao_rva: u32,
    image_size: u32,
}

#[repr(C)]
struct Calls {
    configure: unsafe extern "C" fn(*const u16, Log, *mut c_void) -> bool,
    install: unsafe extern "C" fn(*mut u8, u32, bool) -> bool,
    active: extern "C" fn() -> bool,
    frame_end: extern "C" fn(),
    report: unsafe extern "C" fn(Log, *mut c_void),
    quiesce: extern "C" fn() -> bool,
    jitter: unsafe extern "C" fn(*mut JitterUv) -> bool,
}

const _: () = {
    assert!(size_of::<Site>() == 32 && size_of::<Guard>() == 24 && size_of::<FirstPerson>() == 24);
    assert!(size_of::<Profile>() == 64 && size_of::<Calls>() == 56);
};

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
        let value: Vec<_> = value.to_string().encode_utf16().chain([0]).collect();
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
    functions: &'a [Site; 17],
    unique: &'a [Guard],
    calls: &'a [Site; 6],
    first_person: &'a [FirstPerson; 6],
    reconstruction: &'a [Guard; 4],
    ssao: (u32, Vec<u8>),
    host: &'a Calls,
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
            unsafe { (self.host.configure)(ini.path().as_ptr(), Some(collect), ptr::null_mut()) },
            stage != 0
        );
    }

    fn install(&mut self, size: u32, outside: bool) -> bool {
        LINES.with_borrow_mut(Vec::clear);
        let installed = unsafe { (self.host.install)(self.image.as_mut_ptr(), size, outside) };
        assert!(!(self.host.active)());
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
        (self.host.frame_end)();
        unsafe { (self.host.report)(Some(collect), ptr::null_mut()) };
        assert!(LINES.with_borrow(Vec::is_empty) && (self.host.quiesce)());

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
        let mut uv = JitterUv::default();
        assert!(!unsafe { (self.host.jitter)(&mut uv) });
        assert_eq!(
            [uv.current_u, uv.current_v, uv.previous_u, uv.previous_v],
            [0.0; 4]
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_hook_refusals(profile: &Profile, host: &Calls) -> i32 {
    let result = std::panic::catch_unwind(|| {
        // The native executable lends immutable production profile records for this call.
        let mut fixture = unsafe {
            Fixture {
                image: vec![0; profile.image_size as usize],
                functions: &*profile.functions,
                unique: slice::from_raw_parts(profile.unique, profile.unique_count),
                calls: &*profile.calls,
                first_person: &*profile.first_person,
                reconstruction: &*profile.reconstruction,
                ssao: (profile.ssao_rva, bytes(CStr::from_ptr(profile.ssao_code))),
                host,
            }
        };
        fixture.exercise(profile.image_size);
    });
    i32::from(result.is_err())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_build_selection_fixture(
    identities: &[crate::BuildIdentity; 2],
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let standard = unsafe { CStr::from_ptr(identities[0].sha256) }
            .to_str()
            .unwrap();
        let avx = unsafe { CStr::from_ptr(identities[1].sha256) }
            .to_str()
            .unwrap();
        let cases = [
            ("AnomalyDX11.exe", standard, Some(0)),
            (
                "anomalydx11.exe",
                "C857B6F49D32C62245611B26106862A773185533856E8B858A7D7E849ED2FB4E",
                Some(0),
            ),
            ("AnomalyDX11AVX.exe", avx, Some(1)),
            (
                "ANOMALYDX11AVX.EXE",
                "2032ACFC041E215508692EF4079EF504C7BBABF1793D5668084099F734B92F12",
                Some(1),
            ),
            ("AnomalyDX11AVX.exe", standard, None),
            ("AnomalyDX11.exe", avx, None),
            ("AnomalyDX10AVX.exe", avx, None),
            ("AnomalyDX11.exe", &standard[..63], None),
            ("AnomalyDX11\u{c4}.exe", standard, None),
            ("", "", None),
        ];
        for (name, hash, expected) in cases {
            let wide: Vec<_> = name.encode_utf16().collect();
            let index = unsafe {
                crate::rsf_anomaly_match_build_wide(
                    identities.as_ptr(),
                    identities.len(),
                    wide.as_ptr(),
                    wide.len(),
                    hash.as_ptr().cast(),
                    hash.len(),
                )
            };
            assert_eq!(
                index,
                expected.unwrap_or(usize::MAX),
                "profile for {name:?}"
            );
        }
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
