// SPDX-License-Identifier: MIT
//! Consistency checks on the stage profiles lent by the native fixture.
use crate::engine_guards::{NativeSite, UniqueGuard};
use crate::motion_profile::{MotionProfile, PROFILES as MOTION, SITE_CREATE_SHADER};
use std::ffi::{CStr, c_char, c_void};
use std::slice;

#[repr(C)]
struct Bank<T> {
    data: *const T,
    count: usize,
}

impl<T> Bank<T> {
    unsafe fn values(&self) -> &[T] {
        if self.count == 0 {
            return &[];
        }
        unsafe { slice::from_raw_parts(self.data, self.count) }
    }
}

#[repr(C)]
struct FirstPersonSite {
    _name: *const c_char,
    function_rva: u32,
    set_p_call: u32,
    rm_near_call: u32,
}

#[repr(C)]
struct TargetSlot {
    name: *const c_char,
    offset: u32,
    format: u32,
}

#[repr(C)]
struct Relation {
    base: u32,
    offset: u32,
    observed: u32,
}

#[repr(C)]
struct Profile {
    label: *const c_char,
    executable: *const c_char,
    sha256: *const c_char,
    image_size: u32,
    // Native, geometry and SR records; the motion record is Rust-owned (motion_profile.rs).
    owners: [*const c_void; 3],
    sites: [Bank<NativeSite>; 5],
    guards: [Bank<UniqueGuard>; 3],
    first_person: Bank<FirstPersonSite>,
    required_rvas: [u32; 2],
    sun_target: u32,
    sr_near_index: u32,
    geometry_near_index: u32,
    ssao_code: *const c_char,
    ssao_function: u32,
    ssao_viewport: u32,
    relations: [Relation; 13],
}

const _: () = {
    assert!(size_of::<Profile>() == 400 && std::mem::offset_of!(Profile, sites) == 56);
    assert!(std::mem::offset_of!(Profile, ssao_code) == 224);
    assert!(std::mem::offset_of!(Profile, relations) == 240);
    assert!(size_of::<FirstPersonSite>() == 24 && size_of::<TargetSlot>() == 16);
};

fn check_build_selection() {
    // Independent fingerprints pinned by the two executable evidence records.
    const STANDARD: &str = "C857B6F49D32C62245611B26106862A773185533856E8B858A7D7E849ED2FB4E";
    const AVX: &str = "2032ACFC041E215508692EF4079EF504C7BBABF1793D5668084099F734B92F12";
    let standard = STANDARD.to_ascii_lowercase();
    let avx = AVX.to_ascii_lowercase();
    for (name, hash, expected) in [
        ("AnomalyDX11.exe", standard.as_str(), Some(0)),
        ("anomalydx11.exe", STANDARD, Some(0)),
        ("AnomalyDX11AVX.exe", avx.as_str(), Some(1)),
        ("ANOMALYDX11AVX.EXE", AVX, Some(1)),
        ("AnomalyDX11AVX.exe", STANDARD, None),
        ("AnomalyDX11.exe", AVX, None),
        ("AnomalyDX10AVX.exe", AVX, None),
        ("AnomalyDX11.exe", &STANDARD[..63], None),
        ("AnomalyDX11\u{c4}.exe", STANDARD, None),
        ("", "", None),
    ] {
        assert_eq!(
            crate::native_profile::find(name.as_bytes(), hash.as_bytes()),
            expected,
            "profile for {name:?}"
        );
    }
}

fn byte(hex: &[u8]) -> Option<u8> {
    fn digit(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        }
    }
    Some(digit(hex[0])? << 4 | digit(hex[1])?)
}

fn guard_matches(guard: &UniqueGuard, sites: &[NativeSite]) -> bool {
    let hex = unsafe { CStr::from_ptr(guard.expected_hex) }.to_bytes();
    if hex.len() <= 32 || !hex.len().is_multiple_of(2) {
        return false;
    }
    let Some(bytes) = hex.chunks_exact(2).map(byte).collect::<Option<Vec<_>>>() else {
        return false;
    };
    let name = unsafe { CStr::from_ptr(guard.name) };
    let mut found = false;
    for site in sites
        .iter()
        .filter(|site| site.rva == guard.rva && unsafe { CStr::from_ptr(site.name) } == name)
    {
        if !bytes.starts_with(&site.expected) {
            return false;
        }
        found = true;
    }
    found
}

