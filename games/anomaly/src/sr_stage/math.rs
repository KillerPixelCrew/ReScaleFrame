// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_game_sys::sampling::halton_f64 as halton;
use std::mem::MaybeUninit;

type Plan = rsf_anomaly_sr_plan;
type Sample = rsf_anomaly_sr_jitter_sample;
type Published = rsf_anomaly_sr_jitter_state;
type Uv = rsf_anomaly_sr_jitter_uv;
pub(crate) type JitterSequence = rsf_anomaly_sr_jitter_sequence;
pub(super) type BurstGate = rsf_anomaly_sr_burst_gate;
pub(super) const DEFAULT_SCALE: f32 = RSF_ANOMALY_SR_DEFAULT_SCALE as f32;
pub(super) const DEFAULT_MIP_OFFSET: f32 = RSF_ANOMALY_SR_DEFAULT_MIP_OFFSET as f32;
const MIN_SCALE: f32 = RSF_ANOMALY_SR_MIN_SCALE as f32;
const MAX_SCALE: f32 = RSF_ANOMALY_SR_MAX_SCALE as f32;
const MIN_MIP_OFFSET: f32 = RSF_ANOMALY_SR_MIN_MIP_OFFSET as f32;
const MAX_MIP_OFFSET: f32 = RSF_ANOMALY_SR_MAX_MIP_OFFSET as f32;

pub(super) fn clamp_scale(scale: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        DEFAULT_SCALE
    } else {
        scale.clamp(MIN_SCALE, MAX_SCALE)
    }
}

fn minimum_extent(output: u32) -> u32 {
    (f64::from(output) * f64::from(MIN_SCALE)).ceil() as u32
}

fn render_extent(output: u32, scale: f32) -> u32 {
    let rounded = (f64::from(output) * f64::from(scale) + 0.5).floor() as u32;
    rounded.clamp(minimum_extent(output), output)
}

pub(crate) fn make_plan(ow: u32, oh: u32, scale: f32) -> Option<Plan> {
    if ow == 0 || oh == 0 {
        return None;
    }
    let scale = clamp_scale(scale);
    Some(Plan {
        output_width: ow,
        output_height: oh,
        render_width: render_extent(ow, scale),
        render_height: render_extent(oh, scale),
        scale,
    })
}

pub(crate) fn plan_for_render(ow: u32, oh: u32, rw: u32, rh: u32) -> Option<Plan> {
    if ow == 0
        || oh == 0
        || rw > ow
        || rh > oh
        || rw < minimum_extent(ow)
        || rh < minimum_extent(oh)
    {
        return None;
    }
    Some(Plan {
        output_width: ow,
        output_height: oh,
        render_width: rw,
        render_height: rh,
        scale: (f64::from(rw) / f64::from(ow)) as f32,
    })
}

pub(super) fn clamp_mip_offset(offset: f32) -> f32 {
    if !offset.is_finite() {
        DEFAULT_MIP_OFFSET
    } else {
        offset.clamp(MIN_MIP_OFFSET, MAX_MIP_OFFSET)
    }
}

pub(super) fn mip_delta(render: u32, output: u32, offset: f32) -> f32 {
    if render == 0 || output == 0 {
        return 0.0;
    }
    let delta =
        (f64::from(render) / f64::from(output)).log2() + f64::from(clamp_mip_offset(offset));
    delta.clamp(-3.0, 1.0) as f32
}

fn phase_count(render: u32, output: u32) -> u32 {
    if render == 0 || render > output {
        return 8;
    }
    let ratio = f64::from(output) / f64::from(render);
    // Clamp before conversion so extreme extents use the 64-phase cap.
    (8.0 * ratio * ratio - 1e-9).ceil().clamp(8.0, 64.0) as u32
}

pub(crate) fn jitter_sample(index: u32, phases: u32) -> Sample {
    let phases = if phases == 0 { 8 } else { phases };
    let index = index % phases;
    Sample {
        x: (halton(index + 1, 2) - 0.5) as f32,
        y: (halton(index + 1, 3) - 0.5) as f32,
        index,
        phase_count: phases,
    }
}

impl JitterSequence {
    pub(crate) fn new() -> Self {
        Self {
            phases: 8,
            ..Self::default()
        }
    }

