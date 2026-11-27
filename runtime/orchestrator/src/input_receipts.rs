// SPDX-License-Identifier: MIT
//! Unframed input receipt policy: the replay window the CPU dispatcher applies
//! to receipt IDs and the owner selection the Direct3D pacing owner applies
//! before routing a receipt to a provider frame.

use rsf_game_sys::{RSF_GAME_INPUT_CONTROLLER, RSF_GAME_INPUT_KEYBOARD, RSF_GAME_INPUT_MOUSE};

pub(crate) const PCL_PING: u32 = 8;
pub(crate) const CONTROLLER_SAMPLE: u32 = 13;

pub(crate) struct StreamlineInput {
    pub controller: Option<u32>,
    pub ping: Option<u32>,
    pub foreign_message: bool,
    pub empty: bool,
}

pub(crate) fn streamline_input(kinds: u32, message: u32, ping: u32) -> Option<StreamlineInput> {
    const KINDS: u32 = RSF_GAME_INPUT_KEYBOARD | RSF_GAME_INPUT_MOUSE | RSF_GAME_INPUT_CONTROLLER;
    if kinds & !KINDS != 0 {
        return None;
    }
    Some(StreamlineInput {
        controller: (kinds & RSF_GAME_INPUT_CONTROLLER != 0).then_some(CONTROLLER_SAMPLE),
        ping: (message != 0 && message == ping).then_some(PCL_PING),
        foreign_message: message != 0 && message != ping,
        empty: kinds == 0 && message == 0,
    })
}

/// Receipt IDs accepted within the last 64 of the highest one seen. Bounded
/// callback reordering is admitted once; a repeat or an older ID is refused.
/// The owner serializes access.
#[derive(Clone, Copy, Default)]
pub(crate) struct Window {
    highest: u64,
    seen: u64,
}

impl Window {
    pub(crate) const fn new() -> Self {
        Self {
            highest: 0,
            seen: 0,
        }
    }

    pub(crate) fn accept(&mut self, id: u64) -> bool {
        if id == 0 {
            return false;
        }
        if id > self.highest {
            let distance = id - self.highest;
            self.seen = if distance >= 64 {
                1
            } else {
                (self.seen << distance) | 1
            };
            self.highest = id;
            return true;
        }
        let distance = self.highest - id;
        if distance >= 64 || self.seen & (1u64 << distance) != 0 {
            return false;
        }
        self.seen |= 1u64 << distance;
        true
    }
}

/// A provider frame that might own a receipt. Zeroed rows are ineligible.
#[derive(Clone, Copy, Default)]
pub(crate) struct Owner {
    pub source: u64,
    pub session: u64,
    pub generation: u64,
    pub ready: u64,
    pub simulation_begin: u64,
    pub eligible: bool,
}

