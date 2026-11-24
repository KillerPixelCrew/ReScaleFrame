// SPDX-License-Identifier: MIT
use super::*;
use crate::fake_code::Image;
use std::ptr::without_provenance_mut;

const BASE: usize = 0x1_0000;

fn base() -> *mut u8 {
    without_provenance_mut(BASE)
}

fn owned() -> rsf_ac7_code_patches {
    rsf_ac7_code_patches {
        velocity_module: null_mut(),
        velocity_applied: 0,
        scene_precision_site: null_mut(),
    }
}

/// The unpatched sites of the exact build.
fn native_image() -> Image {
    let image = Image::default();
    image.seed_temporal_patches(BASE);
    image
}

/// Whether every velocity site holds its native (or, when `patched`, its replacement) bytes.
fn velocity_holds(image: &Image, patched: bool) -> bool {
    VELOCITY.iter().all(|patch| {
        let bytes = if patched {
            patch.replacement
        } else {
            patch.native
        };
        image.has(BASE + patch.rva as usize, bytes)
    })
}

const VELOCITY_ENABLED: &str = "AC7 temporal velocity: native pass enabled without TAA; small and fixed-root movable opaque geometry retained with native history, camera-cut and shader checks";

#[test]
fn table_matches_the_former_patches() {
    let rvas: Vec<u32> = VELOCITY.iter().map(|patch| patch.rva).collect();
    assert_eq!(rvas, [0x118a4e3, 0x10fdd0c, 0x11838c4]);
    assert!(
        VELOCITY
            .iter()
            .all(|patch| patch.native.len() == 2 && patch.replacement.len() == 2)
    );
    assert_eq!(NATIVE_SCENE_FORMAT, SCENE_PRECISION_EXPECTED[..13]);
}

#[test]
fn velocity_group_applies_completely_and_once() {
    let image = native_image();
    let mut state = owned();
    assert!(velocity_apply(&mut state, base(), &image));
    assert_eq!(state.velocity_applied, 3);
    assert_eq!(state.velocity_module, base());
    assert!(velocity_holds(&image, true) && image.logged(VELOCITY_ENABLED));
    assert!(velocity_apply(&mut state, base(), &image));
    assert_eq!(image.writes.get(), 3);
}

#[test]
fn velocity_preflight_leaves_every_gate_native() {
    let image = native_image();
    image.put(BASE + 0x11838c4, &[0xcc]);
    let mut state = owned();
    assert!(!velocity_apply(&mut state, base(), &image));
    assert_eq!(state.velocity_applied, 0);
    assert!(state.velocity_module.is_null());
    assert_eq!(image.writes.get(), 0);
    assert!(image.has(BASE + 0x118a4e3, b"\x75\x0e"));
    assert!(image.logged("AC7 temporal velocity refused: expected bytes differ at RVA 0x11838c4"));
}

#[test]
fn velocity_write_failure_rolls_the_group_back() {
    let image = native_image();
    image.protected.set(Some(BASE + 0x10fdd0c));
    let mut state = owned();
    assert!(!velocity_apply(&mut state, base(), &image));
    assert_eq!(state.velocity_applied, 0);
    assert!(state.velocity_module.is_null());
    assert!(velocity_holds(&image, false));
    // The first patch was written and restored.
    assert_eq!(image.writes.get(), 2);
    assert!(!image.logged("AC7 temporal velocity rollback refused; patched module retained"));
}

#[test]
fn velocity_restore_refuses_foreign_bytes_and_keeps_ownership() {
    let image = native_image();
    let mut state = owned();
    assert!(velocity_apply(&mut state, base(), &image));
    image.put(BASE + 0x11838c4, &[0xcc]);
    assert!(!velocity_restore(&mut state, &image));
    assert_eq!(state.velocity_applied, 3);
    assert_eq!(state.velocity_module, base());
    image.put(BASE + 0x11838c4, &[0xeb]);
    assert!(velocity_restore(&mut state, &image));
    assert_eq!(state.velocity_applied, 0);
    assert!(state.velocity_module.is_null() && velocity_holds(&image, false));
}

#[test]
fn scene_precision_replaces_the_format_load_only() {
    let site = BASE + SCENE_PRECISION_RVA as usize;
    let image = native_image();
    let mut state = owned();
    assert!(scene_precision_apply(&mut state, base(), &image));
    assert_eq!(state.scene_precision_site.addr(), site);
    assert!(image.has(site, &PRECISE_SCENE_FORMAT) && image.has(site + 13, b"\x89\x45\xd7"));
    assert!(image.logged(
        "native scene precision: allocation format policy 4 at RVA 0x1095620; engine owns FP16 resource rebuild"
    ));
    assert!(scene_precision_apply(&mut state, base(), &image));
    assert_eq!(image.writes.get(), 1);
    assert!(scene_precision_restore(&mut state, &image));
    assert!(state.scene_precision_site.is_null());
    assert!(image.has(site, &SCENE_PRECISION_EXPECTED));

    // The trailing three bytes are part of the fingerprint although they are never written.
    let foreign = native_image();
    foreign.put(site + 15, &[0x00]);
    let mut state = owned();
    assert!(!scene_precision_apply(&mut state, base(), &foreign));
    assert!(state.scene_precision_site.is_null() && foreign.writes.get() == 0);
    assert!(foreign.logged(
        "native scene-precision patch refused: expected bytes differ or code protection failed at RVA 0x1095620"
    ));
}

#[test]
fn activation_orders_scene_precision_before_velocity() {
    let image = native_image();
    image.put(BASE + SCENE_PRECISION_RVA as usize, &[0x00]);
    let mut state = owned();
    assert!(!apply(&mut state, base(), &image));
    assert!(velocity_holds(&image, false) && image.writes.get() == 0);

    // A velocity refusal keeps the scene precision already applied, as the former start did.
    let image = native_image();
    image.put(BASE + 0x118a4e3, &[0x00]);
    let mut state = owned();
    assert!(!apply(&mut state, base(), &image));
    assert!(!state.scene_precision_site.is_null());
    assert_eq!(state.velocity_applied, 0);
    // A later start retries only the velocity group.
    image.put(BASE + 0x118a4e3, b"\x75");
    let writes = image.writes.get();
    assert!(apply(&mut state, base(), &image));
    assert_eq!(image.writes.get(), writes + 3);
}

#[test]
fn retirement_orders_velocity_before_scene_precision() {
    let image = native_image();
    let mut state = owned();
    assert!(apply(&mut state, base(), &image));
    image.protected.set(Some(BASE + 0x11838c4));
    assert!(!restore(&mut state, &image));
    assert!(image.logged("AC7 temporal velocity patch could not restore; renderer retained"));
    assert_eq!(state.velocity_applied, 3);
    assert!(!state.scene_precision_site.is_null());
    image.protected.set(None);
    assert!(restore(&mut state, &image));
    assert_eq!(state.velocity_applied, 0);
    assert!(state.scene_precision_site.is_null() && state.velocity_module.is_null());
    assert!(velocity_holds(&image, false));
    assert!(image.has(
        BASE + SCENE_PRECISION_RVA as usize,
        &SCENE_PRECISION_EXPECTED
    ));

    let image = native_image();
    let mut state = owned();
    assert!(apply(&mut state, base(), &image));
    image.protected.set(Some(state.scene_precision_site.addr()));
    assert!(!restore(&mut state, &image));
    assert!(image.logged("native scene-precision patch could not restore; renderer retained"));
    assert_eq!(state.velocity_applied, 0);
    assert!(!state.scene_precision_site.is_null());
}
