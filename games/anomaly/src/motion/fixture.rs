// SPDX-License-Identifier: MIT
mod foliage;
mod mover;

use super::*;
use crate::geometry_registry_abi::RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC;
use crate::motion_profile::{MotionProfile, PROFILES};
use crate::native_geometry_abi::SOFTWARE_SKINNED;
use crate::shader_loader::{SHADER_CONTRACT_CASES, run_frame_fixture};
use std::ffi::CStr;

const CORE: &str = include_str!("../../shaders/core/README.md");
const FOLIAGE: &str = include_str!("../../shaders/foliage/README.md");
const CORE_VERTEX: &str = include_str!("../../shaders/core/rsf_core_vertex.h");
const CORE_PIXEL: &str = include_str!("../../shaders/core/rsf_core_pixel.h");
const TESS: &str = include_str!("../../shaders/core/rsf_core_tess.h");
const FOLIAGE_VERTEX: &str = include_str!("../../shaders/foliage/rsf_anomaly_foliage.h");

fn word(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn sample(seed: f64) -> Mat4 {
    Mat4(std::array::from_fn(|i| {
        if i < 12 {
            seed + 0.25 * i as f64 - if i % 3 == 0 { 1.5 } else { 0. }
        } else {
            if i == 15 { 1. } else { 0. }
        }
    }))
}

fn row_offset(text: &str, field: &str) -> usize {
    let line = text
        .lines()
        .find(|line| line.starts_with('|') && line.contains(field))
        .unwrap();
    line.split('|').nth(1).unwrap().trim().parse().unwrap()
}

fn size_before(text: &str, suffix: &str) -> usize {
    let (before, _) = text.split_once(suffix).unwrap();
    before.split_whitespace().last().unwrap().parse().unwrap()
}

fn pack_offset(text: &str, field: &str) -> usize {
    let (_, after) = text.split_once(field).unwrap();
    let (_, register) = after.split_once("packoffset(c").unwrap();
    register
        .split_once(')')
        .unwrap()
        .0
        .parse::<usize>()
        .unwrap()
        * 16
}

fn core_layout() {
    for (name, offset) in [
        ("`rsf_previous_wvp`", 0),
        ("`rsf_previous_bones[234]`", CORE_BONES),
        ("`rsf_history`", CORE_HISTORY),
        ("`rsf_viewport`", 0),
        ("`rsf_jitter`", 16),
        ("`rsf_pixel_control`", 32),
    ] {
        assert_eq!(row_offset(CORE, name), offset, "{name}");
    }
    assert_eq!(
        size_before(CORE, "-byte constant buffer to **VS b13"),
        CORE_BYTES
    );
    assert_eq!(
        size_before(CORE, "-byte constant buffer to **PS b13"),
        PIXEL_BYTES
    );
    assert_eq!(size_before(FOLIAGE, "-byte buffer"), FOLIAGE_BYTES);
    assert_eq!(
        row_offset(FOLIAGE, "uint4 rsf_foliage_history"),
        FOLIAGE_HISTORY
    );
    assert_eq!(
        row_offset(FOLIAGE, "float4 rsf_previous_item_state[61]") + 976,
        FOLIAGE_BYTES
    );
    for (text, field, offset) in [
        (CORE_VERTEX, "rsf_previous_wvp :", 0),
        (CORE_VERTEX, "rsf_previous_bones[234] :", CORE_BONES),
        (CORE_VERTEX, "rsf_history :", CORE_HISTORY),
        (CORE_PIXEL, "rsf_jitter :", 16),
        (CORE_PIXEL, "rsf_pixel_control :", 32),
    ] {
        assert_eq!(pack_offset(text, field), offset, "{field}");
    }
    assert!(CORE_VERTEX.contains("register(b13)") && CORE_PIXEL.contains("register(b13)"));
}

fn foliage_layout() {
    for (name, offset) in [
        ("row_major float4x4 rsf_previous_clip_from_world", 0),
        ("row_major float3x4 rsf_previous_tree_xform", 64),
        ("float4 rsf_previous_consts", 112),
        ("float4 rsf_previous_wave", 128),
        ("float4 rsf_previous_wind", 144),
        ("uint4 rsf_foliage_history", FOLIAGE_HISTORY),
        ("float4 rsf_previous_rows[244]", FOLIAGE_ROWS),
        ("float4 rsf_previous_item_state[61]", FOLIAGE_ITEM_STATE),
    ] {
        assert_eq!(row_offset(FOLIAGE, name), offset, "{name}");
    }
    assert_eq!(FOLIAGE_ROWS + FOLIAGE_MAX_GROUPS * 64, FOLIAGE_ITEM_STATE);
    assert_eq!(FOLIAGE_ITEM_STATE + FOLIAGE_MAX_GROUPS * 16, FOLIAGE_BYTES);
    for field in [
        "register(b13)",
        "rsf_previous_rows[61 * 4]",
        "rsf_previous_item_state[61]",
    ] {
        assert!(FOLIAGE_VERTEX.contains(field), "{field}");
    }
    assert_eq!(
        (
            FOLIAGE_MAX_GROUPS,
            FOLIAGE_MAX_GROUPS * 4,
            FOLIAGE_MAX_GROUPS * 16
        ),
        (61, 244, 976)
    );
}

fn core_bytes() {
    let wvp = sample(0.5);
    let bones = [sample(1.), sample(2.), sample(3.)];
    let mut out = MaybeUninit::uninit();
    let history = CoreHistory {
        draw: true,
        palette: true,
        rigid: false,
    };
    let pack = |wvp: &Mat4, bones: &[Mat4], out: &mut MaybeUninit<_>| unsafe {
        pack_core(wvp, bones.as_ptr(), bones.len(), &history, out).is_ok()
    };
    assert!(pack(&wvp, &bones, &mut out));
    let data = unsafe { out.assume_init_ref() };
    for (i, value) in wvp.0.iter().enumerate() {
        assert_eq!(word(data, i * 4), *value as f32);
    }
    for (bone, matrix) in bones.iter().enumerate() {
        for (i, value) in matrix.0[..12].iter().enumerate() {
            assert_eq!(word(data, CORE_BONES + bone * 48 + i * 4), *value as f32);
        }
    }
    assert_eq!(word(data, CORE_BONES + 9 * 16), 0.);
    assert_eq!(
        std::array::from_fn::<_, 4, _>(|i| word(data, CORE_HISTORY + i * 4)),
        [1., 1., 0., 1.]
    );
    let many = [sample(1.); CORE_MAX_BONES + 1];
    assert!(!pack(&wvp, &many, &mut out));
    let data = unsafe { out.assume_init_ref() };
    assert_eq!(
        [word(data, CORE_HISTORY), word(data, CORE_HISTORY + 12)],
        [0., 1.]
    );
    assert!(pack(&wvp, &many[..CORE_MAX_BONES], &mut out));
    assert_eq!(
        word(unsafe { out.assume_init_ref() }, CORE_BONES + 233 * 16 + 12),
        many[77].0[11] as f32
    );
    let mut bad = wvp;
    bad.0[5] = f64::NAN;
    assert!(!pack(&bad, &[], &mut out));
    rsf_anomaly_motion_pack_core_invalid(&mut out);
    assert!(
        unsafe { out.assume_init_ref() }[..CORE_HISTORY + 12]
            .iter()
            .all(|&v| v == 0)
    );
    let mut foliage = MaybeUninit::new([0x7f; FOLIAGE_BYTES]);
    rsf_anomaly_motion_pack_foliage_invalid(&mut foliage);
    assert!(unsafe { foliage.assume_init() }.iter().all(|&v| v == 0));
}

fn pixel() {
    let mut out = MaybeUninit::uninit();
    let viewport = [2., 3., 800., 400.];
    let jitter = [0.01, -0.02, 0.03, 0.04];
    assert!(rsf_anomaly_motion_pack_pixel(
        &PixelInputs { viewport, jitter },
        &mut out
    ));
    let bytes = unsafe { out.assume_init_ref() };
    assert_eq!(
        std::array::from_fn::<_, 4, _>(|i| word(bytes, 4 * i)),
        [2., 3., 1. / 800., 1. / 400.]
    );
    assert_eq!(
        std::array::from_fn::<_, 4, _>(|i| word(bytes, 16 + 4 * i)),
        jitter
    );
    assert_eq!(
        std::array::from_fn::<_, 4, _>(|i| word(bytes, 32 + 4 * i)),
        [1., 0., 0., 0.]
    );
    assert!(!rsf_anomaly_motion_pack_pixel(
        &PixelInputs {
            viewport: [0., 0., 0., 400.],
            jitter: [0.; 4]
        },
        &mut out
    ));
    assert_eq!(word(unsafe { out.assume_init_ref() }, 32), 0.);
    for invalid in [
        PixelInputs {
            viewport: [0., 0., 1., 1.],
            jitter: [f32::NAN, 0., 0., 0.],
        },
        PixelInputs {
            viewport: [0., 0., f32::from_bits(1), 400.],
            jitter: [0.; 4],
        },
    ] {
        assert!(!rsf_anomaly_motion_pack_pixel(&invalid, &mut out));
        assert_eq!(unsafe { out.assume_init_ref() }, &[0; PIXEL_BYTES]);
    }
}

fn matrix() {
    let world = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 5., 6., 7., 1.,
    ];
    let view = [
        0., 0., 1., 0., 0., 1., 0., 0., -1., 0., 0., 0., 1., 2., 3., 1.,
    ];
    let projection = [
        1.5, 0., 0., 0., 0., 2., 0., 0., 0., 0., 1.01, 1., 0., 0., -0.2, 0.,
    ];
    let w = Mat4(from_engine_matrix(&world));
    assert_eq!([w.0[3], w.0[7], w.0[11], w.0[15]], [5., 6., 7., 1.]);
    let wvp = Mat4(from_engine_matrix(&projection))
        .compose(&Mat4(from_engine_matrix(&view)))
        .compose(&w);
    let point = [0.3, -0.7, 2.0, 1.0];
    let mut reference = point;
    for m in [world, view, projection] {
        reference =
            std::array::from_fn(|c| (0..4).map(|k| reference[k] * f64::from(m[k * 4 + c])).sum());
    }
    for (row, prior) in wvp.0.chunks_exact(4).zip(reference) {
        let clip = row.iter().zip(point).map(|(a, b)| a * b).sum::<f64>();
        assert!((clip - prior).abs() < 1e-5);
    }
}