fn call_reaches(site: &NativeSite, target: u32) -> bool {
    let disp = i32::from_le_bytes(site.expected[1..5].try_into().unwrap());
    site.expected[0] == 0xe8 && i64::from(site.rva) + 5 + i64::from(disp) == i64::from(target)
}

fn ssao_reaches_viewport(profile: &Profile) -> bool {
    let hex = unsafe { CStr::from_ptr(profile.ssao_code) }.to_bytes();
    if hex.len() < 14 || !hex.len().is_multiple_of(2) {
        return false;
    }
    let tail = &hex[hex.len() - 14..];
    if &tail[..6] != b"4c8d05" {
        return false;
    }
    let Some(bytes) = tail[6..]
        .chunks_exact(2)
        .map(byte)
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let displacement = i32::from_le_bytes(bytes.try_into().unwrap());
    i64::from(profile.ssao_function) + (hex.len() / 2) as i64 + i64::from(displacement)
        == i64::from(profile.ssao_viewport)
}

fn check_profile(profile: &Profile, motion_profile: &MotionProfile, target_slots: usize) {
    let [native, geometry, layout, sr, sr_calls] = profile
        .sites
        .each_ref()
        .map(|bank| unsafe { bank.values() });
    let (motion, motion_calls) = (
        &motion_profile.sites[..],
        &motion_profile.create_shader_calls[..],
    );
    let motion_guards = &motion_profile.unique_guards[..];
    let shader_target = motion_profile.sites[SITE_CREATE_SHADER].rva;
    let [native_guards, geometry_guards, sr_guards] = profile
        .guards
        .each_ref()
        .map(|bank| unsafe { bank.values() });
    let first = unsafe { profile.first_person.values() };
    let in_image = |rva| rva != 0 && rva < profile.image_size;
    let label = unsafe { CStr::from_ptr(profile.label) }.to_string_lossy();
    let executable = unsafe { CStr::from_ptr(profile.executable) }.to_string_lossy();

    assert!(
        profile.required_rvas.into_iter().all(in_image),
        "{label} required RVA"
    );
    assert!(
        profile
            .sites
            .iter()
            .flat_map(|bank| unsafe { bank.values() })
            .chain(motion)
            .chain(motion_calls)
            .all(|site| in_image(site.rva)),
        "{label} site outside image"
    );
    assert!(
        sr_calls
            .iter()
            .chain(motion_calls)
            .all(|site| site.expected[0] == 0xe8)
    );
    assert!(
        sr.iter()
            .enumerate()
            .all(|(i, site)| sr[..i].iter().all(|prior| prior.rva != site.rva))
    );
    assert!(first.iter().all(|site| site.function_rva < site.set_p_call
        && site.set_p_call < site.rm_near_call
        && site.rm_near_call - site.set_p_call < 0x120));
    assert!(ssao_reaches_viewport(profile), "{label} SSAO viewport LEA");
    assert!(
        call_reaches(&sr_calls[5], profile.sun_target),
        "{label} sun wrapper call"
    );
    assert!(
        motion_calls
            .iter()
            .all(|call| call_reaches(call, shader_target))
    );
    assert!(
        native_guards
            .iter()
            .all(|guard| guard_matches(guard, native))
    );
    assert!(
        geometry_guards
            .iter()
            .all(|guard| guard_matches(guard, geometry) || guard_matches(guard, layout))
    );
    assert!(sr_guards.iter().all(|guard| guard_matches(guard, sr)));
    assert!(
        motion_guards
            .iter()
            .all(|guard| guard_matches(guard, motion))
    );
    assert!(
        profile
            .relations
            .iter()
            .all(|r| r.base.checked_add(r.offset) == Some(r.observed))
    );
    assert_eq!(
        sr[profile.sr_near_index as usize].expected,
        geometry[profile.geometry_near_index as usize].expected
    );

    println!(
        "{label} ({executable}): image {:#x}; {} native, {} geometry + {} layout, {} SR function + {} call-site, {} motion sites; unique guards native {} geometry {} SR {} motion {}; {} first-person sites; {target_slots} target slots",
        profile.image_size,
        native.len(),
        geometry.len(),
        layout.len(),
        sr.len(),
        sr_calls.len(),
        motion.len(),
        native_guards.len(),
        geometry_guards.len(),
        sr_guards.len(),
        motion_guards.len(),
        first.len()
    );
}

