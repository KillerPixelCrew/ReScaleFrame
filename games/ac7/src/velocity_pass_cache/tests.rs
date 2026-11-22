// SPDX-License-Identifier: MIT
use super::*;
use crate::shader_hash::crc32c;
use std::cell::Cell;
use std::ptr::{null, null_mut, without_provenance_mut};

fn address(value: usize) -> *mut c_void {
    without_provenance_mut(value)
}

/// A DXBC-sized blob whose header declares its whole length.
fn container(size: usize, fill: u8) -> Vec<u8> {
    let mut bytes = vec![fill; size];
    bytes[24..28].copy_from_slice(&(size as u32).to_le_bytes());
    bytes
}

fn element(semantic_index: u32, format: u32, input_slot: u32, byte_offset: u32) -> LayoutElement {
    LayoutElement {
        semantic_index,
        format,
        input_slot,
        byte_offset,
        per_instance: 0,
    }
}

fn beam(extra: bool) -> Vec<LayoutElement> {
    let mut elements = vec![
        element(0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0),
        element(1, DXGI_FORMAT_R32G32B32_FLOAT, 0, 16),
        element(2, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 32),
        element(3, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 64),
        element(4, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 48),
    ];
    if extra {
        elements.push(element(5, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0));
    }
    elements
}

const VS: usize = 0x100;
const PS: usize = 0x200;
const GS: usize = 0x300;
const HS: usize = 0x400;
const DS: usize = 0x500;
const LAYOUT: usize = 0x600;

fn observe(cache: &mut Cache, identity: usize, stage: u32, bytes: &[u8]) {
    cache.observe_shader(identity, stage, Some(Arc::new(bytes.to_vec())));
}

fn observed() -> Cache {
    let mut cache = Cache::default();
    for (identity, stage, fill) in [(VS, 0, 1), (PS, 1, 2), (GS, 2, 3), (HS, 3, 4), (DS, 4, 5)] {
        observe(&mut cache, identity, stage, &container(64, fill));
    }
    cache
}

fn request(shaders: [usize; STAGES], layout: usize, material_blend: u32) -> VariantRequest {
    VariantRequest {
        shaders: shaders.map(address),
        layout: address(layout),
        material_blend,
        stride: BEAM_STRIDE,
    }
}

#[test]
fn shader_hash_is_the_declared_container_crc() {
    let mut bytes = container(64, 7);
    bytes.extend_from_slice(&[9; 16]);
    let mut cache = Cache::default();
    observe(&mut cache, VS, 0, &bytes);
    let source = &cache.sources[&VS];
    assert_eq!(source.hash, crc32c(&bytes[..64]));
    assert_eq!(*source.bytes, bytes);
    assert_eq!(cache.source_bytes, 80);

    observe(&mut cache, PS, 1, &[1; 20]);
    assert_eq!(cache.sources[&PS].hash, 0);
}

#[test]
fn replacement_forgets_first_and_budget_refuses() {
    let mut cache = Cache::default();
    observe(&mut cache, VS, 0, &container(64, 1));
    observe(&mut cache, VS, 1, &container(32, 2));
    assert_eq!(cache.sources.len(), 1);
    assert_eq!(cache.sources[&VS].stage, 1);
    assert_eq!(cache.source_bytes, 32);

    cache.source_bytes = SOURCE_BUDGET - 32 + 1;
    observe(&mut cache, PS, 1, &container(32, 3));
    assert!(!cache.sources.contains_key(&PS));
    // The refused replacement still removes the old bytecode and its budget.
    observe(&mut cache, VS, 0, &container(64, 4));
    assert!(!cache.sources.contains_key(&VS));
    assert_eq!(cache.source_bytes, SOURCE_BUDGET - 64 + 1);
}

#[test]
fn layouts_forget_on_refusal_and_take_new_serials() {
    let mut cache = Cache::default();
    let elements = beam(false);
    cache.observe_layout(LAYOUT, Some(elements.as_slice()));
    assert_eq!(cache.layouts[&LAYOUT].serial, 1);
    cache.observe_layout(LAYOUT, Some(&[][..]));
    assert_eq!(cache.layouts[&LAYOUT].serial, 2);
    assert!(cache.layouts[&LAYOUT].elements.is_empty());
    cache.observe_layout(LAYOUT, None);
    assert!(cache.layouts.is_empty());

    for identity in 0..MAX_LAYOUTS {
        cache.observe_layout(0x10000 + identity, Some(elements.as_slice()));
    }
    cache.observe_layout(LAYOUT, Some(elements.as_slice()));
    assert!(!cache.layouts.contains_key(&LAYOUT));
    assert_eq!(cache.layout_serial, 2 + MAX_LAYOUTS as u64);

    cache.release();
    assert!(cache.layouts.is_empty());
    cache.observe_layout(LAYOUT, Some(elements.as_slice()));
    assert_eq!(cache.layouts[&LAYOUT].serial, 3 + MAX_LAYOUTS as u64);
}