fn core_input(camera: &CameraSample) -> CoreDrawInput {
    CoreDrawInput {
        family: CORE_STATIC,
        skin: -2,
        in_visual: true,
        verdict: STATIC_WORLD,
        kind: RIGID,
        context: true,
        camera_conflict: false,
        previous_camera: camera,
        previous: Default::default(),
    }
}

fn draw_view(world: &Mat4, bones: &[Mat4]) -> DrawSnapshotView {
    DrawSnapshotView {
        world,
        bones: PaletteView {
            data: bones.as_ptr(),
            count: bones.len(),
        },
    }
}

fn decision(input: &CoreDrawInput) -> CoreDecision {
    unsafe { decide_core(input) }.unwrap_or_else(|reason| CoreDecision {
        reason,
        ..Default::default()
    })
}

fn core_policy() {
    use HistoryReason::*;
    let camera = CameraSample {
        view: sample(0.1),
        projection: sample(0.2),
    };
    let vp = camera.projection.compose(&camera.view);
    let d = decision(&core_input(&camera));
    assert!(d.reason == Valid && d.flags.draw && !d.flags.palette && d.previous_wvp == vp);
    let variants: [(HistoryReason, fn(&mut CoreDrawInput)); 6] = [
        (NoVisual, |r| r.in_visual = false),
        (Unidentified, |r| {
            r.verdict = RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC as u8
        }),
        (NoPreviousCamera, |r| r.previous_camera = ptr::null()),
        (CameraConflict, |r| r.camera_conflict = true),
        (NoContext, |r| r.context = false),
        (NoPreviousInstance, |r| r.verdict = IDENTIFIED),
    ];
    for (expected, edit) in variants {
        let mut input = core_input(&camera);
        edit(&mut input);
        assert!(decision(&input).reason == expected);
    }
    let world = sample(3.);
    let mut input = core_input(&camera);
    input.verdict = IDENTIFIED;
    input.previous = draw_view(&world, &[]);
    let d = decision(&input);
    assert!(d.reason == Valid && d.previous_wvp == vp.compose(&world));
    input.family = CORE_MODEL;
    input.skin = -1;
    let d = decision(&input);
    assert!(d.reason == Valid && d.flags.rigid);
    input.skin = 2;
    assert!(decision(&input).reason == KindMismatch);
    let bones = [sample(4.), sample(5.)];
    input.kind = HARDWARE_SKINNED;
    input.previous = draw_view(&world, &bones);
    let d = decision(&input);
    assert!(d.reason == Valid && d.flags.palette && !d.flags.rigid && d.bones.count == 2);
    input.skin = 0;
    assert!(decision(&input).reason == KindMismatch);
    input.kind = SINGLE_BONE;
    let single = sample(6.);
    input.previous = draw_view(&single, &[]);
    let d = decision(&input);
    assert!(
        d.reason == Valid
            && d.flags.draw
            && !d.flags.palette
            && !d.flags.rigid
            && d.bones.count == 0
    );
    assert!(d.previous_wvp == vp.compose(&single));
    for (family, skin) in [(CORE_MODEL, 2), (CORE_MODEL, -1), (CORE_STATIC, -2)] {
        input.family = family;
        input.skin = skin;
        assert!(decision(&input).reason == KindMismatch);
    }
    input.family = CORE_MODEL;
    input.skin = 0;
    input.previous = DrawSnapshotView::default();
    assert!(decision(&input).reason == NoPreviousInstance);
    input.verdict = STATIC_WORLD;
    assert!(decision(&input).reason == KindMismatch);
    input.verdict = IDENTIFIED;
    input.kind = SOFTWARE_SKINNED;
    input.skin = -1;
    assert!(decision(&input).reason == UnsupportedKind);
    let many = [sample(1.); CORE_MAX_BONES + 1];
    input.kind = HARDWARE_SKINNED;
    input.skin = 1;
    input.previous = draw_view(&world, &many);
    assert!(decision(&input).reason == TooManyBones);
}