fn check_profiles(profiles: &[Profile; 2], slots: &[TargetSlot]) {
    let [standard, avx] = profiles;
    let standard_hash = unsafe { CStr::from_ptr(standard.sha256) };
    let avx_hash = unsafe { CStr::from_ptr(avx.sha256) };
    assert_eq!(
        (standard_hash.to_bytes().len(), avx_hash.to_bytes().len()),
        (64, 64)
    );
    assert_ne!(standard_hash, avx_hash);
    assert_ne!(unsafe { CStr::from_ptr(standard.executable) }, unsafe {
        CStr::from_ptr(avx.executable)
    });
    assert_ne!(standard.image_size, avx.image_size);
    assert!(standard.owners.iter().zip(avx.owners).all(|(&a, b)| a != b));
    for ((profile, canonical), motion) in profiles
        .iter()
        .zip(&crate::native_profile::PROFILES)
        .zip(&MOTION)
    {
        assert_eq!(unsafe { CStr::from_ptr(profile.label) }, canonical.label);
        assert_eq!(
            unsafe { CStr::from_ptr(profile.executable) },
            canonical.executable
        );
        assert_eq!(unsafe { CStr::from_ptr(profile.sha256) }, canonical.sha256);
        assert_eq!(profile.image_size, canonical.image_size);
        let projected = unsafe { profile.sites[0].values() };
        assert_eq!(projected.len(), canonical.sites.len());
        for (projection, site) in projected.iter().zip(&canonical.sites) {
            assert_eq!(projection.rva, site.rva);
            assert_eq!(projection.expected, site.expected);
            assert_eq!(unsafe { CStr::from_ptr(projection.name) }, unsafe {
                CStr::from_ptr(site.name)
            });
        }
        let projected = unsafe { profile.guards[0].values() };
        assert_eq!(projected.len(), canonical.guards.len());
        for (projection, guard) in projected.iter().zip(&canonical.guards) {
            assert_eq!(projection.rva, guard.rva);
            assert_eq!(unsafe { CStr::from_ptr(projection.name) }, unsafe {
                CStr::from_ptr(guard.name)
            });
            assert_eq!(unsafe { CStr::from_ptr(projection.expected_hex) }, unsafe {
                CStr::from_ptr(guard.expected_hex)
            });
        }
        check_profile(profile, motion, slots.len());
    }
    let name = |slot: &TargetSlot| unsafe { CStr::from_ptr(slot.name) };
    for (i, slot) in slots.iter().enumerate() {
        assert_eq!(slot.offset % 8, 0);
        assert!(
            slots[..i]
                .iter()
                .all(|prior| prior.offset != slot.offset && name(prior) != name(slot))
        );
    }
    for (expected, offset, format) in [
        (c"$user$generic0", Some(0x3a8), Some(21)),
        (c"$user$generic_temp", Some(0x3b8), None),
        (c"$user$generic1", Some(0x3b0), None),
        (c"$user$ui", Some(0x3d0), None),
        (c"$user$viewport2", Some(0x3c0), None),
        (c"$user$position", None, Some(113)),
        (c"$user$albedo", None, Some(21)),
    ] {
        let slot = slots
            .iter()
            .find(|slot| name(slot) == expected)
            .expect("target slot");
        assert!(
            offset.is_none_or(|offset| slot.offset == offset),
            "{expected:?} offset"
        );
        assert!(
            format.is_none_or(|format| slot.format == format),
            "{expected:?} format"
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_profile_contract(
    profiles: &[Profile; 2],
    slots: *const TargetSlot,
    slot_count: usize,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        // The native fixture supplies these statically bounded profile arrays.
        check_profiles(profiles, unsafe {
            slice::from_raw_parts(slots, slot_count)
        });
        check_build_selection();
        println!("0 failure(s)");
        0
    });
    result.unwrap_or(1)
}