#[test]
fn beam_layout_matches_only_the_factory_declaration() {
    assert!(beam_layout(BEAM_STRIDE, &beam(false)));
    assert!(beam_layout(BEAM_STRIDE, &beam(true)));
    let mut reordered = beam(true);
    reordered.reverse();
    assert!(beam_layout(BEAM_STRIDE, &reordered));

    assert!(!beam_layout(64, &beam(false)));
    assert!(!beam_layout(BEAM_STRIDE, &beam(false)[..4]));

    let mut duplicate = beam(false);
    duplicate.push(duplicate[2]);
    assert!(!beam_layout(BEAM_STRIDE, &duplicate));

    let mut missing = beam(true);
    missing[3] = element(5, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0);
    assert!(!beam_layout(BEAM_STRIDE, &missing));

    let mut wide = beam(false);
    wide[1].format = DXGI_FORMAT_R32G32B32A32_FLOAT;
    assert!(!beam_layout(BEAM_STRIDE, &wide));

    let mut instanced = beam(false);
    instanced[0].per_instance = 1;
    assert!(!beam_layout(BEAM_STRIDE, &instanced));

    let mut slot = beam(true);
    slot[5].input_slot = 0;
    assert!(!beam_layout(BEAM_STRIDE, &slot));

    let mut high = beam(false);
    high.push(element(6, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0));
    assert!(!beam_layout(BEAM_STRIDE, &high));
}

#[test]
fn unobserved_or_mismatched_stages_are_unavailable() {
    let state = Mutex::new(observed());
    let mut hash = 0;
    let build = |_: &VariantSources, _: &mut u32| -> i32 { panic!("no build expected") };
    for shaders in [
        [0, PS, 0, 0, 0],
        [VS, 0, 0, 0, 0],
        [PS, VS, 0, 0, 0],
        [VS, PS, 0x700, 0, 0],
        [VS, PS, HS, 0, 0],
        [VS, PS, 0, HS, 0],
        [VS, PS, 0, 0, DS],
        [VS, PS, 0, DS, HS],
    ] {
        assert_eq!(
            admit(&state, &request(shaders, 0, 0), &mut hash, build),
            NO_VARIANT
        );
    }
    assert!(lock(&state).variants.is_empty());
}

#[test]
fn built_variants_are_reused_and_refusals_cached() {
    let mut cache = observed();
    cache.observe_layout(LAYOUT, Some(beam(true).as_slice()));
    let vertex = cache.sources[&VS].hash;
    let state = Mutex::new(cache);
    let builds = Cell::new(0);
    let mut hash = 0;

    let first = request([VS, PS, 0, 0, 0], LAYOUT, 2);
    let slot = admit(&state, &first, &mut hash, |sources, slot| {
        builds.set(builds.get() + 1);
        assert!(sources.beam_layout);
        assert_eq!(sources.elements.len(), 6);
        assert_eq!(sources.bytes.map(<[u8]>::len), [64, 64, 0, 0, 0]);
        assert_eq!(sources.geometry_hash, 0);
        let lent = sources.bytes[PIXEL];
        assert_eq!(lent, container(64, 2));
        *slot = 3;
        BUILT
    });
    assert_eq!((slot, hash), (3, vertex));
    hash = 0;
    let again = admit(&state, &first, &mut hash, |_, _| panic!("reused"));
    assert_eq!((again, hash), (3, vertex));

    // Material blend is part of the identity, and a refusal is remembered.
    let additive = request([VS, PS, 0, 0, 0], LAYOUT, 4);
    let refuse = |_: &VariantSources, _: &mut u32| {
        builds.set(builds.get() + 1);
        REFUSED
    };
    assert_eq!(admit(&state, &additive, &mut hash, refuse), NO_VARIANT);
    assert_eq!(admit(&state, &additive, &mut hash, refuse), NO_VARIANT);
    assert_eq!(builds.get(), 2);
    assert_eq!(lock(&state).variants.len(), 2);

    // A new layout object at the same address takes a new serial and a new variant.
    lock(&state).observe_layout(LAYOUT, Some(beam(false).as_slice()));
    let rebuilt = admit(&state, &first, &mut hash, |sources, slot| {
        assert_eq!(sources.elements.len(), 5);
        *slot = 4;
        BUILT
    });
    assert_eq!(rebuilt, 4);
    assert_eq!(lock(&state).variants.len(), 3);
}