fn tess_layout() {
    for (field, offset) in [
        ("rsf_previous_wv :", 0),
        ("rsf_previous_projection :", 64),
        ("rsf_tess_history :", TESS_HISTORY),
    ] {
        assert_eq!(pack_offset(TESS, field), offset);
    }
    assert_eq!(TESS_HISTORY + 16, TESS_BYTES);
    assert!(TESS.contains("RSFTessPrevious : register(b13)"));
    let (wv, projection) = (sample(0.5), sample(1.5));
    let mut out = MaybeUninit::uninit();
    assert!(rsf_anomaly_motion_pack_tess(
        &wv,
        &projection,
        true,
        &mut out
    ));
    let bytes = unsafe { out.assume_init_ref() };
    for (i, value) in wv.0.into_iter().chain(projection.0).enumerate() {
        assert_eq!(word(bytes, i * 4), value as f32);
    }
    assert_eq!(
        std::array::from_fn::<_, 4, _>(|i| word(bytes, TESS_HISTORY + i * 4)),
        [1., 0., 0., 1.]
    );
    assert!(!rsf_anomaly_motion_pack_tess(
        &wv,
        &projection,
        false,
        &mut out
    ));
    let bytes = unsafe { out.assume_init_ref() };
    assert_eq!(
        [
            word(bytes, 0),
            word(bytes, TESS_HISTORY),
            word(bytes, TESS_HISTORY + 12)
        ],
        [0., 0., 1.]
    );
    let mut bad = projection;
    bad.0[10] = f64::NAN;
    assert!(!rsf_anomaly_motion_pack_tess(&wv, &bad, true, &mut out));
    assert_eq!(word(unsafe { out.assume_init_ref() }, TESS_HISTORY), 0.);
    out = MaybeUninit::new([0x7f; TESS_BYTES]);
    rsf_anomaly_motion_pack_tess_invalid(&mut out);
    assert!(
        unsafe { out.assume_init_ref() }[..TESS_HISTORY + 12]
            .iter()
            .all(|&v| v == 0)
    );
}

