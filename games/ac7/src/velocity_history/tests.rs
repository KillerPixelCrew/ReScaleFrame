// SPDX-License-Identifier: MIT
use super::*;

fn model(frame: u32, records: u32) -> ModelDraw {
    ModelDraw {
        session_id: 1,
        history_key: 0,
        view_key: 2,
        primitive_key: 3,
        material_key: 4,
        vertex_factory_key: 5,
        vertex_hash: 6,
        geometry_hash: 0,
        start: 7,
        base_vertex: -8,
        particle: 0,
        frame,
        reset: 0,
        records,
    }
}

fn view(frame: u32, width: u32, height: u32) -> ViewDraw {
    ViewDraw {
        session_id: 1,
        history_key: 0,
        view_key: 2,
        family_key: 3,
        frame,
        width,
        height,
    }
}

fn end_frame(history: &mut History) -> Vec<Retired> {
    let mut retired = Vec::new();
    history.end_frame(&mut retired);
    retired.sort_unstable();
    retired
}

#[test]
fn history_is_usable_only_for_the_next_frame_without_reset() {
    let mut history = History::default();
    let (admitted, first) = history.begin_model(&model(10, 4));
    assert!(admitted);
    assert_eq!((first.slot, first.rotate, first.usable), (0, true, false));

    let (_, same) = history.begin_model(&model(10, 4));
    assert_eq!((same.slot, same.rotate, same.usable), (0, false, false));

    let (_, next) = history.begin_model(&model(11, 4));
    assert_eq!((next.rotate, next.usable), (true, true));
    // Validity persists through later draws of the same frame.
    let (_, again) = history.begin_model(&model(11, 4));
    assert_eq!((again.rotate, again.usable), (false, true));

    let (_, skipped) = history.begin_model(&model(13, 4));
    assert_eq!((skipped.rotate, skipped.usable), (true, false));

    let mut reset = model(14, 4);
    reset.reset = 1;
    assert!(!history.begin_model(&reset).1.usable);

    let (_, after_reset) = history.begin_model(&model(15, 4));
    assert!(after_reset.usable);
}

#[test]
fn frame_succession_wraps_but_never_follows_the_empty_frame() {
    let mut history = History::default();
    // A new model's frame is the sentinel, whose wrapped successor is frame 0.
    assert!(!history.begin_model(&model(0, 1)).1.usable);
    let mut other = model(NO_FRAME - 1, 1);
    other.primitive_key = 9;
    history.begin_model(&other);
    other.frame = NO_FRAME;
    assert!(history.begin_model(&other).1.usable);
    other.frame = 0;
    assert!(!history.begin_model(&other).1.usable);
}

#[test]
fn capacity_doubles_from_1024_over_twice_the_frame_records() {
    let mut history = History::default();
    assert_eq!(history.begin_model(&model(1, 512)).1.capacity, 1024);
    // A failed preparation leaves the full minimum capacity available.
    assert_eq!(history.begin_model(&model(1, 512)).1.capacity, 1024);
    history.record_model(0, 1, 512);
    assert_eq!(history.begin_model(&model(1, 1)).1.capacity, 2048);
    history.record_model(0, 1, 1);
    assert_eq!(history.begin_model(&model(1, 1000)).1.capacity, 4096);
    // A new frame restarts the count.
    assert_eq!(history.begin_model(&model(2, 1)).1.capacity, 1024);
}

#[test]
fn record_overflow_refuses_after_rotation_and_epoch() {
    let mut history = History::default();
    history.begin_model(&model(1, RECORD_LIMIT));
    let (admitted, decision) = history.begin_model(&model(2, RECORD_LIMIT + 1));
    assert!(!admitted);
    assert_eq!(
        (decision.slot, decision.rotate, decision.usable),
        (0, true, true)
    );
    let entry = history.models.get_mut(0).unwrap();
    assert_eq!(
        (entry.frame, entry.records, entry.epoch),
        (2, 0, FIRST_EPOCH)
    );

    assert!(history.begin_model(&model(3, RECORD_LIMIT)).0);
    history.record_model(0, 3, RECORD_LIMIT);
    assert!(!history.begin_model(&model(3, 1)).0);
    assert!(!history.begin_model(&model(3, u32::MAX)).0);
}

