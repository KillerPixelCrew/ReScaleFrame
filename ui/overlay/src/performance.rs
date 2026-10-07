//! Rates derived from real application and SDK present counters on the host QPC clock.
use crate::model::Stats;

/// Frames per second over the latest completed sampling window; `None` before a valid window.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Rates {
    pub rendered: Option<f64>,
    pub presented: Option<f64>,
}
/// Counters from one QPC sample, including which presentation counter supplied the total.
#[derive(Clone, Copy)]
struct Sample {
    tick: u64,
    frequency: u64,
    rendered: u64,
    presented: u64,
    sdk: bool,
}
/// Rate estimator that resets on invalid/changed clocks, counter rollback or a change between
/// SDK and application-counter sampling. Provider IDs are not inspected.
#[derive(Default)]
pub(crate) struct Meter {
    baseline: Option<Sample>,
    rates: Rates,
}
impl Meter {
    /// Update at most every half second. Without an SDK total, both rates use application presents.
    /// A reset requires a fresh baseline and window before either rate is available again.
    pub fn sample(&mut self, stats: &Stats<'_>) -> Rates {
        let sample = Sample {
            tick: stats.counter_clock[0],
            frequency: stats.counter_clock[1],
            rendered: stats.application_presented_frames,
            presented: if stats.fg_present_count_valid {
                stats.generation.total_presented
            } else {
                stats.application_presented_frames
            },
            sdk: stats.fg_present_count_valid,
        };
        if sample.tick == 0 || sample.frequency == 0 {
            self.baseline = None;
            self.rates = Rates::default();
            return self.rates;
        }
        let Some(previous) = self.baseline else {
            self.baseline = Some(sample);
            return self.rates;
        };
        if sample.tick < previous.tick
            || sample.frequency != previous.frequency
            || sample.sdk != previous.sdk
            || sample.rendered < previous.rendered
            || sample.presented < previous.presented
        {
            self.baseline = Some(sample);
            self.rates = Rates::default();
            return self.rates;
        }
        let elapsed = (sample.tick - previous.tick) as f64 / sample.frequency as f64;
        if elapsed >= 0.5 {
            self.rates = Rates {
                rendered: Some((sample.rendered - previous.rendered) as f64 / elapsed),
                presented: Some((sample.presented - previous.presented) as f64 / elapsed),
            };
            self.baseline = Some(sample);
        }
        self.rates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_observed_counts_and_resets_when_sdk_reporting_changes() {
        let mut meter = Meter::default();
        let mut stats = Stats {
            counter_clock: [1_000, 1_000],
            ..Stats::default()
        };
        assert!(meter.sample(&stats).rendered.is_none());
        stats.counter_clock[0] = 2_000;
        stats.application_presented_frames = 60;
        assert_eq!(meter.sample(&stats).presented, Some(60.0));
        stats.fg_present_count_valid = true;
        stats.generation.total_presented = 100;
        assert!(meter.sample(&stats).presented.is_none());
        stats.counter_clock[0] = 3_000;
        stats.application_presented_frames = 120;
        stats.generation.total_presented = 220;
        let rates = meter.sample(&stats);
        assert_eq!(rates.rendered, Some(60.0));
        assert_eq!(rates.presented, Some(120.0));
        // Requested 2x does not invent a doubled rate when the SDK only presents real frames.
        stats.generation.requested_mode = 1;
        stats.counter_clock[0] = 4_000;
        stats.application_presented_frames = 180;
        stats.generation.total_presented = 280;
        assert_eq!(meter.sample(&stats).presented, Some(60.0));
        stats.application_presented_frames = 0;
        assert!(meter.sample(&stats).rendered.is_none());
    }
}
