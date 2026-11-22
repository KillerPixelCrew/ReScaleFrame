// SPDX-License-Identifier: MIT
use super::*;

pub(super) fn view_timelines(api: &Oracle) {
    let h = History::new(api);
    let a = h.start(&frame(MAIN, 100));
    assert!(!h.cameras(a, WORLD).unwrap().has_previous);
    let scope = h.start(&frame(SCOPE, 101));
    let mut next = frame(MAIN, 102);
    next.world.view.values[3] = 2.;
    let main = h.start(&next);
    let pair = h.cameras(main, WORLD).unwrap();
    assert!(pair.has_previous && pair.previous_frame == 100);
    assert_eq!(pair.previous.view.values[3], 0.);
    let (status, repeated) = h.begin(&next);
    assert_eq!((status, token(repeated)), (REPEATED, token(main)));
    assert!(h.cameras(scope, WORLD).is_some());
    let scope = h.start(&frame(SCOPE, 103));
    assert_eq!(h.cameras(scope, WORLD).unwrap().previous_frame, 101);
    assert!(h.cameras(a, WORLD).is_none());
}

pub(super) fn reject_frame_mutation(api: &Oracle) {
    let h = History::new(api);
    let input = frame(MAIN, 20);
    let t = h.start(&input);
    let mut changed = input;
    changed.world.view.values[3] = 1.;
    assert_eq!(h.begin(&changed).0, FRAME_CONFLICT);
    assert_eq!(h.begin(&frame(MAIN, 19)).0, OUT_OF_ORDER);
    let mut invalid = frame(MAIN, 21);
    invalid.world.projection.values[0] = f64::NAN;
    assert_eq!(h.begin(&invalid).0, INVALID);
    invalid = frame(MAIN, 21);
    invalid.sizes.render.width = 0;
    assert_eq!(h.begin(&invalid).0, INVALID);
    let camera = h.cameras(t, WORLD).unwrap().current;
    assert_eq!(
        (camera.view.values, camera.projection.values),
        (input.world.view.values, input.world.projection.values)
    );
    assert_eq!((camera.jitter.x, camera.jitter.y), (0., 0.));
    assert_eq!(h.begin(&frame(99, 21)).0, INVALID);
    assert!(h.cameras(Token { view: 99, ..t }, WORLD).is_none());
    assert!(h.cameras(t, 99).is_none());
}

pub(super) fn camera_resets(api: &Oracle) {
    let h = History::new(api);
    let mut input = frame(MAIN, 1);
    input.has_first_person = true;
    input.first_person = camera();
    input.first_person.projection.values[0] = 3.;
    h.start(&input);
    input.source_frame = 3;
    input.first_person.projection.values[0] = 4.;
    let pair = h.cameras(h.start(&input), FIRST_PERSON).unwrap();
    assert!(pair.has_previous);
    assert_eq!(
        (
            pair.previous.projection.values[0],
            pair.current.projection.values[0]
        ),
        (3., 4.)
    );
    input.source_frame = 5;
    input.first_person_cut = true;
    let t = h.start(&input);
    assert!(!h.cameras(t, FIRST_PERSON).unwrap().has_previous);
    assert!(h.cameras(t, WORLD).unwrap().has_previous);
    input.source_frame = 7;
    input.has_first_person = false;
    input.first_person_cut = false;
    assert!(h.cameras(h.start(&input), FIRST_PERSON).is_none());
    input.source_frame = 9;
    input.camera_cut = true;
    assert!(!h.cameras(h.start(&input), WORLD).unwrap().has_previous);
    input.source_frame = 11;
    input.camera_cut = false;
    input.sizes.render = extent(960, 540);
    assert!(!h.cameras(h.start(&input), WORLD).unwrap().has_previous);
    input.source_frame = 13;
    assert!(h.cameras(h.start(&input), WORLD).unwrap().has_previous);
    input.source_frame = 15;
    input.sizes.output = extent(2560, 1440);
    assert!(!h.cameras(h.start(&input), WORLD).unwrap().has_previous);
}

