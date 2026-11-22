//! Rates derived from real application and SDK present counters on the host QPC clock.
use crate::model::Stats;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Rates {
    pub rendered: Option<f64>,
    pub presented: Option<f64>,
}
#[derive(Clone, Copy)]
struct Sample {
    tick: u64,
    frequency: u64,
    count: u64,
}
#[derive(Default)]
struct Counter {
    baseline: Option<Sample>,
    rate: Option<f64>,
}
impl Counter {
    fn sample(&mut self, [tick, frequency]: [u64; 2], count: u64) -> Option<f64> {
        let sample = Sample {
            tick,
            frequency,
            count,
        };
        if tick == 0 || frequency == 0 {
            *self = Self::default();
            return None;
        }
        let Some(previous) = self.baseline else {
            self.baseline = Some(sample);
            return None;
        };
        if tick < previous.tick || frequency != previous.frequency || count < previous.count {
            self.baseline = Some(sample);
            self.rate = None;
            return None;
        }
        let elapsed = (tick - previous.tick) as f64 / frequency as f64;
        if elapsed >= 0.5 {
            self.rate = Some((count - previous.count) as f64 / elapsed);
            self.baseline = Some(sample);
        }
        self.rate
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PresentSource {
    Application,
    Aggregate,
}

#[derive(Default)]
pub(crate) struct Meter {
    rendered: Counter,
    presented: Counter,
    present_source: Option<PresentSource>,
}

impl Meter {
    pub fn sample(&mut self, stats: &Stats<'_>) -> Rates {
        let source = if stats.fg_present_count_valid {
            Some(PresentSource::Aggregate)
        } else if !stats.generation.active {
            Some(PresentSource::Application)
        } else {
            None
        };
        if source != self.present_source {
            self.presented = Counter::default();
            self.present_source = source;
        }
        Rates {
            rendered: self
                .rendered
                .sample(stats.counter_clock, stats.application_presented_frames),
            presented: source.and_then(|source| {
                let count = match source {
                    PresentSource::Application => stats.application_presented_frames,
                    PresentSource::Aggregate => stats.generation.total_presented,
                };
                self.presented.sample(stats.counter_clock, count)
            }),
        }
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
        stats.generation.active = true;
        stats.fg_present_count_valid = false;
        assert_eq!(meter.sample(&stats).presented, None);
        stats.counter_clock[0] = 5_000;
        stats.application_presented_frames = 240;
        assert_eq!(
            meter.sample(&stats),
            Rates {
                rendered: Some(60.0),
                presented: None,
            }
        );
        stats.generation.active = false;
        assert_eq!(meter.sample(&stats).presented, None);
        stats.counter_clock[0] = 6_000;
        stats.application_presented_frames = 300;
        assert_eq!(meter.sample(&stats).presented, Some(60.0));
        stats.application_presented_frames = 0;
        assert!(meter.sample(&stats).rendered.is_none());
    }
}