/// The single eligible source of this session and provider generation whose
/// pacing and actual simulation began by `timestamp`, or 0. Ambiguity is not
/// permission to choose the newest source.
pub(crate) fn owner(owners: &[Owner], session: u64, generation: u64, timestamp: u64) -> u64 {
    let mut selected = 0;
    for row in owners {
        if !row.eligible
            || row.source == 0
            || row.session != session
            || row.generation != generation
            || row.ready == 0
            || row.simulation_begin == 0
            || row.ready > timestamp
            || row.simulation_begin > timestamp
        {
            continue;
        }
        if selected != 0 {
            return 0;
        }
        selected = row.source;
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamline_classifies_physical_input_and_registered_ping() {
        let ping = 0xc123;
        let input = streamline_input(RSF_GAME_INPUT_CONTROLLER, ping, ping).unwrap();
        assert_eq!(
            (input.controller, input.ping),
            (Some(CONTROLLER_SAMPLE), Some(PCL_PING))
        );
        assert!(!input.foreign_message && !input.empty);

        let foreign = streamline_input(RSF_GAME_INPUT_CONTROLLER, ping + 1, ping).unwrap();
        assert!(foreign.foreign_message);
        assert_eq!(
            (foreign.controller, foreign.ping),
            (Some(CONTROLLER_SAMPLE), None)
        );

        let empty = streamline_input(0, 0, 0).unwrap();
        assert!(empty.empty);
        assert_eq!((empty.controller, empty.ping), (None, None));
        assert!(streamline_input(8, 0, ping).is_none());
    }

    #[test]
    fn window_refuses_zero_and_repeats() {
        let mut window = Window::new();
        assert!(!window.accept(0));
        assert!(window.accept(5));
        assert!(!window.accept(5));
        assert!(!window.accept(0));
    }

    #[test]
    fn window_admits_bounded_reordering_once() {
        let mut window = Window::new();
        assert!(window.accept(70));
        assert!(window.accept(68));
        assert!(window.accept(69));
        assert!(!window.accept(68));
        assert!(!window.accept(69));
        // 63 behind is the oldest slot the window still tracks.
        assert!(window.accept(7));
        assert!(!window.accept(7));
        assert!(!window.accept(6));
    }

    #[test]
    fn window_forgets_ids_past_its_span() {
        let mut window = Window::new();
        assert!(window.accept(1));
        assert!(window.accept(2));
        assert!(window.accept(65));
        assert!(!window.accept(1));
        assert!(!window.accept(2), "already seen within the span");
        assert!(window.accept(500));
        assert!(!window.accept(65));
        assert!(!window.accept(436));
        assert!(window.accept(437));
    }

    #[test]
    fn window_keeps_history_across_short_advances() {
        let mut window = Window::new();
        assert!(window.accept(10));
        assert!(window.accept(12));
        assert!(window.accept(11));
        assert!(window.accept(20));
        assert!(!window.accept(10));
        assert!(!window.accept(11));
        assert!(!window.accept(12));
        assert!(window.accept(13));
    }

    #[test]
    fn window_accepts_the_largest_id() {
        let mut window = Window::new();
        assert!(window.accept(u64::MAX - 1));
        assert!(window.accept(u64::MAX));
        assert!(!window.accept(u64::MAX - 1));
        assert!(!window.accept(u64::MAX));
    }

    fn row(source: u64, ready: u64, simulation_begin: u64) -> Owner {
        Owner {
            source,
            session: 3,
            generation: 9,
            ready,
            simulation_begin,
            eligible: true,
        }
    }

    #[test]
    fn owner_selects_the_unique_matching_source() {
        let rows = [row(40, 500, 510), Owner::default()];
        assert_eq!(owner(&rows, 3, 9, 510), 40);
        assert_eq!(owner(&rows, 3, 9, 900), 40);
        assert_eq!(owner(&[], 3, 9, 900), 0);
    }

    #[test]
    fn owner_requires_session_generation_and_reached_timestamps() {
        let rows = [row(40, 500, 510)];
        assert_eq!(owner(&rows, 4, 9, 900), 0);
        assert_eq!(owner(&rows, 3, 8, 900), 0);
        assert_eq!(owner(&rows, 3, 9, 505), 0, "simulation has not begun");
        assert_eq!(
            owner(&[row(40, 600, 510)], 3, 9, 550),
            0,
            "pacing not ready"
        );
    }

    #[test]
    fn owner_skips_incomplete_rows() {
        let mut ineligible = row(41, 500, 510);
        ineligible.eligible = false;
        let rows = [
            ineligible,
            row(0, 500, 510),
            row(42, 0, 510),
            row(43, 500, 0),
            row(44, 500, 510),
        ];
        assert_eq!(owner(&rows, 3, 9, 900), 44);
    }

    #[test]
    fn owner_refuses_ambiguity() {
        let rows = [row(40, 500, 510), row(41, 520, 530)];
        assert_eq!(owner(&rows, 3, 9, 900), 0);
        assert_eq!(
            owner(&rows, 3, 9, 515),
            40,
            "the later row has not reached its timestamps"
        );
    }
}
