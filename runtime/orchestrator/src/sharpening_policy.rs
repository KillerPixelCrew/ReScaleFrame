// SPDX-License-Identifier: MIT
const RCAS: u32 = 1;
const DEPTH_RCAS: u32 = 2;
const DEPTH_DAS: u32 = 3;
pub(super) const FILTERS: [u32; 3] = [RCAS, DEPTH_RCAS, DEPTH_DAS];

// Modes that need depth use the same RCAS fallback on each graphics API.
pub(super) fn effective_mode(mode: u32, has_depth: bool) -> Option<u32> {
    match mode {
        RCAS => Some(RCAS),
        DEPTH_RCAS | DEPTH_DAS if has_depth => Some(mode),
        DEPTH_RCAS | DEPTH_DAS => Some(RCAS),
        _ => None,
    }
}