pub(super) fn rigid_instances(api: &Oracle) {
    let h = History::new(api);
    let (a, b) = (key(10, 100, 1), key(11, 100, 1));
    let mut data = pose();
    let first_token = h.start(&frame(MAIN, 10));
    let first = h.capture(first_token, a, &data);
    assert_eq!(first.status(), CAPTURED);
    assert!(first.current().is_some() && first.previous().is_none());
    data.world.values[3] = 7.;
    assert_eq!(first.current().unwrap().world.values[3], 0.);
    let next = h.start(&frame(MAIN, 12));
    let current = h.capture(next, a, &data);
    assert_eq!(
        (
            current.previous().unwrap().world.values[3],
            current.current().unwrap().world.values[3]
        ),
        (0., 7.)
    );
    let again = h.capture(next, a, &data);
    assert_eq!(
        (again.status(), again.handles()),
        (CAPTURE_REPEATED, current.handles())
    );
    data.world.values[3] = 8.;
    assert_eq!(h.capture(next, a, &data).status(), CAPTURE_CONFLICT);
    assert_eq!(current.current().unwrap().world.values[3], 7.);
    assert!(h.capture(next, b, &data).previous().is_none());
    assert_eq!(h.capture(first_token, a, &data).status(), STALE);
}

pub(super) fn retirement(api: &Oracle) {
    let h = History::new(api);
    let k = key(42, 500, 3);
    let data = pose();
    let t = h.start(&frame(MAIN, 1));
    h.capture(t, k, &data);
    let scope = h.start(&frame(SCOPE, 2));
    assert!(h.capture(scope, k, &data).previous().is_none());
    let t = h.start(&frame(MAIN, 3));
    assert!(h.capture(t, k, &data).previous().is_some());
    for different in [key(42, 501, 3), key(42, 500, 4)] {
        assert!(h.capture(t, different, &data).previous().is_none());
    }
    h.start(&frame(MAIN, 5)); // Omitted from one visible source frame.
    let t = h.start(&frame(MAIN, 7));
    assert!(h.capture(t, k, &data).previous().is_none());
    assert!(h.retire(k));
    assert_eq!(h.capture(t, k, &data).status(), RETIRED);
    assert_eq!(h.capture(scope, k, &data).status(), RETIRED);
    assert_eq!(h.capture(t, key(42, 500, 2), &data).status(), RETIRED);
    let later = h.capture(t, key(42, 500, 4), &data);
    assert_eq!(later.status(), CAPTURED);
    assert!(later.previous().is_none());
}

pub(super) fn bones(api: &Oracle) {
    let h = History::new(api);
    let k = key(5, 6, 1);
    let mut bones = [identity(), identity()];
    bones[1].values[7] = 4.;
    let mut data = Pose {
        kind: SKINNED,
        palette_layout: 77,
        bone_count: bones.len(),
        ..pose()
    };
    let t = h.start(&frame(MAIN, 1));
    data.bones = bones.as_ptr();
    let initial = h.capture(t, k, &data);
    bones[1].values[7] = 9.;
    assert_eq!(initial.current().unwrap().bone1.values[7], 4.);
    let t = h.start(&frame(MAIN, 3));
    data.bones = bones.as_ptr();
    let next = h.capture(t, k, &data);
    assert_eq!(next.previous().unwrap().bone1.values[7], 4.);
    assert_eq!(h.capture(t, k, &data).handles(), next.handles());
    bones[0].values[3] = 12.;
    data.bones = bones.as_ptr();
    assert_eq!(h.capture(t, k, &data).status(), CAPTURE_CONFLICT);
    let t = h.start(&frame(MAIN, 5));
    data.palette_layout = 78;
    assert!(h.capture(t, k, &data).previous().is_none());
    let t = h.start(&frame(MAIN, 7));
    data.bone_count = 1;
    assert!(h.capture(t, k, &data).previous().is_none());
}

pub(super) fn details(api: &Oracle) {
    let h = History::new(api);
    let (ka, kb) = (key(1001, 90, 1), key(1002, 90, 1));
    let mut constants = [
        Vec4 {
            x: 1.,
            y: 2.,
            z: 3.,
            w: 4.,
        },
        Vec4 {
            x: 5.,
            y: 6.,
            z: 7.,
            w: 8.,
        },
    ];
    let mut a = Pose {
        kind: DETAIL,
        has_deformation: true,
        deformation_layout: 99,
        constants: constants.as_ptr(),
        constant_count: 2,
        has_detail: true,
        fade: 0.25,
        ..pose()
    };
    let mut b = a;
    b.world.values[3] = 100.;
    b.fade = 0.75;
    let t = h.start(&frame(MAIN, 1));
    h.capture(t, ka, &a);
    h.capture(t, kb, &b);
    let t = h.start(&frame(MAIN, 3));
    // Only the first item's shader constants change. The second keeps its frozen schema/values.
    let unchanged = constants;
    constants[0].w = 20.;
    a.constants = constants.as_ptr();
    b.constants = unchanged.as_ptr();
    a.fade = 0.5;
    b.fade = 0.9;
    let current_b = h.capture(t, kb, &b);
    let current_a = h.capture(t, ka, &a);
    let earlier_a = current_a.previous().unwrap();
    let earlier_b = current_b.previous().unwrap();
    assert_eq!(earlier_a.fade, 0.25);
    assert_eq!((earlier_b.fade, earlier_b.world.values[3]), (0.75, 100.));
    assert_eq!(
        (
            earlier_a.deformation_w,
            current_a.current().unwrap().deformation_w
        ),
        (4., 20.)
    );
    assert_eq!(h.capture(t, ka, &a).handles().1, current_a.handles().1);
    assert!(h.retire(ka));
    assert!(h.capture(t, key(1001, 90, 2), &a).previous().is_none());
    let t = h.start(&frame(MAIN, 5));
    b.deformation_layout = 100;
    assert!(h.capture(t, kb, &b).previous().is_none());
}