fn tess_policy() {
    let camera = CameraSample {
        view: sample(0.1),
        projection: sample(0.2),
    };
    let world = sample(0.7);
    let bones = [sample(1.), sample(2.)];
    let input = CoreDrawInput {
        family: CORE_MODEL,
        skin: 2,
        verdict: IDENTIFIED,
        kind: HARDWARE_SKINNED,
        previous: draw_view(&world, &bones),
        ..core_input(&camera)
    };
    let d = decision(&input);
    assert!(d.reason == HistoryReason::Valid);
    assert!(
        d.previous_wv == camera.view.compose(&world) && d.previous_projection == camera.projection
    );
    let reconstructed = camera.projection.compose(&d.previous_wv);
    assert!(
        reconstructed
            .0
            .iter()
            .zip(d.previous_wvp.0)
            .all(|(a, b)| (a - b).abs() <= 1e-9 * (1. + b.abs()))
    );
    let input = CoreDrawInput {
        kind: SINGLE_BONE,
        skin: 0,
        previous: draw_view(&world, &[]),
        ..input
    };
    let single = decision(&input);
    assert!(
        single.reason == HistoryReason::Valid && single.previous_wv == camera.view.compose(&world)
    );
}

fn frame(source: u64, generation: u64, width: u32, height: u32) -> FrameIdentity {
    FrameIdentity {
        source,
        generation,
        width,
        height,
    }
}