#[test]
fn particles_drop_the_factory_and_draw_offsets() {
    let mut history = History::default();
    let mut a = model(1, 1);
    a.particle = 1;
    let mut b = a;
    b.vertex_factory_key = 50;
    b.start = 70;
    b.base_vertex = 80;
    assert_eq!(
        history.begin_model(&a).1.slot,
        history.begin_model(&b).1.slot
    );

    let mut mesh = model(1, 1);
    let first = history.begin_model(&mesh).1.slot;
    mesh.start += 1;
    assert_ne!(history.begin_model(&mesh).1.slot, first);
    mesh.geometry_hash = 2;
    let generated = history.begin_model(&mesh).1.slot;
    mesh.vertex_hash = 3;
    assert_ne!(history.begin_model(&mesh).1.slot, generated);
}

#[test]
fn history_key_replaces_the_view_key_for_identity() {
    let mut history = History::default();
    let mut a = model(1, 1);
    a.history_key = 40;
    let mut b = a;
    b.view_key = 41;
    assert_eq!(
        history.begin_model(&a).1.slot,
        history.begin_model(&b).1.slot
    );
    let c = model(1, 1);
    assert_ne!(
        history.begin_model(&c).1.slot,
        history.begin_model(&a).1.slot
    );

    let mut v = view(1, 8, 8);
    v.history_key = 40;
    let slot = history.begin_view(&v).slot;
    v.view_key = 41;
    assert_eq!(history.begin_view(&v).slot, slot);
}

#[test]
fn view_allocates_until_ready_and_on_extent_change() {
    let mut history = History::default();
    let first = history.begin_view(&view(1, 64, 32));
    assert_eq!(
        first,
        ViewDecision {
            slot: 0,
            action: ViewAction::Allocate,
            width: 64,
            height: 32
        }
    );
    // A failed allocation leaves the slot unready and retries.
    assert_eq!(
        history.begin_view(&view(2, 64, 32)).action,
        ViewAction::Allocate
    );
    history.ready_view(0, &view(2, 64, 32));
    assert_eq!(
        history.begin_view(&view(2, 64, 32)).action,
        ViewAction::SameFrame
    );
    assert_eq!(
        history.begin_view(&view(3, 64, 32)).action,
        ViewAction::Reuse
    );
    history.ready_view(0, &view(3, 64, 32));
    history.mark_written(0, true);

    let resized = history.begin_view(&view(4, 128, 32));
    assert_eq!((resized.action, resized.width), (ViewAction::Allocate, 128));
    let entry = history.views.get_mut(0).unwrap();
    assert_eq!(
        (entry.frame, entry.epoch, entry.written),
        (NO_FRAME, 0, false)
    );
}

#[test]
fn same_frame_keeps_the_view_epoch() {
    let mut history = History::default();
    history.begin_view(&view(1, 8, 8));
    history.ready_view(0, &view(1, 8, 8));
    end_frame(&mut history);
    assert_eq!(
        history.begin_view(&view(1, 8, 8)).action,
        ViewAction::SameFrame
    );
    assert_eq!(history.views.get_mut(0).unwrap().epoch, FIRST_EPOCH);
}

