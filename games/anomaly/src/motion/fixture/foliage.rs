// SPDX-License-Identifier: MIT
use super::*;
use crate::motion::foliage::*;
use crate::sys::RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED;

fn group([x, y, z]: [f32; 3], scale: f32, sun: f32, hemi: f32) -> [f32; 16] {
    [
        scale, 0., 0., x, 0., scale, 0., y, 0., 0., scale, z, sun, sun, sun, hemi,
    ]
}

fn batch(groups: &[[f32; 16]]) -> Rows {
    let mut out = MaybeUninit::uninit();
    rsf_anomaly_foliage_poison(&mut out);
    let mut out = unsafe { out.assume_init() };
    out[..groups.len()].copy_from_slice(groups);
    out
}

fn wind(time: f32, x: f32) -> FoliageConstants {
    FoliageConstants {
        consts: [1. / 16384., 1. / 16384., 0.5, 0.25],
        wave: [0.2, 0.14, 0.33, time],
        wind: [x, 0., 0.5, 0.],
    }
}

fn tree(constants: FoliageConstants, lod: i64) -> TreeRecord {
    TreeRecord {
        xform: [1., 0., 0., 5., 0., 1., 0., 6., 0., 0., 1., 7.],
        constants,
        lod,
    }
}

fn lanes<const N: usize>(bytes: &[u8], offset: usize) -> [f32; N] {
    std::array::from_fn(|i| word(bytes, offset + i * 4))
}

fn history_lanes(bytes: &[u8]) -> [u32; 4] {
    lanes::<4>(bytes, FOLIAGE_HISTORY).map(f32::to_bits)
}

fn pack_detail(
    clip: &Mat4,
    previous: &FoliageConstants,
    items: &[FoliageItem],
    live: u32,
    out: &mut MaybeUninit<[u8; FOLIAGE_BYTES]>,
) -> bool {
    unsafe {
        rsf_anomaly_motion_pack_detail(clip, previous, items.as_ptr(), items.len(), live, out)
    }
}

// The fixture adds native NDC jitter, then reads the packer's or policy's output.
fn add_jitter(projection: &Mat4, u: f64, v: f64) -> Mat4 {
    let mut result = *projection;
    for i in 0..4 {
        result.0[i] += 2. * u * projection.0[12 + i];
        result.0[4 + i] -= 2. * v * projection.0[12 + i];
    }
    result
}

pub(super) fn packing() {
    let clip = sample(0.5);
    let xform = std::array::from_fn(|i| (i + 1) as f32);
    let previous = wind(3.5, 0.75);
    let mut out = MaybeUninit::uninit();
    assert!(rsf_anomaly_motion_pack_tree(
        &clip, &xform, &previous, &mut out
    ));
    let bytes = unsafe { out.assume_init_ref() };
    assert_eq!(lanes::<16>(bytes, 0), clip.0.map(|value| value as f32));
    assert_eq!(lanes::<12>(bytes, 64), xform);
    assert_eq!(lanes::<4>(bytes, 112), previous.consts);
    assert_eq!(lanes::<4>(bytes, 128), previous.wave);
    assert_eq!(lanes::<4>(bytes, 144), previous.wind);
    assert_eq!(history_lanes(bytes), [1, 0, 1, 0]);
    assert!(bytes[FOLIAGE_ROWS..].iter().all(|&v| v == 0));
    let mut bad_xform = xform;
    bad_xform[5] = f32::NAN;
    assert!(!rsf_anomaly_motion_pack_tree(
        &clip, &bad_xform, &previous, &mut out
    ));
    assert_eq!(history_lanes(unsafe { out.assume_init_ref() }), [0; 4]);
    let mut bad_wind = previous;
    bad_wind.wind[2] = f32::INFINITY;
    assert!(!rsf_anomaly_motion_pack_tree(
        &clip, &xform, &bad_wind, &mut out
    ));

    let mut items = [FoliageItem::default(); FOLIAGE_MAX_GROUPS];
    let one = group([10., 20., 30.], 0.5, 0.3, 0.7);
    let three = group([-4., 5., 6.], 1.5, 0.2, 0.9);
    items[1] = FoliageItem {
        valid: true,
        scale: 0.5,
        fade: 1.,
        rows: one,
    };
    items[3] = FoliageItem {
        valid: true,
        scale: 1.5,
        fade: 0.5,
        rows: three,
    };
    assert!(pack_detail(&clip, &previous, &items, 5, &mut out));
    let bytes = unsafe { out.assume_init_ref() };
    assert_eq!(history_lanes(bytes), [1, 5, 1, 0]);
    assert_eq!(lanes::<16>(bytes, FOLIAGE_ROWS + 64), one);
    assert_eq!(lanes::<16>(bytes, FOLIAGE_ROWS + 3 * 64), three);
    assert_eq!(
        lanes::<4>(bytes, FOLIAGE_ITEM_STATE + 16),
        [1., 0.5, 1., 0.]
    );
    assert_eq!(
        [
            word(bytes, FOLIAGE_ITEM_STATE),
            word(bytes, FOLIAGE_ROWS),
            word(bytes, FOLIAGE_ITEM_STATE + 4 * 16)
        ],
        [0.; 3]
    );
    items[3].fade = 1.5;
    assert!(pack_detail(&clip, &previous, &items, 5, &mut out));
    let bytes = unsafe { out.assume_init_ref() };
    assert_eq!(
        [
            word(bytes, FOLIAGE_ITEM_STATE + 3 * 16),
            word(bytes, FOLIAGE_ROWS + 3 * 64)
        ],
        [0.; 2]
    );
    assert!(!pack_detail(&clip, &previous, &items, 62, &mut out));

    let projection = Mat4([
        1.2, 0., 0., 0., 0., 2.1, 0., 0., 0., 0., 1.001, -0.1, 0., 0., 1., 0.,
    ]);
    let (u, v) = (0.003f32, -0.002f32);
    let restored = add_jitter(&projection, f64::from(u), f64::from(v)).remove_jitter(u, v);
    assert!(
        restored
            .0
            .iter()
            .zip(projection.0)
            .all(|(a, b)| (a - b).abs() < 1e-12)
    );
    assert!(projection.remove_jitter(0., 0.) == projection);
}