#[test]
fn unstored_outcomes_build_again() {
    let state = Mutex::new(observed());
    let mut hash = 0;
    let plain = request([VS, PS, 0, 0, 0], 0, 0);
    for outcome in [2, BUILT] {
        assert_eq!(admit(&state, &plain, &mut hash, |_, _| outcome), NO_VARIANT);
    }
    assert!(lock(&state).variants.is_empty());
    let slot = admit(&state, &plain, &mut hash, |sources, slot| {
        assert!(!sources.beam_layout);
        assert_eq!(sources.elements.len(), 0);
        *slot = 0;
        BUILT
    });
    assert_eq!(slot, 0);
}

#[test]
fn beam_classification_separates_variants_for_the_same_layout() {
    let mut cache = observed();
    cache.observe_layout(LAYOUT, Some(&beam(false)));
    let state = Mutex::new(cache);
    let mut hash = 0;
    let mut draw = request([VS, PS, 0, 0, 0], LAYOUT, 0);
    draw.stride = 64;
    assert_eq!(
        admit(&state, &draw, &mut hash, |source, _| {
            assert!(!source.beam_layout);
            REFUSED
        }),
        NO_VARIANT
    );
    draw.stride = BEAM_STRIDE;
    assert_eq!(
        admit(&state, &draw, &mut hash, |source, slot| {
            assert!(source.beam_layout);
            *slot = 0;
            BUILT
        }),
        0
    );
}

#[test]
fn builder_leases_payload_across_replacement() {
    let mut cache = observed();
    let bytes = cache.sources[&VS].bytes.as_ptr();
    let state = Mutex::new(cache);
    let draw = request([VS, PS, 0, 0, 0], 0, 0);
    let mut hash = 0;
    admit(&state, &draw, &mut hash, |source, _| {
        lock(&state).observe_shader(VS, VERTEX as u32, None);
        assert_eq!(source.bytes[VERTEX].as_ptr(), bytes);
        // SAFETY: The callback still holds the lease after the cache forgot the source.
        let lent = source.bytes[VERTEX];
        assert_eq!(lent, container(64, 1));
        REFUSED
    });
    assert_eq!(
        admit(&state, &draw, &mut hash, |_, _| panic!("old source")),
        NO_VARIANT
    );
}

#[test]
fn generated_geometry_history_identity() {
    let cache = observed();
    let (gs, hs, ds) = (
        cache.sources[&GS].hash,
        cache.sources[&HS].hash,
        cache.sources[&DS].hash,
    );
    let state = Mutex::new(cache);
    let mut hash = 0;
    admit(
        &state,
        &request([VS, PS, GS, 0, 0], 0, 0),
        &mut hash,
        |sources, slot| {
            assert_eq!(sources.geometry_hash, gs);
            assert_eq!(sources.bytes.map(<[u8]>::len), [64, 64, 64, 0, 0]);
            *slot = 0;
            BUILT
        },
    );
    admit(
        &state,
        &request([VS, PS, 0, HS, DS], 0, 0),
        &mut hash,
        |sources, slot| {
            assert_eq!(sources.geometry_hash, hs ^ (ds << 1) ^ (ds >> 31));
            assert_eq!(sources.bytes.map(<[u8]>::len), [64, 64, 0, 64, 64]);
            *slot = 1;
            BUILT
        },
    );
    assert_eq!(lock(&state).variants.len(), 2);
}

#[test]
fn reset_discards_slots_and_late_results() {
    let state = Mutex::new(observed());
    let mut hash = 0;
    let plain = request([VS, PS, 0, 0, 0], 0, 0);
    let slot = admit(&state, &plain, &mut hash, |_, slot| {
        // The native list is discarded while this build runs.
        lock(&state).reset_variants();
        *slot = 0;
        BUILT
    });
    assert_eq!(slot, NO_VARIANT);
    assert!(lock(&state).variants.is_empty());

    admit(&state, &plain, &mut hash, |_, slot| {
        *slot = 0;
        BUILT
    });
    lock(&state).reset_variants();
    let builds = Cell::new(0);
    admit(&state, &plain, &mut hash, |_, slot| {
        builds.set(1);
        *slot = 0;
        BUILT
    });
    assert_eq!(builds.get(), 1);

    lock(&state).release();
    assert_eq!(admit(&state, &plain, &mut hash, |_, _| BUILT), NO_VARIANT);
    let cache = lock(&state);
    assert!(cache.sources.is_empty() && cache.variants.is_empty() && cache.source_bytes == 0);
}

#[test]
fn ffi_rejects_null_arguments() {
    // Every call returns before it touches the process cache.
    unsafe {
        rsf_ac7_velocity_layout_seen(null_mut(), null(), 0, 0);
        assert_eq!(
            rsf_ac7_velocity_admit_variant(null(), null_mut(), None, null_mut()),
            NO_VARIANT
        );
    }
}
