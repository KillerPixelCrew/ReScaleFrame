// SPDX-License-Identifier: MIT
#[derive(Default)]
pub(super) struct Rates {
    tick: u64,
    base: u64,
    output: u64,
    backend: u32,
    pub(super) base_fps: f32,
    pub(super) output_fps: f32,
    pub(super) output_valid: bool,
}

impl Rates {
    pub(super) fn sample(
        &mut self,
        presents: u64,
        backend: u32,
        output: u64,
        valid: bool,
        now: u64,
    ) {
        if self.tick == 0 || self.backend != backend || output < self.output {
            *self = Self {
                tick: now,
                base: presents,
                output,
                backend,
                ..Self::default()
            };
        } else if now.wrapping_sub(self.tick) >= 500 {
            let seconds = now.wrapping_sub(self.tick) as f32 / 1000.0;
            self.base_fps = presents.wrapping_sub(self.base) as f32 / seconds;
            self.output_valid = valid;
            self.output_fps = if self.output_valid {
                (output - self.output) as f32 / seconds
            } else {
                0.0
            };
            self.tick = now;
            self.base = presents;
            self.output = output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rates;

    #[test]
    fn rates_restart_on_provider_switch_and_output_counter_reset() {
        let mut rates = Rates::default();
        rates.sample(u64::MAX, 1, 10, true, 1);
        rates.sample(29, 1, 70, true, 501);
        assert_eq!(
            (rates.base_fps, rates.output_fps, rates.output_valid),
            (60.0, 120.0, true)
        );
        rates.sample(59, 2, 100, true, 1001);
        assert_eq!(
            (rates.base_fps, rates.output_fps, rates.output_valid),
            (0.0, 0.0, false)
        );
        rates.sample(89, 2, 160, false, 1501);
        assert_eq!(
            (rates.base_fps, rates.output_fps, rates.output_valid),
            (60.0, 0.0, false)
        );
        rates.sample(119, 2, 4, true, 2001);
        assert_eq!(
            (rates.base_fps, rates.output_fps, rates.output_valid),
            (0.0, 0.0, false)
        );
    }
}
