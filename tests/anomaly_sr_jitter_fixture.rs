// SPDX-License-Identifier: MIT
use crate::sr_stage::math::{
    clamp_mip_offset, jitter_sample, jitter_to_uv, mip_delta, next_jitter, phase_count,
};
use crate::sr_swap::{engine_mul, jitter_projection};
use crate::sys::*;
use rsf_game_sys::sampling::halton_f64 as halton;
use std::ffi::{CStr, c_char, c_int};
use std::panic::catch_unwind;

type Sample = rsf_anomaly_sr_jitter_sample;
type Sequence = rsf_anomaly_sr_jitter_sequence;
type State = rsf_anomaly_sr_jitter_state;
type Gate = rsf_anomaly_sr_burst_gate;

fn sample(index: u32, phases: u32) -> Sample {
    jitter_sample(index, phases)
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected}"
    );
}

fn halton_sequence() {
    assert_eq!([1, 2, 3, 4].map(|i| halton(i, 2)), [0.5, 0.25, 0.75, 0.125]);
    for (i, expected) in [1.0 / 3.0, 2.0 / 3.0, 1.0 / 9.0].into_iter().enumerate() {
        near(halton(i as u32 + 1, 3), expected, 1e-15);
    }
    let mut x_strata = [false; 8];
    let mut y_strata = [false; 9];
    for i in 0..9 {
        let s = sample(i, 64);
        assert!((-0.5..0.5).contains(&s.x) && (-0.5..0.5).contains(&s.y));
        if i < 8 {
            x_strata[((f64::from(s.x) + 0.5) * 8.0) as usize] = true;
        }
        y_strata[((halton(i + 1, 3) * 9.0 + 1e-9).floor()) as usize] = true;
        near(s.x.into(), halton(i + 1, 2) - 0.5, 1e-7);
        near(s.y.into(), halton(i + 1, 3) - 0.5, 1e-7);
    }
    assert!(x_strata.into_iter().all(|hit| hit) && y_strata.into_iter().all(|hit| hit));
    let first = sample(0, 8);
    let wrapped = sample(18, 18);
    assert_eq!(
        (first.x, first.y, first.index, first.phase_count),
        (0.0, -1.0 / 6.0, 0, 8)
    );
    assert_eq!((wrapped.x, wrapped.y, wrapped.index), (first.x, first.y, 0));
    assert_eq!(sample(19, 18).index, 1);
    assert_eq!(sample(5, 0).phase_count, 8);
    for (render, output, expected) in [
        (1286, 1920, 18),
        (858, 1280, 18),
        (960, 1920, 32),
        (1920, 1920, 8),
        (1500, 1920, 14),
        (0, 1920, 8),
        (2000, 1920, 8),
        (100, 1920, 64),
        (1, u32::MAX, 64),
    ] {
        assert_eq!(phase_count(render, output), expected);
    }
}

fn sequence_resets() {
    let mut seq = Sequence::default();
    let world = [1286, 724, 1920, 1080];
    let next =
        |s: &mut Sequence, dims, generation, continuous| s.next(dims, generation, continuous);
    let first = next(&mut seq, world, 0, true);
    assert_eq!(
        (first.index, first.phase_count, seq.reset, seq.resets),
        (0, 18, true, 0)
    );
    for i in 1..18 {
        assert_eq!(next(&mut seq, world, 0, true).index, i);
        assert!(!seq.reset);
    }
    assert_eq!(next(&mut seq, world, 0, true).index, 0);
    assert!(!seq.reset);
    assert_eq!(next(&mut seq, world, 0, true).index, 1);
    assert_eq!(next(&mut seq, world, 0, false).index, 0);
    assert!(seq.reset && seq.resets == 1);
    assert_eq!(next(&mut seq, world, 0, true).index, 1);
    let scaled = next(&mut seq, [960, 540, 1920, 1080], 0, true);
    assert_eq!((scaled.index, scaled.phase_count, seq.resets), (0, 32, 2));
    let native = [1280, 720, 1280, 720];
    assert_eq!(next(&mut seq, native, 0, true).phase_count, 8);
    assert_eq!(seq.resets, 3);
    assert_eq!(next(&mut seq, native, 1, true).index, 0);
    assert_eq!(next(&mut seq, native, 1, true).index, 1);
    assert_eq!((seq.index, seq.resets, seq.frames), (2, 4, 26));
    seq = Sequence::default();
    assert_eq!(next(&mut seq, world, 0, true).index, 0);
    assert_eq!(seq.resets, 0);
}