    pub(crate) fn next(
        &mut self,
        dimensions: [u32; 4],
        generation: u64,
        continuous: bool,
    ) -> Sample {
        let phases = phase_count(dimensions[0], dimensions[2]);
        self.reset = !self.started
            || self.dimensions != dimensions
            || self.generation != generation
            || self.phases != phases
            || !continuous;
        if self.reset {
            self.resets = self.resets.wrapping_add(u64::from(self.started));
            self.started = true;
            self.dimensions = dimensions;
            self.generation = generation;
            self.phases = phases;
            self.index = 0;
        }
        let sample = jitter_sample(self.index, self.phases);
        self.index = (self.index + 1) % self.phases;
        self.frames = self.frames.wrapping_add(1);
        sample
    }
}

pub(crate) fn next_jitter(
    last: &Published,
    scope: bool,
    applied: bool,
    sample: &Sample,
    rw: u32,
    rh: u32,
    reset: bool,
) -> Published {
    if scope {
        Published {
            scope,
            applied: false,
            previous_applied: false,
            current_x: 0.0,
            current_y: 0.0,
            previous_x: 0.0,
            previous_y: 0.0,
            ..*last
        }
    } else {
        Published {
            frame: last.frame.wrapping_add(1),
            sequence_index: sample.index,
            phase_count: sample.phase_count,
            current_x: if applied { sample.x } else { 0.0 },
            current_y: if applied { sample.y } else { 0.0 },
            previous_x: last.current_x,
            previous_y: last.current_y,
            render_width: rw,
            render_height: rh,
            previous_render_width: last.render_width,
            previous_render_height: last.render_height,
            scope,
            applied,
            previous_applied: last.applied,
            reset,
        }
    }
}

pub(super) fn jitter_to_uv(state: &Published) -> Uv {
    let pair = |enabled, x, y, width: u32, height: u32| {
        if enabled && width != 0 && height != 0 {
            (x / width as f32, y / height as f32)
        } else {
            (0.0, 0.0)
        }
    };
    let (current_u, current_v) = pair(
        state.applied,
        state.current_x,
        state.current_y,
        state.render_width,
        state.render_height,
    );
    let (previous_u, previous_v) = pair(
        state.previous_applied,
        state.previous_x,
        state.previous_y,
        state.previous_render_width,
        state.previous_render_height,
    );
    Uv {
        current_u,
        current_v,
        previous_u,
        previous_v,
    }
}

impl BurstGate {
    pub(super) fn new() -> Self {
        Self {
            settle: RSF_ANOMALY_SR_BURST_SETTLE,
            ..Self::default()
        }
    }

    pub(super) fn frame(
        &mut self,
        loading: bool,
        without_world: u64,
        taken: bool,
    ) -> rsf_anomaly_sr_burst_verdict {
        let broken = loading || !self.marked || without_world != self.mark;
        self.mark = without_world;
        self.marked = true;
        if broken {
            self.clear = 0;
            self.held = self.held.wrapping_add(1);
            self.restarts = self.restarts.wrapping_add(u64::from(taken));
            return rsf_anomaly_sr_burst_verdict {
                capture: false,
                discard: taken,
            };
        }
        self.clear = self.clear.wrapping_add(1);
        rsf_anomaly_sr_burst_verdict {
            capture: taken || self.clear >= u64::from(self.settle),
            discard: false,
        }
    }