fn detail(
    history: &mut History,
    view: u8,
    route: u64,
    constants: &FoliageConstants,
    rows: &Rows,
) -> DetailRemap {
    let mut out = MaybeUninit::uninit();
    rsf_anomaly_foliage_detail(Some(history), view, route, constants, rows, &mut out);
    unsafe { out.assume_init() }
}

pub(super) fn identity() {
    let mut history = History::default();
    let (route, other_route) = (0x1111, 0x2222);
    let (w0, w1) = (wind(1., 0.5), wind(1.1, 0.6));
    let (p1, p2, p3, p4, p5) = (
        group([1., 0., 1.], 1., 0.1, 0.5),
        group([2., 0., 2.], 0.9, 0.1, 0.5),
        group([3., 0., 3.], 0.8, 0.2, 0.6),
        group([4., 0., 4.], 0.7, 0.2, 0.6),
        group([5., 0., 5.], 0.6, 0.3, 0.7),
    );
    let first = batch(&[p1, p2, p3]);
    assert!(!history.begin(0, frame(10, 0, 640, 360)));
    let remap = detail(&mut history, 0, route, &w0, &first);
    assert_eq!(
        (
            remap.live,
            remap.matched,
            remap.unmatched,
            remap.route_found
        ),
        (3, 0, 3, false)
    );
    detail(&mut history, 0, route, &w0, &batch(&[p4, p5]));
    assert_eq!(rsf_anomaly_foliage_recorded(Some(&history), 0), 5);

    assert!(history.begin(0, frame(11, 0, 640, 360)));
    let next = batch(&[
        group([5., 0., 5.], 0.65, 0.3, 0.7),
        group([9., 0., 9.], 1., 0.4, 0.4),
        group([1., 0., 1.], 0.95, 0.1, 0.5),
        group([3., 0., 3.], 0.75, 0.2, 0.6),
    ]);
    let remap = detail(&mut history, 0, route, &w1, &next);
    assert_eq!(
        (
            remap.live,
            remap.matched,
            remap.unmatched,
            remap.route_found
        ),
        (4, 3, 1, true)
    );
    assert!(remap.previous == w0);
    assert!(remap.items[0].valid && remap.items[0].rows == p5 && remap.items[0].scale == 0.6);
    assert!(!remap.items[1].valid);
    assert!(remap.items[2].valid && remap.items[2].rows == p1 && remap.items[2].scale == 1.);
    assert!(remap.items[3].valid && remap.items[3].rows == p3);
    assert!(remap.items[2].fade == 1. && !remap.items[4].valid);
    let mut out = MaybeUninit::uninit();
    assert!(pack_detail(
        &sample(0.2).compose(&sample(0.1)),
        &remap.previous,
        &remap.items,
        remap.live,
        &mut out
    ));
    let bytes = unsafe { out.assume_init_ref() };
    assert_eq!(lanes::<16>(bytes, FOLIAGE_ROWS + 2 * 64), p1);
    assert_eq!(word(bytes, FOLIAGE_ITEM_STATE + 16), 0.);

    assert!(history.begin(0, frame(12, 0, 640, 360)));
    let moved = batch(&[group([5., 0., 5.], 0.66, 0.3, 0.7)]);
    let foreign = batch(&[group([50., 0., 50.], 1., 0.1, 0.1)]);
    let remap = detail(&mut history, 0, other_route, &w1, &foreign);
    assert!(!remap.route_found && remap.matched == 0);
    let remap = detail(&mut history, 0, route, &w1, &moved);
    assert!(remap.route_found && remap.matched == 1);
    assert_eq!(detail(&mut history, 0, route, &w1, &moved).conflicts, 0);
    let changed = batch(&[group([5., 0., 5.], 0.5, 0.3, 0.7)]);
    let remap = detail(&mut history, 0, route, &w1, &changed);
    assert!(remap.conflicts == 1 && rsf_anomaly_foliage_conflicts(Some(&history)) == 1);
    assert!(remap.matched == 0 && !remap.items[0].valid);
    assert!(history.begin(0, frame(13, 0, 640, 360)));
    let remap = detail(&mut history, 0, route, &w1, &moved);
    assert!(remap.route_found && remap.matched == 0);
    let remap = detail(&mut history, 0, other_route, &w1, &foreign);
    assert!(remap.route_found && remap.matched == 1);
    assert_eq!(detail(&mut history, 0, route, &w1, &batch(&[])).live, 0);
    let mut garbage = batch(&[group([7., 0., 7.], 1., 0.1, 0.1)]);
    garbage[1][5] = f32::NAN;
    assert_eq!(rsf_anomaly_foliage_count(&garbage), 1);
    assert_eq!(
        rsf_anomaly_foliage_scale(&group([0.; 3], 0.75, 0., 0.)),
        0.75
    );

    assert!(history.begin(0, frame(14, 0, 640, 360)));
    assert!(detail(&mut history, 0, route, &w1, &moved).route_found);
    assert!(!history.begin(0, frame(15, 0, 1280, 720)));
    assert!(!detail(&mut history, 0, route, &w1, &moved).route_found);
    assert!(history.begin(0, frame(16, 0, 1280, 720)));
    assert!(!history.begin(0, frame(17, 1, 1280, 720)));
    let split = frame(18, 1, 1280, 720);
    assert!(history.begin(0, split) && history.begin(0, split));
    assert!(!history.begin(0, frame(3, 1, 1280, 720)));
    assert!(!rsf_anomaly_foliage_active(Some(&history), 1));
    history.begin(1, frame(20, 0, 320, 180));
    assert_eq!(detail(&mut history, 1, route, &w1, &moved).live, 1);
    assert!(!detail(&mut history, 1, route, &w1, &moved).route_found);
    rsf_anomaly_foliage_reset(Some(&mut history));
    assert!(!rsf_anomaly_foliage_active(Some(&history), 0));

    history.begin(0, frame(21, 1, 640, 360));
    detail(&mut history, 0, route, &w0, &first);
    history.begin(0, frame(22, 1, 640, 360));
    let duplicate = batch(&[
        group([1., 0., 1.], 0.5, 0.1, 0.5),
        group([1., 0., 1.], 0.6, 0.1, 0.5),
    ]);
    let remap = detail(&mut history, 0, route, &w1, &duplicate);
    assert_eq!((remap.conflicts, remap.matched, remap.unmatched), (1, 0, 2));
    let remap = detail(&mut history, 0, route, &w0, &first);
    assert_eq!(
        (remap.route_found, remap.matched, remap.unmatched),
        (false, 0, 3)
    );

    let previous = tree(w0, 3);
    let current = TreeRecord {
        constants: w1,
        ..previous
    };
    let tree = |history: &mut History, visual, sample: &TreeRecord| {
        rsf_anomaly_foliage_tree(Some(history), 0, visual, sample)
    };
    history.begin(0, frame(30, 0, 640, 360));
    assert!(tree(&mut history, 0xabc0, &previous).is_null());
    assert!(history.begin(0, frame(31, 0, 640, 360)));
    let before = tree(&mut history, 0xabc0, &current);
    assert!(
        unsafe { before.as_ref() }.is_some_and(|found| *found == previous && found.constants == w0)
    );
    assert_eq!(tree(&mut history, 0xabc0, &current), before);
    let conflict = TreeRecord { lod: 4, ..current };
    let count = rsf_anomaly_foliage_conflicts(Some(&history));
    assert!(tree(&mut history, 0xabc0, &conflict).is_null());
    assert_eq!(rsf_anomaly_foliage_conflicts(Some(&history)), count + 1);
    assert!(tree(&mut history, 0xdef0, &current).is_null());
    assert!(history.begin(0, frame(32, 0, 640, 360)));
    assert!(tree(&mut history, 0xabc0, &current).is_null());
}