pub(super) fn reject_producer(api: &Oracle) {
    let h = History::new(api);
    let t = h.start(&frame(MAIN, 1));
    for key in [key(0, 10, 1), key(1, 0, 1), key(1, 10, 0)] {
        assert_eq!(h.capture(t, key, &pose()).status(), CAPTURE_INVALID);
    }
    assert!(!h.retire(key(0, 10, 1)));
    let mut owner = 100;
    let mut capture = |data: &Pose| {
        owner += 1;
        h.capture(t, key(owner, 10, 1), data)
    };
    let invalid = |result: OwnedCapture<'_>| {
        assert_eq!(result.status(), CAPTURE_INVALID);
        assert!(result.current().is_none() && result.previous().is_none());
    };
    let mut bad = pose();
    bad.world.values[0] = f64::INFINITY;
    invalid(capture(&bad));
    bad = pose();
    bad.world.values[12] = 1.;
    invalid(capture(&bad));
    for data in [
        Pose {
            domain: 99,
            ..pose()
        },
        Pose { kind: 99, ..pose() },
        Pose {
            kind: SKINNED,
            ..pose()
        },
    ] {
        invalid(capture(&data));
    }
    let mut bones = vec![identity(); 1025];
    let mut data = Pose {
        kind: SKINNED,
        bones: bones.as_ptr(),
        bone_count: 1,
        ..pose()
    };
    invalid(capture(&data)); // No palette layout.
    data.palette_layout = 1;
    bones[0].values[11] = f64::NAN;
    data.bones = bones.as_ptr();
    invalid(capture(&data));
    bones[0] = identity();
    data.bones = bones.as_ptr();
    data.bone_count = bones.len();
    invalid(capture(&data));
    invalid(capture(&Pose {
        bones: bones.as_ptr(),
        bone_count: 1,
        ..pose()
    }));
    let mut data = Pose {
        kind: DEFORMED,
        ..pose()
    };
    invalid(capture(&data)); // No deformation.
    let mut constants = [Vec4 {
        w: 1.,
        ..Default::default()
    }];
    data.has_deformation = true;
    data.deformation_layout = 1;
    data.constants = constants.as_ptr();
    data.constant_count = 1;
    assert_eq!(capture(&data).status(), CAPTURED);
    constants[0].x = f64::INFINITY;
    data.constants = constants.as_ptr();
    invalid(capture(&data));
    constants[0].x = 0.;
    data.constants = constants.as_ptr();
    data.kind = DETAIL;
    invalid(capture(&data)); // Missing per-item fade.
    data.has_detail = true;
    for fade in [-0.01, f64::NAN] {
        data.fade = fade;
        invalid(capture(&data));
    }
    data.fade = 0.;
    assert_eq!(capture(&data).status(), CAPTURED);
    let missing = capture(&Pose {
        domain: FIRST_PERSON,
        ..pose()
    });
    assert_eq!(missing.status(), MISSING_PROJECTION);
    assert!(missing.current().is_none() && missing.previous().is_none());
}