    pub(super) fn interrupted(&mut self) {
        self.clear = 0;
        self.held = self.held.wrapping_add(1);
        self.restarts = self.restarts.wrapping_add(1);
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_clamp_mip_offset(value: f32) -> f32 {
    clamp_mip_offset(value)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_mip_delta(render: u32, output: u32, offset: f32) -> f32 {
    mip_delta(render, output, offset)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_phase_count(render: u32, output: u32) -> u32 {
    phase_count(render, output)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_jitter_sample_at(index: u32, phases: u32) -> Sample {
    jitter_sample(index, phases)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_sequence_next(
    sequence: &mut JitterSequence,
    dimensions: &[u32; 4],
    generation: u64,
    continuous: bool,
) -> Sample {
    sequence.next(*dimensions, generation, continuous)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_next_jitter(
    last: &Published,
    scope: bool,
    applied: bool,
    sample: &Sample,
    rw: u32,
    rh: u32,
    reset: bool,
    out: &mut MaybeUninit<Published>,
) {
    out.write(next_jitter(last, scope, applied, sample, rw, rh, reset));
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_jitter_to_uv(state: &Published, out: &mut MaybeUninit<Uv>) {
    out.write(jitter_to_uv(state));
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_burst_frame(
    gate: &mut BurstGate,
    loading: bool,
    without_world: u64,
    taken: bool,
) -> rsf_anomaly_sr_burst_verdict {
    gate.frame(loading, without_world, taken)
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_sr_burst_interrupted(gate: &mut BurstGate) {
    gate.interrupted();
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    #[test]
    fn scale_defaults_and_bounds() {
        for scale in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            assert_eq!(clamp_scale(scale), 0.67);
        }
        for (requested, expected) in [
            (0.30, 0.33),
            (0.33, 0.33),
            (0.5, 0.5),
            (0.67, 0.67),
            (1.0, 1.0),
            (1.5, 1.0),
            (100.0, 1.0),
        ] {
            assert_eq!(clamp_scale(requested), expected);
        }
    }

    #[test]
    fn backend_render_sizes_keep_the_requested_rectangles() {
        for (rw, rh) in [(1280, 720), (960, 540), (1920, 1080), (640, 360)] {
            let plan = plan_for_render(1920, 1080, rw, rh).unwrap();
            assert_eq!([plan.render_width, plan.render_height], [rw, rh]);
            assert!((f64::from(plan.scale) - f64::from(rw) / 1920.0).abs() < 1e-6);
        }
        assert!(plan_for_render(1920, 1080, 600, 340).is_none());
        assert!(plan_for_render(1920, 1080, 2000, 1080).is_none());
        assert!(plan_for_render(0, 1080, 960, 540).is_none());
        assert!(plan_for_render(1921, 1081, 634, 356).is_none());
        assert!(plan_for_render(1921, 1081, 634, 357).is_some());
    }

    #[test]
    fn render_sizes_round_per_axis_and_stay_in_bounds() {
        for (output, scale, render) in [
            ([1920, 1080], 0.67, [1286, 724]),
            ([2560, 1440], 0.67, [1715, 965]),
            ([3840, 2160], 0.77, [2957, 1663]),
            ([1280, 720], 0.67, [858, 482]),
            ([1919, 1079], 0.67, [1286, 723]),
            ([1921, 1081], 0.5, [961, 541]),
            ([1920, 1080], 0.5, [960, 540]),
            ([1920, 1080], 0.3, [634, 357]),
            ([2560, 1600], 1.0 / 3.0, [853, 533]),
            ([1, 2], 0.5, [1, 1]),
            ([3, 1], 0.5, [2, 1]),
            ([1, 1], 0.67, [1, 1]),
            ([7, 7], 2.0, [7, 7]),
            ([1920, 1080], 7.0, [1920, 1080]),
            ([1920, 1080], f32::NAN, [1286, 724]),
        ] {
            let plan = make_plan(output[0], output[1], scale).unwrap();
            assert_eq!([plan.output_width, plan.output_height], output);
            assert_eq!([plan.render_width, plan.render_height], render);
            assert_eq!(plan.scale, clamp_scale(scale));
        }
        assert_eq!(render_extent(0, 0.67), 0);
        assert!(make_plan(0, 1080, 0.67).is_none());
        assert!(make_plan(1920, 0, 0.67).is_none());
        for scale in [
            0.34, 0.4, 0.5, 0.55, 0.6, 0.6667, 0.67, 0.75, 0.77, 0.9, 1.0,
        ] {
            for output in 1..=8192 {
                let render = render_extent(output, scale);
                let exact = f64::from(output) * f64::from(scale);
                let lowest = (f64::from(output) * f64::from(0.33f32)).ceil() as u32;
                assert!((lowest..=output).contains(&render));
                assert!(render == lowest || (f64::from(render) - exact).abs() <= 0.5 + 1e-9);
            }
        }
    }
}
