// SPDX-License-Identifier: MIT
use super::*;
use rsf_ac7::sys::{RSF_AC7_PROCESS_HOOKS, RSF_AC7_ROLE_OUTPUT, RSF_AC7_ROLE_TONEMAP};

#[test]
fn hooks_are_unresolved_until_admitted_and_roles_follow_the_table() {
    // rsf-ac7's discovery tests resolve every hook to its recorded RVA.
    assert_eq!(rsf_ac7_hook_rva(19), 0);
    assert_eq!(rsf_ac7_hook_rva(RSF_AC7_HOOK_SITE_COUNT), 0);
    assert_eq!(rsf_ac7_hook_role(0), RSF_AC7_ROLE_TONEMAP);
    assert_eq!(rsf_ac7_hook_role(5), RSF_AC7_ROLE_OUTPUT);
    assert_eq!(rsf_ac7_hook_role(RSF_AC7_PROCESS_HOOKS), 0);
    assert_eq!(rsf_ac7_hook_role(u32::MAX), 0);
    assert_eq!(HOOKS, RSF_AC7_HOOK_SITE_COUNT as usize);
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
