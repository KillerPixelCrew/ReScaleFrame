// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn visual_lookup_distinguishes_absent_unique_and_ambiguous() {
    let registry = IdentityRegistry::default();
    let lookup = |visual| {
        let mut probe = Probe::default();
        rsf_anomaly_geometry_probe_visual(&registry, visual, &mut probe);
        (probe.lookup as i32, probe.key.generation)
    };
    assert_eq!(lookup(0x500), (RSF_ANOMALY_VISUAL_MISSING, 0));
    let first = rsf_anomaly_geometry_register(&registry, 1, 0x500);
    assert_ne!(first, 0);
    assert_eq!(lookup(0x500), (RSF_ANOMALY_VISUAL_UNIQUE, first));
    assert_eq!(lookup(0x4ff), (RSF_ANOMALY_VISUAL_MISSING, 0));
    assert_eq!(lookup(0x501), (RSF_ANOMALY_VISUAL_MISSING, 0));
    assert_ne!(rsf_anomaly_geometry_register(&registry, 2, 0x500), 0);
    assert_eq!(lookup(0x500), (RSF_ANOMALY_VISUAL_AMBIGUOUS, 0));
    assert_ne!(rsf_anomaly_geometry_retire(&registry, 2, 0x500), 0);
    assert_eq!(lookup(0x500), (RSF_ANOMALY_VISUAL_UNIQUE, first));
}

#[test]
fn classification_preserves_shadow_identity_and_deformation_precedence() {
    use crate::geometry_registry_abi::{
        RSF_ANOMALY_DRAW_IDENTIFIED as Identified,
        RSF_ANOMALY_DRAW_IGNORED_SHADOW as IgnoredShadow,
        RSF_ANOMALY_DRAW_IGNORED_STATIC_WORLD as IgnoredStaticWorld,
        RSF_ANOMALY_DRAW_UNIDENTIFIED_AMBIGUOUS as UnidentifiedAmbiguous,
        RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED as UnidentifiedDeformed,
        RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC as UnidentifiedDynamic,
        RSF_ANOMALY_DRAW_UNIDENTIFIED_NO_CAPTURE as UnidentifiedNoCapture,
        RSF_ANOMALY_DRAW_UNIDENTIFIED_UNREADABLE as UnidentifiedUnreadable,
        RSF_ANOMALY_PASS_MAIN as Main, RSF_ANOMALY_PASS_SHADOW as Shadow,
        RSF_ANOMALY_PASS_UNKNOWN as Unknown, RSF_ANOMALY_TYPE_PROGRESSIVE as Progressive,
        RSF_ANOMALY_TYPE_SKELETON_PM as SkeletonPM, RSF_ANOMALY_TYPE_SKELETON_ST as SkeletonST,
        RSF_ANOMALY_TYPE_STATIC as Static, RSF_ANOMALY_TYPE_TREE_PM as TreePM,
        RSF_ANOMALY_TYPE_TREE_ST as TreeST, RSF_ANOMALY_VISUAL_AMBIGUOUS as Ambiguous,
        RSF_ANOMALY_VISUAL_MISSING as Missing, RSF_ANOMALY_VISUAL_UNIQUE as Unique,
    };

    let cases = [
        (Main, Static, Unique, true, false, true, Identified),
        (Main, SkeletonPM, Unique, true, false, true, Identified),
        (Main, TreeST, Unique, true, false, true, Identified),
        (Shadow, Static, Unique, true, false, true, IgnoredShadow),
        (
            Shadow,
            Progressive,
            Missing,
            true,
            false,
            true,
            IgnoredShadow,
        ),
        (
            Shadow,
            SkeletonST,
            Missing,
            false,
            false,
            false,
            IgnoredShadow,
        ),
        (Main, Static, Missing, true, true, true, IgnoredStaticWorld),
        (
            Main,
            Progressive,
            Missing,
            true,
            true,
            true,
            IgnoredStaticWorld,
        ),
        (
            Main,
            Static,
            Missing,
            true,
            false,
            true,
            UnidentifiedDynamic,
        ),
        (
            Main,
            Progressive,
            Missing,
            true,
            false,
            true,
            UnidentifiedDynamic,
        ),
        (
            Main,
            Static,
            Missing,
            false,
            false,
            true,
            UnidentifiedUnreadable,
        ),
        (
            Main,
            SkeletonST,
            Missing,
            true,
            true,
            true,
            UnidentifiedDynamic,
        ),
        (
            Main,
            SkeletonPM,
            Missing,
            false,
            false,
            true,
            UnidentifiedDynamic,
        ),
        (
            Main,
            TreeST,
            Missing,
            true,
            true,
            true,
            UnidentifiedDeformed,
        ),
        (
            Main,
            TreePM,
            Missing,
            true,
            false,
            true,
            UnidentifiedDeformed,
        ),
        (
            Main,
            Static,
            Ambiguous,
            true,
            false,
            true,
            UnidentifiedAmbiguous,
        ),
        (
            Main,
            Static,
            Missing,
            true,
            true,
            false,
            UnidentifiedNoCapture,
        ),
        (
            Unknown,
            Static,
            Missing,
            true,
            true,
            true,
            IgnoredStaticWorld,
        ),
        (Unknown, Static, Unique, true, false, true, Identified),
    ];
    for (pass, visual_type, lookup, readable, identity, capturing, expected) in cases {
        let draw = Draw {
            pass,
            visual_type,
            lookup,
            capturing: u32::from(capturing),
            world_readable: u32::from(readable),
            world_identity: u32::from(identity),
            foliage_identity: 0,
        };
        assert_eq!(classify(&draw), expected);
    }
}