pub(super) fn policy() {
    use HistoryReason::*;
    let camera = CameraSample {
        view: sample(0.1),
        projection: sample(0.2),
    };
    let previous = tree(wind(1., 0.5), 2);
    let tree = || TreeDrawInput {
        family: FOLIAGE_TREE,
        in_visual: true,
        kind: TREE,
        verdict: IDENTIFIED_FOLIAGE,
        context: true,
        camera_conflict: false,
        previous_camera: &camera,
        constants_ok: true,
        current: TreeRecord {
            constants: wind(1.1, 0.6),
            ..previous
        },
        previous: &previous,
        previous_jitter_u: 0.,
        previous_jitter_v: 0.,
    };
    let vp = camera.projection.compose(&camera.view);
    assert!(unsafe { decide_tree(&tree()) }.ok() == Some(vp));
    let cases: [(HistoryReason, fn(&mut TreeDrawInput)); 10] = [
        (KindMismatch, |t| t.family = CORE_STATIC),
        (NoVisual, |t| t.in_visual = false),
        (Unidentified, |t| {
            t.verdict = RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED as u8
        }),
        (NoContext, |t| t.context = false),
        (FoliageNoConstants, |t| t.constants_ok = false),
        (CameraConflict, |t| t.camera_conflict = true),
        (NoPreviousCamera, |t| t.previous_camera = ptr::null()),
        (FoliageNoPrevious, |t| t.previous = ptr::null()),
        (FoliageLodChanged, |t| t.current.lod = 3),
        (FoliageXformChanged, |t| t.current.xform[3] = 9.),
    ];
    for (reason, edit) in cases {
        let mut input = tree();
        edit(&mut input);
        assert!(unsafe { decide_tree(&input) }.err() == Some(reason));
    }
    let jittered = CameraSample {
        projection: add_jitter(&camera.projection, 0.004, -0.001),
        ..camera
    };
    let input = TreeDrawInput {
        previous_camera: &jittered,
        previous_jitter_u: 0.004,
        previous_jitter_v: -0.001,
        ..tree()
    };
    let expected = jittered
        .projection
        .remove_jitter(0.004, -0.001)
        .compose(&camera.view);
    assert!(unsafe { decide_tree(&input) }.ok() == Some(expected));

    let detail = || DetailDrawInput {
        family: FOLIAGE_DETAIL,
        detail_window: true,
        context: true,
        camera_conflict: false,
        previous_camera: &camera,
        constants_ok: true,
        world_readable: true,
        world_identity: true,
        route_found: true,
        live_groups: 12,
        previous_jitter_u: 0.,
        previous_jitter_v: 0.,
    };
    assert!(unsafe { decide_detail(&detail()) }.ok() == Some(vp));
    let cases: [(HistoryReason, fn(&mut DetailDrawInput)); 9] = [
        (KindMismatch, |d| d.family = FOLIAGE_TREE),
        (NoVisual, |d| d.detail_window = false),
        (NoContext, |d| d.context = false),
        (FoliageNoConstants, |d| d.live_groups = 0),
        (FoliageWorld, |d| d.world_identity = false),
        (FoliageWorld, |d| d.world_readable = false),
        (CameraConflict, |d| d.camera_conflict = true),
        (NoPreviousCamera, |d| d.previous_camera = ptr::null()),
        (FoliageNoPrevious, |d| d.route_found = false),
    ];
    for (reason, edit) in cases {
        let mut input = detail();
        edit(&mut input);
        assert!(unsafe { decide_detail(&input) }.err() == Some(reason));
    }
    assert_eq!(
        unsafe {
            std::ffi::CStr::from_ptr(rsf_anomaly_motion_history_reason_name(
                FoliageXformChanged as u8,
            ))
        },
        c"foliage_xform_changed",
    );
    assert_eq!(InvalidMatrix as u8 + 1, 17);
}
