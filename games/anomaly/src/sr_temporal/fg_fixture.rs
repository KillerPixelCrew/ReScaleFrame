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

    pub(crate) fn reason(&self) -> String {
        let status = self.0.published().status;
        unsafe { CStr::from_ptr(status.reason.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) fn output_format(&self) -> u32 {
        unsafe { rsf_anomaly_sr_output_format(&self.0) }
    }

    pub(crate) fn evaluate(&self, context: Raw, inputs: &mut PassInputs) -> bool {
        unsafe {
            rsf_anomaly_sr_evaluate(
                &self.0,
                context,
                inputs,
                &ResetFacts::default(),
                null_mut(),
                null_mut(),
            )
        }
    }
}