const VIEW: [f32; 16] = [
    0.8, 0.0, -0.6, 0.0, 0.0, 1.0, 0.0, 0.0, 0.6, 0.0, 0.8, 0.0, -3.0, -1.5, 7.25, 1.0,
];

fn perspective(aspect: f32) -> [f32; 16] {
    let h = 1.0 / 0.6_f32.tan();
    let q = 1000.0 / (1000.0 - 0.2);
    [
        h / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        h,
        0.0,
        0.0,
        0.0,
        0.0,
        q,
        1.0,
        0.0,
        0.0,
        -q * 0.2,
        0.0,
    ]
}

fn clip(point: [f64; 4], matrix: [f32; 16]) -> [f64; 4] {
    std::array::from_fn(|col| {
        (0..4)
            .map(|row| point[row] * f64::from(matrix[row * 4 + col]))
            .sum()
    })
}

fn uv(clip: [f64; 4]) -> [f64; 2] {
    [clip[0] / clip[3] * 0.5 + 0.5, 0.5 - clip[1] / clip[3] * 0.5]
}

fn ndc_offset() {
    let (w, h) = (1286, 724);
    let p = perspective(1920.0 / 1080.0);
    for (x, y) in [0, 1, 7]
        .map(|i| {
            let s = sample(i, 18);
            (s.x, s.y)
        })
        .into_iter()
        .chain([(-0.5, 0.4999)])
    {
        let shifted = jitter_projection(&p, x, y, w, h).unwrap();
        for i in (0..16).filter(|&i| i != 8 && i != 9) {
            assert_eq!(shifted[i].to_bits(), p[i].to_bits());
        }
        near(shifted[8].into(), 2.0 * f64::from(x) / f64::from(w), 1e-8);
        near(shifted[9].into(), -2.0 * f64::from(y) / f64::from(h), 1e-8);
        for point in [
            [0.0, 0.0, 5.0, 1.0],
            [-3.5, 2.25, 40.0, 1.0],
            [12.0, -7.0, 900.0, 1.0],
            [0.1, 0.05, 0.3, 1.0],
        ] {
            let (a, b) = (clip(point, p), clip(point, shifted));
            assert_eq!(a[2..], b[2..]);
            near((uv(b)[0] - uv(a)[0]) * f64::from(w), x.into(), 2e-4);
            near((uv(b)[1] - uv(a)[1]) * f64::from(h), y.into(), 2e-4);
        }
    }
    let ortho = [
        0.01, 0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0, 0.1, -0.2, 0.5, 1.0,
    ];
    let shifted = jitter_projection(&ortho, 0.25, -0.125, 200, 100).unwrap();
    assert_eq!(shifted[..12], ortho[..12]);
    near(shifted[12].into(), 0.1025, 1e-7);
    near(shifted[13].into(), -0.1975, 1e-7);
    let point = [3.0, 4.0, 10.0, 1.0];
    let (a, b) = (uv(clip(point, ortho)), uv(clip(point, shifted)));
    near((b[0] - a[0]) * 200.0, 0.25, 1e-4);
    near((b[1] - a[1]) * 100.0, -0.125, 1e-4);
    let mut invalid = p;
    invalid[5] = f32::INFINITY;
    assert!(jitter_projection(&invalid, 0.1, 0.1, w, h).is_none());
    assert!(jitter_projection(&p, f32::NAN, 0.1, w, h).is_none());
    for (w, h) in [(1, h), (w, 0), (16385, h)] {
        assert!(jitter_projection(&p, 0.1, 0.1, w, h).is_none());
    }
    let full = engine_mul(&p, &VIEW);
    for point in [[1.0, 2.0, 3.0, 1.0], [-5.0, 0.5, 20.0, 1.0]] {
        for (direct, chained) in clip(point, full)
            .into_iter()
            .zip(clip(clip(point, VIEW), p))
        {
            near(direct, chained, 1e-4 * (1.0 + chained.abs()));
        }
    }
}

