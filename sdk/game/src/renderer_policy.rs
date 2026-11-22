// SPDX-License-Identifier: MIT
use crate::{rsf_game_depth_domain, rsf_game_depth_domains};

pub fn depth_domains_requested(value: &rsf_game_depth_domains) -> bool {
    // Every field occupies whole u32 words, including both nested domains.
    // There is no padding to observe, and signed zero and reserved bytes count.
    const {
        assert!(size_of::<rsf_game_depth_domain>() == 26 * 4);
        assert!(size_of::<rsf_game_depth_domains>() == 60 * 4);
    }
    let bytes = unsafe {
        core::slice::from_raw_parts(core::ptr::from_ref(value).cast::<u8>(), size_of_val(value))
    };
    bytes.iter().any(|&byte| byte != 0)
}
