// SPDX-License-Identifier: MIT
use super::*;
use crate::sr_stage_abi::RSF_ANOMALY_SR_MOTION_TO_UV;
use rsf_game_sys::sr_abi::rsf_motion_decode_params;

unsafe extern "C" {
    // The same private decoder parameter provider used by the SR pass (sr_pass.rs).
    fn rsf_anomaly_sr_motion_decode_params(out: &mut rsf_motion_decode_params);
}

const EXTENT: [f64; 2] = [960., 540.];

fn shift(x: f64, y: f64, z: f64) -> Mat4 {
    let mut result = Mat4::IDENTITY;
    [result.0[3], result.0[7], result.0[11]] = [x, y, z];
    result
}

fn yaw(angle: f64) -> Mat4 {
    let mut result = Mat4::IDENTITY;
    let (s, c) = angle.sin_cos();
    [result.0[0], result.0[2], result.0[8], result.0[10]] = [c, s, -s, c];
    result
}

fn projection() -> Mat4 {
    let h = 1. / 0.6f64.tan();
    let [w, height] = EXTENT;
    let q = 900. / (900. - 0.2);
    let mut result = Mat4::default();
    [
        result.0[0],
        result.0[5],
        result.0[10],
        result.0[11],
        result.0[14],
    ] = [h / (w / height), h, q, -q * 0.2, 1.];
    result
}

fn dot(a: &[f64], b: [f64; 4]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn transform(matrix: &Mat4, point: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|r| dot(&matrix.0[r * 4..r * 4 + 4], point))
}

fn uv(clip: [f64; 4]) -> [f64; 2] {
    [clip[0] / clip[3] * 0.5 + 0.5, 0.5 - clip[1] / clip[3] * 0.5]
}

fn pixels(clip: [f64; 4]) -> [f64; 2] {
    let uv = uv(clip);
    std::array::from_fn(|i| uv[i] * EXTENT[i])
}

struct Draw {
    view: Mat4,
    world: Mat4,
    bones: [Mat4; 2],
    jitter: [f64; 2],
}

impl Draw {
    fn camera(&self, jitter: bool) -> CameraSample {
        let mut projection = projection();
        if jitter {
            let [jx, jy] = self.jitter;
            for c in 0..4 {
                projection.0[c] += 2. * jx / EXTENT[0] * projection.0[12 + c];
                projection.0[4 + c] -= 2. * jy / EXTENT[1] * projection.0[12 + c];
            }
        }
        CameraSample {
            view: self.view,
            projection,
        }
    }
}

