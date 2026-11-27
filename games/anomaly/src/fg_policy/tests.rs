// SPDX-License-Identifier: MIT
use super::*;

#[derive(Clone, Copy)]
struct Facts {
    level: bool,
    scope: bool,
    presents: bool,
    inputs: bool,
    cut: bool,
    extent: [u32; 2],
}

const GAME: Facts = Facts {
    level: true,
    scope: false,
    presents: true,
    inputs: true,
    cut: false,
    extent: [1280, 720],
};
const MENU: Facts = Facts {
    level: false,
    ..GAME
};
const SCOPE: Facts = Facts {
    scope: true,
    presents: false,
    ..GAME
};
const NO_PRESENT: Facts = Facts {
    presents: false,
    ..GAME
};
const NO_INPUTS: Facts = Facts {
    inputs: false,
    ..GAME
};

struct Run {
    policy: Policy,
    frame: u32,
}

impl Run {
    fn new() -> Self {
        Self {
            policy: Policy::new(4),
            frame: 100,
        }
    }

    fn step(&mut self, facts: Facts) -> (Decision, Decision) {
        self.frame += 1;
        let begin = self.policy.begin(self.frame, facts.extent, facts.cut);
        if facts.scope {
            self.policy.scope();
        }
        if facts.level && self.policy.ui_boundary().tag != 0 {
            self.policy.inputs(facts.inputs);
        }
        if facts.presents {
            self.policy.presented();
        }
        (begin, self.policy.end())
    }
}

#[test]
fn gameplay_admission_and_completed_tags_are_distinct() {
    let mut run = Run::new();
    let (begin, first) = run.step(GAME);
    assert_eq!(begin.tag, 1);
    assert_eq!(first.screen, GAMEPLAY);
    assert_eq!(first.skip, RSF_ANOMALY_SKIP_NONE);
    assert_eq!(first.reset, RSF_ANOMALY_RESET_FIRST);
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_NONE);
    assert_eq!(run.step(NO_INPUTS).1.skip, RSF_ANOMALY_SKIP_NO_INPUTS);
    let counts = run.policy.counters();
    assert_eq!(counts.eligible_at_ui_boundary, 3);
    assert_eq!(counts.fed, 2);
    assert_eq!(counts.presented, 3);
}

#[test]
fn menu_loading_and_missing_present_or_inputs_break_history() {
    let mut run = Run::new();
    run.step(GAME);
    for _ in 0..4 {
        assert_eq!(run.step(MENU).1.skip, RSF_ANOMALY_SKIP_NO_LEVEL);
    }
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_DISCONTINUITY);
    assert_eq!(run.step(NO_PRESENT).1.skip, RSF_ANOMALY_SKIP_NO_PRESENT);
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_DISCONTINUITY);
    assert_eq!(run.step(NO_INPUTS).1.skip, RSF_ANOMALY_SKIP_NO_INPUTS);
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_DISCONTINUITY);
}

#[test]
fn scope_holds_history_for_four_engine_frames() {
    let mut run = Run::new();
    run.step(GAME);
    for _ in 0..5 {
        assert_eq!(run.step(SCOPE).1.skip, RSF_ANOMALY_SKIP_SCOPE);
        let (begin, end) = run.step(GAME);
        assert_eq!(begin.screen, HELD);
        assert_eq!(end.skip, RSF_ANOMALY_SKIP_SCOPE);
    }
    for _ in 0..3 {
        assert_eq!(run.step(GAME).1.skip, RSF_ANOMALY_SKIP_SCOPE);
    }
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_SCOPE_END);
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_NONE);
    let counts = run.policy.counters();
    assert_eq!((counts.scope_frames, counts.unpresented), (5, 5));
}

#[test]
fn reset_survives_a_refused_frame_and_size_and_cut_restart_history() {
    let mut run = Run::new();
    run.step(GAME);
    run.policy.device_reset();
    assert_eq!(run.step(GAME).1.reset, RSF_ANOMALY_RESET_DEVICE);
    let bigger = Facts {
        extent: [1600, 900],
        ..GAME
    };
    assert_eq!(run.step(bigger).1.reset, RSF_ANOMALY_RESET_SIZE);
    assert_eq!(run.step(bigger).1.reset, RSF_ANOMALY_RESET_NONE);
    assert_eq!(
        run.step(Facts {
            cut: true,
            ..bigger
        })
        .1
        .reset,
        RSF_ANOMALY_RESET_CUT
    );
    run.policy.device_reset();
    assert_eq!(
        run.step(Facts {
            inputs: false,
            ..bigger
        })
        .1
        .skip,
        RSF_ANOMALY_SKIP_NO_INPUTS
    );
    assert_eq!(run.step(bigger).1.reset, RSF_ANOMALY_RESET_DEVICE);
}

#[test]
fn each_frame_has_one_skip_class_and_each_fed_frame_has_one_reset_class() {
    let mut run = Run::new();
    for _ in 0..20 {
        for facts in [
            GAME, GAME, MENU, GAME, SCOPE, GAME, GAME, GAME, GAME, GAME, NO_PRESENT, GAME,
        ] {
            run.step(facts);
        }
    }
    let counts = run.policy.counters();
    assert_eq!(counts.skipped.iter().sum::<u64>(), counts.source_frames);
    assert_eq!(counts.skipped[RSF_ANOMALY_SKIP_NONE as usize], counts.fed);
    assert_eq!(counts.presented + counts.unpresented, counts.source_frames);
    assert_eq!(counts.resets.iter().sum::<u64>(), counts.fed);
}