fn snapshot(world: Mat4, bones: &[Mat4]) -> Snapshot<'_> {
    Snapshot {
        world,
        kind: if bones.is_empty() {
            RIGID
        } else {
            HARDWARE_SKINNED
        },
        palette_layout: u64::from(u8::from(!bones.is_empty())),
        bones: Cow::Borrowed(bones),
        deformation: None,
        detail: None,
    }
}

fn history() {
    let mut history = MotionHistoryState::default();
    let key = InstanceKey {
        owner: 1,
        visual: 2,
        generation: 3,
    };
    let a = CameraSample {
        view: sample(0.1),
        projection: sample(0.2),
    };
    let b = CameraSample {
        view: sample(0.3),
        projection: sample(0.4),
    };
    let original = sample(1.);
    let first = frame(10, 0, 640, 360);
    assert!(!history.begin(0, first));
    assert!(matches!(history.observe(0, 0, &a), CameraStatus::Recorded));
    assert!(matches!(history.observe(0, 0, &a), CameraStatus::Same));
    assert!(history.record(0, key, 0, snapshot(original, &[])).is_none());
    assert!(!history.begin(0, first));
    assert!(history.begin(0, frame(11, 0, 640, 360)));
    assert!(history.previous_camera(0, 0) == Some(&a));
    assert!(history.previous_camera(0, 1).is_none() && history.previous_camera(1, 0).is_none());

    let mut moved = original;
    moved.0[3] = 9.;
    let previous = history
        .record(0, key, 0, snapshot(moved, &[]))
        .expect("previous instance");
    assert!(previous.world == original);
    let previous = std::ptr::from_ref(previous);
    assert!(std::ptr::eq(
        previous,
        history.record(0, key, 0, snapshot(moved, &[])).unwrap()
    ));
    let mut conflicting = moved;
    conflicting.0[7] = 4.;
    assert!(
        history
            .record(0, key, 0, snapshot(conflicting, &[]))
            .is_none()
            && history.conflicts == 1
    );
    assert!(matches!(history.observe(0, 0, &a), CameraStatus::Recorded));
    assert!(matches!(history.observe(0, 0, &b), CameraStatus::Conflict));
    history.begin(0, frame(12, 0, 640, 360));
    assert!(history.record(0, key, 0, snapshot(moved, &[])).is_none());
    assert!(history.previous_camera(0, 0).is_none());
    history.begin(0, frame(13, 0, 640, 360));
    assert!(history.record(0, key, 0, snapshot(moved, &[])).is_some());

    let bones = [sample(2.)];
    for id in [
        frame(14, 0, 640, 360),
        frame(15, 0, 1280, 720),
        frame(16, 1, 1280, 720),
    ] {
        history.begin(0, id);
        assert!(history.record(0, key, 0, snapshot(moved, &bones)).is_none());
    }
    history.begin(0, frame(17, 1, 1280, 720));
    assert!(history.record(0, key, 0, snapshot(moved, &bones)).is_some());
    history.retire(key);
    history.begin(0, frame(18, 1, 1280, 720));
    assert!(history.record(0, key, 0, snapshot(moved, &bones)).is_none());
    let next = InstanceKey {
        generation: 4,
        ..key
    };
    assert!(
        history
            .record(0, next, 0, snapshot(moved, &bones))
            .is_none()
    );
    history.begin(0, frame(19, 1, 1280, 720));
    assert!(
        history
            .record(0, next, 0, snapshot(moved, &bones))
            .is_some()
    );
    assert!(!history.begin(0, frame(5, 1, 1280, 720)));
}

