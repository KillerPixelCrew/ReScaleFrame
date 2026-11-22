// SPDX-License-Identifier: MIT
//! Opt-in XeSS timing for both the Runtime and standalone bridge.
use std::sync::{Mutex, PoisonError};

const PHASES: usize = 6;
const SEQUENCE: usize = 24;
const PRESENTS_PER_REPORT: u32 = 120;

#[repr(u32)]
pub(super) enum Phase {
    AllocatorWait = 0,
    Record = 1,
    Submit = 2,
    Present = 3,
    Retire = 4,
}

#[repr(C)]
pub(crate) struct GpuSample {
    pub(crate) start: u64,
    pub(crate) copies_end: u64,
    pub(crate) end: u64,
    pub(crate) frequency: u64,
    pub(crate) calibration_gpu: u64,
    pub(crate) calibration_cpu: u64,
    pub(crate) present_enter: i64,
    pub(crate) present_exit: i64,
}

#[repr(C)]
pub(super) struct Report {
    presents: u32,
    gpu_samples: u32,
    sequence_count: u32,
    reserved: u32,
    phase_mean: [f64; PHASES],
    phase_max: [f64; PHASES],
    gpu_mean: f64,
    gpu_max: f64,
    gpu_start_after_present_enter: f64,
    present_exit_after_gpu_end: f64,
    copies_mean: f64,
    sequence: [f64; SEQUENCE],
}

const _: () = {
    assert!(size_of::<GpuSample>() == 64);
    assert!(size_of::<Report>() == 344);
    assert!(std::mem::offset_of!(Report, phase_mean) == 16);
    assert!(std::mem::offset_of!(Report, sequence) == 152);
};

#[derive(Default)]
struct Batch {
    phase_count: [u64; PHASES],
    phase_sum: [f64; PHASES],
    phase_max: [f64; PHASES],
    presents: u32,
    gpu_sum: f64,
    gpu_max: f64,
    copies_sum: f64,
    copies_samples: u32,
    gpu_start_after_present_enter: f64,
    present_exit_after_gpu_end: f64,
    gpu_samples: u32,
    sequence: [f64; SEQUENCE],
    sequence_count: usize,
}

impl Batch {
    fn cpu_sample(&mut self, phase: u32, start: i64, end: i64, frequency: i64) {
        let phase = phase as usize;
        if phase >= PHASES || frequency <= 0 || start <= 0 {
            return;
        }
        let Some(ticks) = end.checked_sub(start).filter(|&ticks| ticks >= 0) else {
            return;
        };
        let ms = ticks as f64 * 1000.0 / frequency as f64;
        self.phase_sum[phase] += ms;
        self.phase_count[phase] += 1;
        self.phase_max[phase] = self.phase_max[phase].max(ms);
        if phase == Phase::Present as usize && self.sequence_count < SEQUENCE {
            self.sequence[self.sequence_count] = ms;
            self.sequence_count += 1;
        }
    }

    fn gpu_sample(&mut self, sample: &GpuSample, cpu_frequency: i64) {
        if sample.end <= sample.start
            || sample.frequency == 0
            || cpu_frequency <= 0
            || sample.present_enter <= 0
            || sample.present_exit < sample.present_enter
        {
            return;
        }
        let gpu_frequency = sample.frequency as f64;
        let cpu_frequency = cpu_frequency as f64;
        let to_cpu = |gpu| {
            sample.calibration_cpu as f64
                - (sample.calibration_gpu as f64 - gpu as f64) * cpu_frequency / gpu_frequency
        };
        let ms_per_cpu_tick = 1000.0 / cpu_frequency;
        let duration = (sample.end - sample.start) as f64 * 1000.0 / gpu_frequency;
        self.gpu_sum += duration;
        self.gpu_max = self.gpu_max.max(duration);
        if (sample.start..=sample.end).contains(&sample.copies_end) {
            self.copies_samples += 1;
            self.copies_sum += (sample.copies_end - sample.start) as f64 * 1000.0 / gpu_frequency;
        }
        self.gpu_start_after_present_enter +=
            (to_cpu(sample.start) - sample.present_enter as f64) * ms_per_cpu_tick;
        self.present_exit_after_gpu_end +=
            (sample.present_exit as f64 - to_cpu(sample.end)) * ms_per_cpu_tick;
        self.gpu_samples += 1;
    }

    fn end_present(&mut self) -> Option<Report> {
        self.presents += 1;
        if self.presents < PRESENTS_PER_REPORT {
            return None;
        }
        let batch = std::mem::take(self);
        let gpu_samples = f64::from(batch.gpu_samples.max(1));
        Some(Report {
            presents: batch.presents,
            gpu_samples: batch.gpu_samples,
            sequence_count: batch.sequence_count as u32,
            reserved: 0,
            phase_mean: std::array::from_fn(|i| {
                batch.phase_sum[i] / batch.phase_count[i].max(1) as f64
            }),
            phase_max: batch.phase_max,
            gpu_mean: batch.gpu_sum / gpu_samples,
            gpu_max: batch.gpu_max,
            gpu_start_after_present_enter: batch.gpu_start_after_present_enter / gpu_samples,
            present_exit_after_gpu_end: batch.present_exit_after_gpu_end / gpu_samples,
            copies_mean: batch.copies_sum / f64::from(batch.copies_samples.max(1)),
            sequence: batch.sequence,
        })
    }
}

// Kept at a stable address by the session. The GPU callback and XeLL
// sleep can sample it without borrowing the mutable presentation owner.
#[derive(Default)]
pub(crate) struct Accumulator(Mutex<Batch>);

impl Accumulator {
    pub(super) fn cpu_sample(&self, phase: u32, start: i64, end: i64, frequency: i64) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cpu_sample(phase, start, end, frequency);
    }

    pub(super) fn end_present(&self) -> Option<Report> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .end_present()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xess_timing_cpu(
    timing: &Accumulator,
    phase: u32,
    start: i64,
    end: i64,
    frequency: i64,
) {
    timing.cpu_sample(phase, start, end, frequency);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_xess_timing_gpu(
    timing: &Accumulator,
    sample: &GpuSample,
    cpu_frequency: i64,
) {
    timing
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .gpu_sample(sample, cpu_frequency);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_cpu_and_calibrated_gpu_samples_at_the_batch_boundary() {
        let mut batch = Batch::default();
        batch.cpu_sample(Phase::Present as u32, 10, 20, 1000);
        batch.gpu_sample(
            &GpuSample {
                start: 1200,
                copies_end: 1300,
                end: 1500,
                frequency: 1000,
                calibration_gpu: 1000,
                calibration_cpu: 10_000,
                present_enter: 10_100,
                present_exit: 10_600,
            },
            1000,
        );
        for _ in 1..PRESENTS_PER_REPORT {
            assert!(batch.end_present().is_none());
        }
        let report = batch.end_present().unwrap();
        assert_eq!(report.presents, PRESENTS_PER_REPORT);
        assert_eq!(report.sequence_count, 1);
        assert_eq!(report.sequence[0], 10.0);
        assert_eq!(report.phase_mean[Phase::Present as usize], 10.0);
        assert_eq!(report.gpu_samples, 1);
        assert_eq!(report.gpu_mean, 300.0);
        assert_eq!(report.copies_mean, 100.0);
        assert_eq!(report.gpu_start_after_present_enter, 100.0);
        assert_eq!(report.present_exit_after_gpu_end, 100.0);
        assert_eq!(batch.sequence_count, 0);
        assert!(batch.end_present().is_none());
    }
}
