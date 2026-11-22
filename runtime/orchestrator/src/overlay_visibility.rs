// SPDX-License-Identifier: MIT
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

const INSERT: u32 = 0x2d;
const DEBOUNCE_MS: u64 = 250;
const EXTERNAL: u32 = 0;
const WINDOW: u32 = 1;
const POLL: u32 = 2;

pub(super) type Apply = unsafe extern "C" fn(u32, u32);

#[derive(Clone, Copy)]
struct Change {
    visible: bool,
    source: u32,
}

struct Visibility {
    requested: bool,
    last_toggle: Option<u64>,
    key_down: bool,
    poll_focused: bool,
    applying: bool,
    pending: Option<Change>,
}

impl Visibility {
    const fn new(down: bool) -> Self {
        Self {
            requested: false,
            last_toggle: None,
            key_down: down,
            poll_focused: false,
            applying: false,
            pending: None,
        }
    }

    fn request(&mut self, visible: bool, source: u32, now: u64) -> Option<Change> {
        if self.requested == visible
            || (source != EXTERNAL
                && self
                    .last_toggle
                    .is_some_and(|previous| now.saturating_sub(previous) < DEBOUNCE_MS))
        {
            return None;
        }
        self.requested = visible;
        if source != EXTERNAL {
            self.last_toggle = Some(now);
        }
        let change = Change { visible, source };
        if self.applying {
            self.pending = Some(change);
            None
        } else {
            self.applying = true;
            Some(change)
        }
    }

    fn press(&mut self, repeat: bool, now: u64) -> (bool, Option<Change>) {
        let already_down = std::mem::replace(&mut self.key_down, true);
        if repeat || already_down {
            return (false, None);
        }
        (true, self.request(!self.requested, WINDOW, now))
    }

    fn poll(&mut self, focused: bool, down: bool, now: u64) -> Option<Change> {
        let was_focused = std::mem::replace(&mut self.poll_focused, focused);
        let was_down = std::mem::replace(&mut self.key_down, down);
        if focused && was_focused && down && !was_down {
            self.request(!self.requested, POLL, now)
        } else {
            None
        }
    }

    fn next(&mut self) -> Option<Change> {
        let next = self.pending.take();
        self.applying = next.is_some();
        next
    }

    fn focus_lost(&mut self) {
        self.key_down = false;
        self.poll_focused = false;
    }
}

static STATE: Mutex<Visibility> = Mutex::new(Visibility::new(false));
static VISIBLE: AtomicBool = AtomicBool::new(false);
static TOGGLE_KEY: AtomicU32 = AtomicU32::new(INSERT);

fn lock() -> MutexGuard<'static, Visibility> {
    crate::sync::lock(&STATE)
}

fn apply_changes(mut change: Option<Change>, apply: Apply) {
    while let Some(next) = change {
        if VISIBLE.swap(next.visible, Ordering::AcqRel) != next.visible {
            // User32 or the log sink can re-enter. New requests only queue
            // while this caller is applying the cursor change.
            unsafe { apply(u32::from(next.visible), next.source) };
        }
        change = lock().next();
    }
}

pub(super) extern "C" fn rsf_core_overlay_visible() -> u32 {
    VISIBLE.load(Ordering::Acquire).into()
}

pub(super) extern "C" fn rsf_core_overlay_toggle_key() -> u32 {
    TOGGLE_KEY.load(Ordering::Relaxed)
}

pub(super) fn reset(toggle_key: u32, down: bool) -> bool {
    let mut state = lock();
    *state = Visibility {
        applying: state.applying,
        ..Visibility::new(down)
    };
    TOGGLE_KEY.store(toggle_key, Ordering::Relaxed);
    VISIBLE.swap(false, Ordering::AcqRel)
}

pub(super) extern "C" fn rsf_core_overlay_visibility_set(visible: u32, now: u64, apply: Apply) {
    let change = lock().request(visible != 0, EXTERNAL, now);
    apply_changes(change, apply);
}

pub(super) fn message_press(repeat: bool, now: u64, apply: Apply) -> bool {
    let (edge, change) = lock().press(repeat, now);
    apply_changes(change, apply);
    edge
}

pub(super) fn message_release() {
    lock().key_down = false;
}

pub(super) fn focus_lost() {
    lock().focus_lost();
}

pub(super) extern "C" fn rsf_core_overlay_toggle_poll(
    focused: u32,
    down: u32,
    now: u64,
    apply: Apply,
) {
    let change = lock().poll(focused != 0, down != 0, now);
    apply_changes(change, apply);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_routes_do_not_repeat_an_admitted_change() {
        let mut state = Visibility::new(false);
        assert!(state.poll(true, false, 0).is_none());
        let (_, open) = state.press(false, 0);
        assert!(open.unwrap().visible);
        assert!(state.poll(true, true, 1).is_none());
        assert!(state.request(false, EXTERNAL, 2).is_none());
        assert!(!state.requested);
        assert!(!state.next().unwrap().visible);
        assert!(state.next().is_none());
    }

    #[test]
    fn focus_gain_with_key_held_is_not_a_press() {
        let mut state = Visibility::new(false);
        assert!(state.poll(false, true, 0).is_none());
        assert!(state.poll(true, true, 1).is_none());
        assert!(state.poll(true, false, 2).is_none());
        assert!(state.poll(true, true, 3).unwrap().visible);
    }

    #[test]
    fn focus_loss_releases_the_message_latch() {
        let mut state = Visibility::new(false);
        assert!(state.press(false, 0).1.unwrap().visible);
        assert!(state.next().is_none());
        state.focus_lost();
        let (edge, change) = state.press(false, 300);
        assert!(edge);
        assert!(!change.unwrap().visible);
    }
}
