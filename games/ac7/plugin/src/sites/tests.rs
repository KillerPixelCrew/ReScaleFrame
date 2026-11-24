// SPDX-License-Identifier: MIT
use super::*;
use rsf_ac7::sys::{RSF_AC7_PROCESS_HOOKS, RSF_AC7_ROLE_OUTPUT, RSF_AC7_ROLE_TONEMAP};
use std::collections::HashSet;

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
    assert!(!wanted(RSF_AC7_HOOK_SITE_COUNT, true));
    let always = (0..RSF_AC7_HOOK_SITE_COUNT)
        .filter(|&hook| wanted(hook, false))
        .count();
    assert_eq!(always, 59);
}
