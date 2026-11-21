// SPDX-License-Identifier: MIT
use super::*;
use crate::fake_code::Image;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::c_uchar;
use std::ptr::{null, null_mut, without_provenance};

/// The exact build as far as admission reads it.
fn exact_build() -> Image {
    let image = Image::default();
    for site in HOOK_SITES.iter().chain(&HELPERS) {
        image.put(site.rva as usize, &site.expected);
    }
    for gate in &DEPTH_GATES {
        image.put(gate.rva as usize, gate.expected);
    }
    image
}

fn last_log(image: &Image) -> String {
    image.log.borrow().last().cloned().unwrap_or_default()
}

#[test]
fn hook_indices_name_the_former_sites() {
    // The names the native graph fixture checked against the former C++ table.
    assert_eq!(rsf_ac7_hook_rva(19), 0x3a3f70); // engine_tick
    assert_eq!(rsf_ac7_hook_rva(57), 0xebf050); // base_pass
    assert_eq!(rsf_ac7_hook_rva(11), 0x10991e0); // descriptor_output
    assert_eq!(rsf_ac7_hook_rva(43), 0xf4c620); // pixel_enqueue_d
    assert_eq!(rsf_ac7_hook_rva(56), 0xe31e90); // create_pixel_shader
    assert_eq!(rsf_ac7_hook_rva(64), 0x190ce60); // anim_trail_fill
    assert_eq!(rsf_ac7_hook_rva(RSF_AC7_HOOK_SITE_COUNT), 0);
    assert_eq!(rsf_ac7_hook_role(0), RSF_AC7_ROLE_TONEMAP);
    assert_eq!(rsf_ac7_hook_role(5), RSF_AC7_ROLE_OUTPUT);
    assert_eq!(rsf_ac7_hook_role(RSF_AC7_PROCESS_HOOKS), 0);
    assert_eq!(rsf_ac7_hook_role(u32::MAX), 0);
    let unique: HashSet<u32> = HOOK_SITES.iter().map(|site| site.rva).collect();
    assert_eq!(unique.len(), HOOK_SITES.len());
}

#[test]
fn evidence_hooks_wait_for_diagnostics() {
    assert!(!wanted(RSF_AC7_EVIDENCE_FIRST, false) && !wanted(RSF_AC7_EVIDENCE_LAST, false));
    assert!(wanted(RSF_AC7_EVIDENCE_FIRST, true) && wanted(RSF_AC7_EVIDENCE_LAST, true));
    // pixel_view_uniform and sky_projection bracket the evidence range and are always hooked.
    assert!(wanted(37, false) && wanted(44, false));
    assert!(!rsf_ac7_hook_wanted(RSF_AC7_HOOK_SITE_COUNT, true));
    let always = (0..RSF_AC7_HOOK_SITE_COUNT)
        .filter(|&hook| wanted(hook, false))
        .count();
    assert_eq!(always, 59);
}

#[test]
fn admission_accepts_the_exact_build_silently() {
    let image = exact_build();
    assert!(admit(null(), &image));
    assert!(image.log.borrow().is_empty());
}

#[test]
fn admission_reports_the_first_difference_in_former_order() {
    let image = exact_build();
    image.put(0x11346d0, &[0xcc]); // last helper
    image.put(0x109d03a, &[0xcc]); // third depth gate
    image.put(0x190ce60, &[0xcc]); // last hooked entry
    assert!(!admit(null(), &image));
    assert_eq!(
        last_log(&image),
        "AC7 native renderer refused: expected entry bytes differ at RVA 0x190ce60"
    );
    image.put(0x190ce60, &[0x48]);
    assert!(!admit(null(), &image));
    assert_eq!(
        last_log(&image),
        "AC7 native renderer refused: enlarged UI depth/view gate missing at RVA 0x109d03a"
    );
    image.put(0x109d03a, &[0x44]);
    assert!(!admit(null(), &image));
    assert_eq!(
        last_log(&image),
        "AC7 native renderer refused: expected helper bytes differ at RVA 0x11346d0"
    );
    image.put(0x11346d0, &[0x48]);
    assert!(admit(null(), &image));
    assert_eq!(image.log.borrow().len(), 3);
}

thread_local! {
    static SEEN: RefCell<Vec<(usize, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn record(
    address: *const c_uchar,
    expected: *const c_uchar,
    count: usize,
) -> bool {
    let bytes = unsafe { std::slice::from_raw_parts(expected, count) }.to_vec();
    SEEN.with(|seen| seen.borrow_mut().push((address.addr(), bytes)));
    true
}

unsafe extern "C" fn refuse_write(
    _: *mut c_uchar,
    _: *const c_uchar,
    _: *const c_uchar,
    _: usize,
) -> bool {
    false
}

#[test]
fn native_admission_compares_each_site_at_the_module_base() {
    let base = 0x1_4000_0000usize;
    let leaves = rsf_ac7_code_leaves {
        matches: Some(record),
        write: Some(refuse_write),
        log: None,
        user: null_mut(),
    };
    assert!(unsafe { rsf_ac7_build_admit(without_provenance(base), &leaves) });
    SEEN.with(|seen| {
        let seen = seen.borrow();
        assert_eq!(
            seen.len(),
            HOOK_SITES.len() + DEPTH_GATES.len() + HELPERS.len()
        );
        assert_eq!(seen[0], (base + 0x10b0220, HOOK_SITES[0].expected.to_vec()));
        let gate = &seen[HOOK_SITES.len() + 4];
        assert_eq!(gate, &(base + 0x11583a6, DEPTH_GATES[4].expected.to_vec()));
        assert_eq!(
            seen.last().map(|(address, _)| *address),
            Some(base + 0x11346d0)
        );
    });
    assert!(!unsafe { rsf_ac7_build_admit(without_provenance(base), null()) });
    let incomplete = rsf_ac7_code_leaves {
        write: None,
        ..leaves
    };
    assert!(!unsafe { rsf_ac7_build_admit(without_provenance(base), &incomplete) });
}