fn run(skin: i32) {
    let a = Draw {
        view: yaw(0.).compose(&shift(-0.1, -1.7, 0.)),
        world: shift(1., 0., 8.).compose(&yaw(0.10)),
        bones: [shift(0., 0.02, 0.), yaw(0.20)],
        jitter: [0.25, -0.375],
    };
    let b = Draw {
        view: yaw(0.02).compose(&shift(-0.15, -1.7, 0.)),
        world: shift(1.4, 0., 8.).compose(&yaw(0.18)),
        bones: [shift(0., 0.05, 0.), yaw(0.35)],
        jitter: [-0.4375, 0.125],
    };
    let vertex = [0.3, 1.2, 0.1, 1.];
    let weight = 0.35;
    let world = |draw: &Draw| {
        if skin == 0 {
            draw.world.compose(&draw.bones[1])
        } else {
            draw.world
        }
    };
    let object = |draw: &Draw| {
        if skin == 0 {
            vertex
        } else {
            let [a, b] = draw.bones.map(|bone| transform(&bone, vertex));
            let mut point = std::array::from_fn(|i| (1. - weight) * a[i] + weight * b[i]);
            point[3] = 1.;
            point
        }
    };
    let clip = |draw: &Draw, jitter| {
        let camera = draw.camera(jitter);
        let wvp = camera
            .projection
            .compose(&camera.view)
            .compose(&world(draw));
        transform(&wvp, object(draw))
    };
    let key = InstanceKey {
        owner: 0x10,
        visual: 0x20,
        generation: 1,
    };
    let mut history = MotionHistoryState::default();
    let record = |history: &mut MotionHistoryState, source, draw: &Draw| {
        let continuous = history.begin(0, frame(source, 0, EXTENT[0] as u32, EXTENT[1] as u32));
        history.observe(0, 0, &draw.camera(true));
        let bones = if skin == 0 {
            &[][..]
        } else {
            draw.bones.as_slice()
        };
        let previous = history.record(0, key, 0, snapshot(world(draw), bones));
        (continuous, previous.map(Snapshot::draw_view))
    };
    record(&mut history, 100, &a);
    let (continuous, previous) = record(&mut history, 101, &b);
    assert!(continuous);
    let input = CoreDrawInput {
        family: CORE_MODEL,
        in_visual: true,
        context: true,
        skin,
        verdict: IDENTIFIED,
        camera_conflict: false,
        kind: if skin == 0 {
            SINGLE_BONE
        } else {
            HARDWARE_SKINNED
        },
        previous_camera: history.previous_camera(0, 0).unwrap(),
        previous: previous.expect("mover previous snapshot"),
    };
    let d = decision(&input);
    assert!(d.reason == HistoryReason::Valid);
    let mut vs = MaybeUninit::uninit();
    assert!(
        unsafe {
            pack_core(
                &d.previous_wvp,
                d.bones.data,
                d.bones.count,
                &d.flags,
                &mut vs,
            )
        }
        .is_ok()
    );
    let vs = unsafe { vs.assume_init_ref() };
    assert_eq!(
        [
            word(vs, CORE_HISTORY),
            word(vs, CORE_HISTORY + 4),
            word(vs, CORE_HISTORY + 12)
        ],
        [1., if skin == 0 { 0. } else { 1. }, 1.]
    );
    let vs_row = |base, row| -> [f64; 4] {
        std::array::from_fn(|c| f64::from(word(vs, base + (row * 4 + c) * 4)))
    };
    let mut previous_object = vertex;
    if skin != 0 {
        for (r, value) in previous_object[..3].iter_mut().enumerate() {
            *value = (1. - weight) * dot(&vs_row(CORE_BONES, r), vertex)
                + weight * dot(&vs_row(CORE_BONES, 3 + r), vertex);
        }
    }
    let previous_clip = std::array::from_fn(|r| dot(&vs_row(0, r), previous_object));
    let previous_uv = uv(previous_clip);
    let raster = pixels(clip(&b, true));
    let jitter = [
        (b.jitter[0] / EXTENT[0]) as f32,
        (b.jitter[1] / EXTENT[1]) as f32,
        (a.jitter[0] / EXTENT[0]) as f32,
        (a.jitter[1] / EXTENT[1]) as f32,
    ];
    let mut ps = MaybeUninit::uninit();
    assert!(rsf_anomaly_motion_pack_pixel(
        &PixelInputs {
            viewport: [0., 0., EXTENT[0] as f32, EXTENT[1] as f32],
            jitter
        },
        &mut ps
    ));
    let ps = unsafe { ps.assume_init_ref() };
    let components = std::array::from_fn::<_, 2, _>(|i| {
        let current = raster[i] * f64::from(word(ps, 8 + i * 4)) - f64::from(word(ps, 16 + i * 4));
        let previous = previous_uv[i] - f64::from(word(ps, 24 + i * 4));
        (current - previous) as f32
    });
    let stored = [components[0], components[1], 1., 0.];
    let mut params = rsf_motion_decode_params::default();
    unsafe { rsf_anomaly_sr_motion_decode_params(&mut params) };
    assert!(stored[params.validity_channel as usize] > params.validity_threshold);
    let [dx, dy] = [
        (stored[0] - params.bias_x) * params.scale_x * params.output_scale_x,
        (stored[1] - params.bias_y) * params.scale_y * params.output_scale_y,
    ];
    let written = [
        -f64::from(dx) * EXTENT[0] * RSF_ANOMALY_SR_MOTION_TO_UV,
        -f64::from(dy) * EXTENT[1] * RSF_ANOMALY_SR_MOTION_TO_UV,
    ];
    let old = pixels(clip(&a, false));
    let current = pixels(clip(&b, false));
    let truth = [old[0] - current[0], old[1] - current[1]];
    let camera = a.camera(false);
    let camera_previous = pixels(transform(
        &camera.projection.compose(&camera.view),
        transform(&world(&b), object(&b)),
    ));
    let camera_only = [
        camera_previous[0] - current[0],
        camera_previous[1] - current[1],
    ];
    let error = (written[0] - truth[0]).hypot(written[1] - truth[1]);
    let ghost = (camera_only[0] - truth[0]).hypot(camera_only[1] - truth[1]);
    println!(
        "mover SKIN_{skin}: written {written:.4?} px, truth {truth:.4?} px, error {error:.5} px; camera-only misses by {ghost:.2} px"
    );
    assert!(error < 2e-3 && truth[0].hypot(truth[1]) > 5. && ghost > 5.);
}

pub(super) fn exercise() {
    for skin in [2, 0] {
        run(skin);
    }
}
