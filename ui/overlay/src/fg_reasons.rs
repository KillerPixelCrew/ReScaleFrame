// SPDX-License-Identifier: MIT
use std::ffi::CStr;

use rsf_game_sys::*;

pub(crate) fn description(reason: u32) -> Option<&'static CStr> {
    Some(match reason {
        RSF_NATIVE_FG_REASON_NONE => return None,
        RSF_NATIVE_FG_REASON_NO_MATCH => c"Waiting for a matching scene and window",
        RSF_NATIVE_FG_REASON_NOT_PREPARED => c"Waiting for reconstruction inputs",
        RSF_NATIVE_FG_REASON_NO_SUBMISSION => c"Waiting for a complete frame submission",
        RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED => c"Waiting for an eligible scene",
        RSF_NATIVE_FG_REASON_RESET => c"Resetting temporal history",
        RSF_NATIVE_FG_REASON_VSYNC => c"Current VSync mode is unsupported",
        RSF_NATIVE_FG_REASON_INVALID_INPUT => c"Frame generation input is invalid",
        RSF_NATIVE_FG_REASON_HUDLESS_MISSING => c"Waiting for HUD-less scene color",
        _ => c"Frame generation deferred for an unknown reason",
    })
}

pub(crate) fn text(reason: u32) -> Option<&'static str> {
    description(reason).map(|text| text.to_str().expect("static FG reason text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_and_unknown_reasons_are_reported_explicitly() {
        assert_eq!(description(RSF_NATIVE_FG_REASON_NONE), None);
        assert_eq!(
            description(RSF_NATIVE_FG_REASON_INVALID_INPUT),
            Some(c"Frame generation input is invalid")
        );
        assert_eq!(
            description(u32::MAX),
            Some(c"Frame generation deferred for an unknown reason")
        );
    }
}