fn mip_bias() {
    let ratio = (1286.0_f64 / 1920.0).log2();
    for (rw, ow, offset, expected) in [
        (1286, 1920, -0.3, ratio - 0.3),
        (960, 1920, 0.0, -1.0),
        (1920, 1920, 0.0, 0.0),
        (1920, 1920, -0.3, -0.3),
        (960, 1920, -5.0, -3.0),
        (1920, 1920, 5.0, 1.0),
        (1286, 1920, f32::NAN, ratio),
        (0, 1920, 0.0, 0.0),
        (1286, 0, 0.0, 0.0),
    ] {
        near(mip_delta(rw, ow, offset).into(), expected, 1e-6);
    }
    for (offset, expected) in [(-0.75, -0.75), (-9.0, -2.0), (3.0, 1.0)] {
        assert_eq!(clamp_mip_offset(offset), expected);
    }
}

fn next_state(
    state: &State,
    scope: bool,
    applied: bool,
    sample: Sample,
    extent: [u32; 2],
    reset: bool,
) -> State {
    next_jitter(state, scope, applied, &sample, extent[0], extent[1], reset)
}

fn published_state() {
    let extent = [1286, 724];
    let [s0, s1, s2] = [0, 1, 2].map(|i| sample(i, 18));
    let mut state = next_state(&State::default(), false, true, s0, extent, true);
    assert!(
        state.frame == 1
            && state.applied
            && !state.previous_applied
            && state.reset
            && state.current_x == s0.x
    );
    let uv = jitter_to_uv(&state);
    near(uv.current_u.into(), f64::from(s0.x) / 1286.0, 1e-7);
    near(uv.current_v.into(), f64::from(s0.y) / 724.0, 1e-7);
    assert_eq!((uv.previous_u, uv.previous_v), (0.0, 0.0));
    state = next_state(&state, false, true, s1, extent, false);
    assert!(state.frame == 2 && state.previous_applied);
    assert_eq!((state.previous_x, state.previous_y), (s0.x, s0.y));
    let uv = jitter_to_uv(&state);
    near(uv.previous_u.into(), f64::from(s0.x) / 1286.0, 1e-7);
    near(uv.current_u.into(), f64::from(s1.x) / 1286.0, 1e-7);
    let scope = next_state(&state, true, false, s2, extent, false);
    assert!(scope.scope && !scope.applied);
    let uv = jitter_to_uv(&scope);
    assert_eq!(
        [uv.current_u, uv.current_v, uv.previous_u, uv.previous_v],
        [0.0; 4]
    );
    state = next_state(&state, false, true, s2, extent, false);
    assert_eq!(
        (
            state.previous_x,
            state.frame,
            state.sequence_index,
            state.phase_count
        ),
        (s1.x, 3, 2, 18)
    );
    state = next_state(&state, false, false, sample(3, 18), extent, false);
    let uv = jitter_to_uv(&state);
    assert!(!state.applied && uv.current_u == 0.0);
    near(uv.previous_u.into(), f64::from(s2.x) / 1286.0, 1e-7);
    let s4 = sample(4, 18);
    state = next_state(&state, false, true, s4, extent, false);
    assert!(!state.previous_applied && jitter_to_uv(&state).previous_u == 0.0);
    let scaled = sample(0, 32);
    state = next_state(&state, false, true, scaled, [960, 540], true);
    let uv = jitter_to_uv(&state);
    near(uv.previous_u.into(), f64::from(s4.x) / 1286.0, 1e-7);
    near(uv.current_u.into(), f64::from(scaled.x) / 960.0, 1e-7);
}