#[test]
fn commit_needs_written_motion_for_the_same_frame_view_and_family() {
    let mut history = History::default();
    history.begin_view(&view(5, 16, 8));
    history.ready_view(0, &view(5, 16, 8));
    assert_eq!(history.commit_view(&view(5, 0, 0)), UNAVAILABLE_VIEW);
    history.mark_written(0, true);
    assert_eq!(
        history.commit_view(&view(5, 0, 0)),
        ViewDecision {
            slot: 0,
            action: ViewAction::Commit,
            width: 16,
            height: 8
        }
    );
    assert_eq!(history.commit_view(&view(6, 0, 0)), UNAVAILABLE_VIEW);
    let mut family = view(5, 16, 8);
    family.family_key = 9;
    assert_eq!(history.begin_view(&family), UNAVAILABLE_VIEW);
    assert_eq!(history.commit_view(&family), UNAVAILABLE_VIEW);
    // The history key finds the entry, but the commit still needs the drawn view key.
    let mut keyed = view(7, 16, 8);
    keyed.history_key = 30;
    let slot = history.begin_view(&keyed).slot;
    history.ready_view(slot, &keyed);
    history.mark_written(slot, true);
    keyed.view_key = 31;
    assert_eq!(history.begin_view(&keyed), UNAVAILABLE_VIEW);
    assert_eq!(history.commit_view(&keyed), UNAVAILABLE_VIEW);

    history.mark_written(0, false);
    assert_eq!(history.commit_view(&view(5, 0, 0)), UNAVAILABLE_VIEW);
    let mut other = view(5, 0, 0);
    other.session_id = 2;
    assert_eq!(history.commit_view(&other), UNAVAILABLE_VIEW);
}

#[test]
fn entries_retire_at_the_third_frame_end_after_use() {
    let mut history = History::default();
    history.begin_model(&model(1, 1));
    history.begin_view(&view(1, 8, 8));
    history.ready_view(0, &view(1, 8, 8));
    assert!(end_frame(&mut history).is_empty());
    assert!(end_frame(&mut history).is_empty());
    assert_eq!(
        end_frame(&mut history),
        [Retired::Model(0), Retired::View(0)]
    );
    assert_eq!((history.models.len(), history.views.len()), (0, 0));
    assert_eq!(history.epoch, FIRST_EPOCH + 3);
}

#[test]
fn use_refreshes_the_epoch() {
    let mut history = History::default();
    history.begin_model(&model(1, 1));
    end_frame(&mut history);
    end_frame(&mut history);
    history.begin_model(&model(3, 1));
    assert!(end_frame(&mut history).is_empty());
    assert!(end_frame(&mut history).is_empty());
    assert_eq!(end_frame(&mut history), [Retired::Model(0)]);
}

#[test]
fn a_view_left_unready_keeps_epoch_zero() {
    let mut history = History::default();
    history.begin_view(&view(1, 8, 8));
    assert!(end_frame(&mut history).is_empty());
    assert_eq!(end_frame(&mut history), [Retired::View(0)]);
}

#[test]
fn retired_slots_are_reused_as_fresh_entries() {
    let mut history = History::default();
    history.begin_model(&model(1, 1));
    let mut other = model(1, 1);
    other.material_key = 99;
    assert_eq!(history.begin_model(&other).1.slot, 1);
    end_frame(&mut history);
    end_frame(&mut history);
    history.begin_model(&other);
    assert_eq!(end_frame(&mut history), [Retired::Model(0)]);

    let mut third = model(3, 1);
    third.material_key = 7;
    let (_, decision) = history.begin_model(&third);
    assert_eq!(
        (decision.slot, decision.rotate, decision.usable),
        (0, true, false)
    );
    assert_eq!(history.models.len(), 2);
}

#[test]
fn reset_restarts_slots_and_the_epoch() {
    let mut history = History::default();
    history.begin_model(&model(1, 1));
    history.begin_view(&view(1, 8, 8));
    end_frame(&mut history);
    history = History::default();
    assert_eq!(history.epoch, FIRST_EPOCH);
    assert!(!history.begin_model(&model(2, 1)).1.usable);
    assert_eq!(
        history.begin_view(&view(2, 8, 8)).action,
        ViewAction::Allocate
    );
}
