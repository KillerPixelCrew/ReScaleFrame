// SPDX-License-Identifier: MIT
use super::*;
use crate::shader_hash::crc32c;
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

/// The identity and lease of a request the device owner does not hold yet.
fn build(cache: &Cache, request: &VariantRequest) -> (VariantKey, Lease) {
    match cache.prepare(request, |_| false) {
        Prepared::Build(key, lease) => (key, lease),
        _ => panic!("expected a build"),
    }
}

#[test]
fn unobserved_or_mismatched_stages_are_unavailable() {
    let cache = observed();
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
        let unavailable = cache.prepare(&request(shaders, 0, 0), |_| panic!("no lookup expected"));
        assert!(matches!(unavailable, Prepared::Unavailable));
    }
}

#[test]
fn identity_covers_blend_layout_serial_and_lends_the_sources() {
    let mut cache = observed();
    cache.observe_layout(LAYOUT, Some(beam(true).as_slice()));
    let vertex = cache.sources[&VS].hash;
    let first = request([VS, PS, 0, 0, 0], LAYOUT, 2);
    let (key, lease) = build(&cache, &first);
    let sources = lease.sources();
    assert!(sources.beam_layout);
    assert_eq!(sources.elements.len(), 6);
    assert_eq!(sources.bytes.map(<[u8]>::len), [64, 64, 0, 0, 0]);
    assert_eq!((sources.vertex_hash, sources.geometry_hash), (vertex, 0));
    assert_eq!(sources.bytes[PIXEL], container(64, 2));
    // A held identity is not leased again.
    assert!(
        matches!(cache.prepare(&first, |held| *held == key), Prepared::Known(known) if known == key)
    );

    // Material blend is part of the identity.
    let (additive, _) = build(&cache, &request([VS, PS, 0, 0, 0], LAYOUT, 4));
    assert!(additive != key);
    // A new layout object at the same address takes a new serial and a new identity.
    cache.observe_layout(LAYOUT, Some(beam(false).as_slice()));
    let (rebuilt, lease) = build(&cache, &first);
    assert!(rebuilt != key);
    assert_eq!(lease.sources().elements.len(), 5);
}

#[test]
fn beam_classification_separates_variants_for_the_same_layout() {
    let mut cache = observed();
    cache.observe_layout(LAYOUT, Some(&beam(false)));
    let mut draw = request([VS, PS, 0, 0, 0], LAYOUT, 0);
    draw.stride = 64;
    let (narrow, lease) = build(&cache, &draw);
    assert!(!lease.sources().beam_layout);
    draw.stride = BEAM_STRIDE;
    let (wide, lease) = build(&cache, &draw);
    assert!(lease.sources().beam_layout);
    assert!(narrow != wide);
}

#[test]
fn builder_leases_payload_across_replacement() {
    let mut cache = observed();
    let bytes = cache.sources[&VS].bytes.as_ptr();
    let draw = request([VS, PS, 0, 0, 0], 0, 0);
    let (_, lease) = build(&cache, &draw);
    cache.observe_shader(VS, VERTEX as u32, None);
    let sources = lease.sources();
    assert_eq!(sources.bytes[VERTEX].as_ptr(), bytes);
    assert_eq!(sources.bytes[VERTEX], container(64, 1));
    assert!(matches!(
        cache.prepare(&draw, |_| false),
        Prepared::Unavailable
    ));
}

#[test]
fn generated_geometry_history_identity() {
    let cache = observed();
    let (gs, hs, ds) = (
        cache.sources[&GS].hash,
        cache.sources[&HS].hash,
        cache.sources[&DS].hash,
    );
    let (_, lease) = build(&cache, &request([VS, PS, GS, 0, 0], 0, 0));
    assert_eq!(lease.sources().geometry_hash, gs);
    assert_eq!(lease.sources().bytes.map(<[u8]>::len), [64, 64, 64, 0, 0]);
    let (_, lease) = build(&cache, &request([VS, PS, 0, HS, DS], 0, 0));
    assert_eq!(lease.sources().geometry_hash, hs ^ (ds << 1) ^ (ds >> 31));
    assert_eq!(lease.sources().bytes.map(<[u8]>::len), [64, 64, 0, 64, 64]);
}

#[test]
fn ffi_rejects_null_arguments() {
    // The call returns before it touches the process cache.
    unsafe { rsf_ac7_velocity_layout_seen(null_mut(), null(), 0, 0) };
}
