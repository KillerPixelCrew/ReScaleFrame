// SPDX-License-Identifier: MIT

/// D3D12 frame-generation results from the Runtime session and presentation switch service.
pub struct PresentationResult {
    pub session: i32,
    pub provider_switch: i32,
}

impl PresentationResult {
    /// The legacy aggregate reports an attempted switch failure first.
    pub fn aggregate(&self) -> i32 {
        if self.provider_switch != 0 {
            self.provider_switch
        } else {
            self.session
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_switch_failure_takes_precedence() {
        for (session, provider_switch, aggregate) in
            [(0, 0, 0), (-5, 0, -5), (0, -3, -3), (-5, -3, -3)]
        {
            let result = PresentationResult {
                session,
                provider_switch,
            };
            assert_eq!(result.aggregate(), aggregate);
        }
    }
}