fn motion_cancellation() {
    let (w, h) = (858, 482);
    let view = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, -1.0, 3.0, 1.0,
    ];
    let mut seq = Sequence::default();
    let mut state = State::default();
    let mut previous = None;
    let mut checked = 0;
    for _ in 0..20 {
        let sample = seq.next([w, h, 1280, 720], 0, true);
        state = next_state(&state, false, true, sample, [w, h], seq.reset);
        let projection =
            jitter_projection(&perspective(1280.0 / 720.0), sample.x, sample.y, w, h).unwrap();
        let full = engine_mul(&projection, &view);
        if let Some(previous) = previous {
            let jitter = jitter_to_uv(&state);
            for point in [
                [0.0, 0.0, 5.0, 1.0],
                [-2.0, 1.5, 12.0, 1.0],
                [4.0, -3.0, 60.0, 1.0],
            ] {
                let (now, before) = (uv(clip(point, full)), uv(clip(point, previous)));
                near(
                    now[0] - f64::from(jitter.current_u),
                    before[0] - f64::from(jitter.previous_u),
                    2e-6,
                );
                near(
                    now[1] - f64::from(jitter.current_v),
                    before[1] - f64::from(jitter.previous_v),
                    2e-6,
                );
                near(
                    now[0] - before[0],
                    f64::from(jitter.current_u) - f64::from(jitter.previous_u),
                    2e-6,
                );
                checked += 1;
            }
        }
        previous = Some(full);
    }
    assert_eq!(checked, 57);
}

fn burst_gate() {
    let settle = RSF_ANOMALY_SR_BURST_SETTLE;
    let mut gate = Gate {
        settle,
        ..Gate::default()
    };
    let step = |gate: &mut Gate, loading, without_world, taken| {
        let v = gate.frame(loading, without_world, taken);
        (v.capture, v.discard)
    };
    for _ in 0..1201 {
        assert_eq!(step(&mut gate, true, 7, false), (false, false));
    }
    assert_eq!((gate.clear, gate.restarts), (0, 0));
    for _ in 1..settle {
        assert_eq!(step(&mut gate, false, 7, false), (false, false));
    }
    assert_eq!(gate.clear, u64::from(settle - 1));
    assert_eq!(step(&mut gate, false, 7, false), (true, false));
    for _ in 0..5 {
        assert_eq!(step(&mut gate, false, 7, true), (true, false));
    }
    assert_eq!(step(&mut gate, false, 8, true), (false, true));
    assert_eq!((gate.clear, gate.restarts), (0, 1));
    for _ in 1..settle {
        assert_eq!(step(&mut gate, false, 8, false), (false, false));
    }
    assert_eq!(step(&mut gate, false, 8, false), (true, false));
    assert_eq!(step(&mut gate, true, 8, true), (false, true));
    assert_eq!(gate.restarts, 2);
    for _ in 0..settle {
        step(&mut gate, false, 8, false);
    }
    gate.interrupted();
    assert_eq!((gate.clear, gate.restarts), (0, 3));
    assert_eq!(step(&mut gate, false, 8, false), (false, false));
    let mut quick = Gate {
        settle: 1,
        ..Gate::default()
    };
    assert_eq!(step(&mut quick, false, 0, false), (false, false));
    assert_eq!(step(&mut quick, false, 0, false), (true, false));
    assert_eq!(quick.restarts, 0);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_jitter_fixture(argc: c_int, argv: *const *const c_char) -> i32 {
    // tests/fixture_main_args.c passes the command line; argv[1] selects one case.
    let selection = if argc > 1 {
        unsafe { *argv.add(1) }
    } else {
        std::ptr::null()
    };
    let filter = if selection.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(selection) }.to_bytes())
    };
    let cases: [(&str, fn()); 7] = [
        ("halton", halton_sequence),
        ("sequence", sequence_resets),
        ("ndc", ndc_offset),
        ("mip_bias", mip_bias),
        ("published", published_state),
        ("motion_cancellation", motion_cancellation),
        ("burst_gate", burst_gate),
    ];
    let mut ran = 0;
    let mut failures = 0;
    for (name, run) in cases {
        if filter.is_none_or(|selected| selected == name.as_bytes()) {
            ran += 1;
            if catch_unwind(run).is_err() {
                eprintln!("FAIL {name}");
                failures += 1;
            }
        }
    }
    if ran == 0 {
        eprintln!("unknown case");
        return 2;
    }
    println!("{ran} case(s), {failures} failure(s)");
    i32::from(failures != 0)
}
