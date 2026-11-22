// SPDX-License-Identifier: MIT
use super::*;

pub(crate) struct Temporal(Box<Driver>);

impl Temporal {
    pub(crate) fn bind(runtime: Raw, vendors: [&CStr; 3], log: rsf_game_log_fn) -> Option<Self> {
        let [streamline, fidelityfx, xess] = vendors.map(CStr::as_ptr);
        let driver = Self(unsafe { Box::from_raw(rsf_anomaly_sr_create()) });
        let initial = Settings {
            backend: DLSS,
            ..DEFAULT_SETTINGS
        };
        let config = rsf_anomaly_sr_driver_config {
            stage: 5,
            first_person_zero: true,
            initial,
            streamline,
            fidelityfx,
            xess,
            ..Default::default()
        };
        unsafe { rsf_anomaly_sr_configure(&driver.0, &config, log, null_mut()) };
        if !unsafe { rsf_anomaly_sr_bind_runtime(&driver.0, runtime) } {
            return None;
        }
        rsf_anomaly_sr_request(&driver.0, &initial);
        Some(driver)
    }

    pub(crate) fn boundary(&self, device: Raw, [width, height]: [u32; 2]) {
        unsafe { rsf_anomaly_sr_boundary(&self.0, device, width, height) }
    }

    pub(crate) fn plan(&self, [width, height]: [u32; 2]) -> Option<[u32; 2]> {
        let (mut rw, mut rh) = (0, 0);
        unsafe { rsf_anomaly_sr_plan(&self.0, width, height, &mut rw, &mut rh) }.then_some([rw, rh])
    }

    #[cfg(all(feature = "fg-fixture", not(test)))]
    pub(crate) fn reason(&self) -> String {
        let status = self.0.published().status;
        unsafe { CStr::from_ptr(status.reason.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) fn output_format(&self) -> u32 {
        unsafe { rsf_anomaly_sr_output_format(&self.0) }
    }

    #[cfg(all(feature = "fg-fixture", not(test)))]
    pub(crate) fn evaluate(&self, context: Raw, inputs: &mut PassInputs) -> bool {
        self.evaluate_with_sequence(context, inputs, false)
    }

    pub(crate) fn evaluate_with_sequence(
        &self,
        context: Raw,
        inputs: &mut PassInputs,
        sequence: bool,
    ) -> bool {
        unsafe {
            rsf_anomaly_sr_evaluate(
                &self.0,
                context,
                inputs,
                &ResetFacts {
                    sequence,
                    ..Default::default()
                },
                null_mut(),
                null_mut(),
            )
        }
    }

    #[cfg(feature = "sr-fixtures")]
    pub(crate) fn request(&self, backend: u32) {
        rsf_anomaly_sr_request(
            &self.0,
            &Settings {
                backend,
                ..DEFAULT_SETTINGS
            },
        );
    }

    #[cfg(feature = "sr-fixtures")]
    pub(crate) fn selection(&self) -> (u32, u32, i32) {
        let status = self.0.published().status;
        (
            status.pending_backend,
            status.applied.backend,
            status.select_result,
        )
    }

    #[cfg(feature = "sr-fixtures")]
    pub(crate) fn running(&self) -> (bool, u32) {
        let status = self.0.published().status;
        (status.running, status.pipeline.running)
    }

    #[cfg(feature = "sr-fixtures")]
    pub(crate) fn summary(&self) -> rsf_overlay_sr_report {
        let counters = self.0.published().counters;
        println!(
            "evaluated DLSS={} FSR3={} FSR4={} XeSS={}, refused pipeline={} pass={}, resets first={} settings={} sequence={}",
            counters.evaluated[DLSS as usize],
            counters.evaluated[FSR3 as usize],
            counters.evaluated[FSR4 as usize],
            counters.evaluated[XESS as usize],
            counters.refused[Refusal::Pipeline as usize],
            counters.refused[Refusal::Pass as usize],
            counters.resets[ResetReason::First as usize],
            counters.resets[ResetReason::Settings as usize],
            counters.resets[ResetReason::Sequence as usize],
        );
        let mut report = rsf_overlay_sr_report::default();
        rsf_anomaly_sr_overlay_report(&self.0, &mut report);
        report
    }

    #[cfg(feature = "sr-fixtures")]
    pub(crate) fn dump(&self, prefix: &CStr) -> bool {
        unsafe { rsf_anomaly_sr_request_dump(&self.0, prefix.as_ptr()) }
    }

    #[cfg(feature = "sr-fixtures")]
    pub(crate) fn stop(&self) {
        unsafe { rsf_anomaly_sr_stop(&self.0) };
    }
}

#[cfg(feature = "sr-fixtures")]
impl Drop for Temporal {
    fn drop(&mut self) {
        self.stop();
    }
}