pub(super) fn first_person(api: &Oracle) {
    let h = History::new(api);
    let k = key(5, 6, 1);
    let mut input = frame(MAIN, 1);
    input.has_first_person = true;
    input.first_person = camera();
    input.first_person.projection.values[0] = 3.;
    let mut world = pose();
    let mut hand = Pose {
        domain: FIRST_PERSON,
        ..pose()
    };
    hand.world.values[3] = 2.;
    let t = h.start(&input);
    h.capture(t, k, &world);
    assert_eq!(h.capture(t, k, &hand).status(), CAPTURED);
    input.source_frame = 3;
    let t = h.start(&input);
    world.world.values[3] = 9.;
    hand.world.values[3] = 4.;
    assert_eq!(
        h.capture(t, k, &world).previous().unwrap().world.values[3],
        0.
    );
    assert_eq!(
        h.capture(t, k, &hand).previous().unwrap().world.values[3],
        2.
    );
    input.source_frame = 5;
    input.first_person_cut = true;
    let t = h.start(&input);
    assert!(h.capture(t, k, &hand).previous().is_none());
    assert!(h.capture(t, k, &world).previous().is_some());
    input.source_frame = 7;
    input.first_person_cut = false;
    input.camera_cut = true;
    let t = h.start(&input);
    assert!(
        h.capture(t, k, &hand).previous().is_none() && h.capture(t, k, &world).previous().is_none()
    );
}

pub(super) fn explicit_resets(api: &Oracle) {
    let h = History::new(api);
    let other = History::new(api);
    let k = key(4, 5, 1);
    let t = h.start(&frame(MAIN, 10));
    let wrong = other.start(&frame(MAIN, 10));
    assert!(h.cameras(wrong, WORLD).is_none());
    let scope = h.start(&frame(SCOPE, 11));
    h.capture(t, k, &pose());
    h.retire(k);
    assert!(h.reset_view(MAIN));
    assert!(h.cameras(t, WORLD).is_none());
    assert!(h.cameras(scope, WORLD).is_some());
    let restarted = h.start(&frame(MAIN, 0));
    assert!(!h.cameras(restarted, WORLD).unwrap().has_previous);
    assert_eq!(h.capture(restarted, k, &pose()).status(), RETIRED);
    assert!(!h.reset_view(99));
    h.reset_all();
    assert!(h.cameras(scope, WORLD).is_none() && h.cameras(restarted, WORLD).is_none());
}

pub(super) fn degenerate_camera(api: &Oracle) {
    let h = History::new(api);
    let mut input = frame(MAIN, 1);
    input.world.projection = Matrix::default();
    assert_eq!(h.begin(&input).0, INVALID);
    input = frame(MAIN, 1);
    input.world.view.values[10] = 0.;
    assert_eq!(h.begin(&input).0, INVALID);
    input = frame(MAIN, 1);
    input.world.view.values[12] = 1.;
    assert_eq!(h.begin(&input).0, INVALID);
    assert_eq!(
        h.begin(&Frame {
            sizes: sizes(),
            ..Default::default()
        })
        .0,
        INVALID
    );
    input = frame(MAIN, 1);
    input.has_first_person = true;
    assert_eq!(h.begin(&input).0, INVALID);
    input = frame(MAIN, 1);
    input.world.jitter.x = f64::MAX;
    assert_eq!(h.begin(&input).0, INVALID);
    let t = h.start(&frame(MAIN, 1));
    assert_eq!(
        h.capture(t, key(1, 2, 1), &Pose::default()).status(),
        CAPTURE_INVALID
    );
}

pub(super) fn frame_plans(api: &Oracle) {
    let h = History::new(api);
    let mut input = frame(MAIN, 80);
    input.world.jitter = Vec2 { x: 0.25, y: 0.5 };
    input.has_first_person = true;
    input.first_person = camera();
    input.first_person.projection.values[0] = 3.;
    input.first_person.jitter = Vec2 { x: -0.25, y: -0.5 };
    let t = h.start(&input);
    let plan = h.plan(t).unwrap();
    assert_eq!(token(plan.token), token(t));
    assert_eq!(
        [
            plan.sizes.render,
            plan.sizes.output,
            plan.screen_ui,
            plan.sizes.pda
        ]
        .map(dimension),
        [(1280, 720), (1920, 1080), (1920, 1080), (1024, 512)]
    );
    let render = plan.sizes.render;
    assert_eq!(
        plan.world.values,
        math::jitter(api, &input.world.projection, 0.25, 0.5, render)
            .unwrap()
            .values
    );
    assert!(plan.has_first_person);
    assert_eq!(
        plan.first_person.values,
        math::jitter(api, &input.first_person.projection, -0.25, -0.5, render)
            .unwrap()
            .values
    );
    assert_eq!(plan.presentation, 0);
    let scope = h.start(&frame(SCOPE, 81));
    let scope_plan = h.plan(scope).unwrap();
    assert!(scope_plan.presentation == 1 && !scope_plan.has_first_person);
    h.start(&frame(MAIN, 82));
    assert!(h.plan(plan.token).is_none());
}