fn far_diagnostics() {
    for (distance, band) in [
        (0., 0),
        (19.99, 0),
        (20., 1),
        (49.9, 1),
        (50., 2),
        (99.9, 2),
        (100., 3),
        (149.9, 3),
        (150., 4),
        (5000., 4),
        (f64::NAN, 5),
        (-1., 5),
    ] {
        assert_eq!(rsf_anomaly_motion_distance_band(distance), band);
    }
    for (band, name) in [(2, c"50-100m"), (5, c"unknown"), (99, c"unknown")] {
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(rsf_anomaly_motion_distance_band_name(band)) },
            name
        );
    }
    let mut view = Mat4([
        0., 0., -1., -5., 0., 1., 0., -2., 1., 0., 0., -10., 0., 0., 0., 1.,
    ]);
    let mut world = Mat4::IDENTITY;
    world.0[3] = 130.;
    world.0[7] = 2.;
    world.0[11] = -5.;
    let distance = rsf_anomaly_motion_camera_distance(&view, &world);
    assert!((distance - 120.).abs() < 1e-9);
    assert_eq!(rsf_anomaly_motion_distance_band(distance), 3);
    view.0[14] = 1.;
    assert!(rsf_anomaly_motion_camera_distance(&view, &world).is_nan());
    use ModelElement::*;
    for (root, expected) in [
        (b"deffer_model_bump-hq_2.vs".as_slice(), High),
        (b"deffer_model_bump_d-hq_1.vs", High),
        (b"deffer_model_bump_2.vs", Low),
        (b"deffer_model_flat_4.vs", Low),
        (b"deffer_model_bump(tess_pn,)_3.vs", Tessellated),
        (b"deffer_base_bump.vs", Unknown),
        (b"model_env_lq_0.vs", Unknown),
    ] {
        assert_eq!(
            unsafe { rsf_anomaly_motion_model_element(root.as_ptr(), root.len()) } as u8,
            expected as u8
        );
    }
    let old = [Mat4::IDENTITY; 4];
    let mut current = old;
    let mut world = Mat4::IDENTITY;
    let previous = draw_view(&Mat4::IDENTITY, &old);
    let stale = |world: &Mat4, bones: &[Mat4]| unsafe {
        rsf_anomaly_motion_stale_palette(&previous, &draw_view(world, bones), 0.001)
    };
    assert!(!stale(&world, &current));
    world.0[3] = 0.05;
    assert!(stale(&world, &current));
    world.0[3] = 0.0005;
    assert!(!stale(&world, &current));
    world.0[3] = 0.05;
    current[2].0[7] = 0.01;
    assert!(!stale(&world, &current));
    assert!(!stale(&world, &current[..3]));
    assert!(!stale(&world, &[]));
}

fn binder() {
    crate::motion_target::run_binder_fixture().expect("RTV2 binder contract");
}

const MOTION_CASES: &[(&str, fn())] = &[
    ("binder", binder),
    ("core-vertex", core_bytes),
    ("far-character", far_diagnostics),
    ("foliage-identity", foliage::identity),
    ("foliage-layout-readme", foliage_layout),
    ("foliage-packing", foliage::packing),
    ("foliage-policy", foliage::policy),
    ("history", history),
    ("layouts", core_layout),
    ("matrix", matrix),
    ("mover", mover::exercise),
    ("pixel", pixel),
    ("policy", core_policy),
    ("tess-layout", tess_layout),
    ("tess-policy", tess_policy),
];

#[derive(Clone, Copy)]
enum Case {
    Rust(fn()),
    Frame,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_contract(selection: *const c_char) -> i32 {
    let [standard, avx] = PROFILES.each_ref().map(MotionProfile::frame_layout);
    let mut cases = MOTION_CASES
        .iter()
        .chain(SHADER_CONTRACT_CASES)
        .map(|&(name, body)| (name, Case::Rust(body)))
        .chain(std::iter::once(("frame", Case::Frame)));
    let run = |name: &str, body| {
        let result = std::panic::catch_unwind(|| match body {
            Case::Rust(body) => body(),
            Case::Frame => {
                run_frame_fixture(&standard, &avx).expect("compiler caller-frame contract")
            }
        });
        if result.is_err() {
            eprintln!("FAILED {name}");
        }
        i32::from(result.is_err())
    };
    if !selection.is_null() {
        let selection = unsafe { CStr::from_ptr(selection) }.to_string_lossy();
        if let Some((name, body)) = cases.find(|(name, _)| *name == selection.as_ref()) {
            return run(name, body);
        }
        eprintln!("unknown case {selection}");
        return 2;
    }
    let mut all: Vec<_> = cases.collect();
    all.sort_unstable_by_key(|&(name, _)| name);
    for &(name, body) in &all {
        if run(name, body) != 0 {
            return 1;
        }
    }
    println!("anomaly motion contract: {} cases passed", all.len());
    0
}
