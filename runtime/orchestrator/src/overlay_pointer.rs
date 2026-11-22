// SPDX-License-Identifier: MIT
use crate::overlay_cursor_hooks::rsf_core_overlay_cursor_ready;
use crate::overlay_input_route;
use crate::overlay_visibility::rsf_core_overlay_visible;
use crate::overlay_window::{INVALID, NOT_INSTALLED, OK, rsf_core_overlay_input_window};
use crate::sr_session::has_capacity;
use crate::sys::rsf_overlay_input as Input;
use std::ffi::c_void;
use std::mem;
use std::sync::{Mutex, MutexGuard, PoisonError};

const LEFT: u32 = 1;
pub(super) const DOWN: u32 = 1;
pub(super) const UP: u32 = 2;
pub(super) const CANCEL: u32 = 4;
pub(super) const POINTER: u32 = 8;
const TAP_MS: u64 = 500;
const TAP_SLOP: u32 = 48;
const TOUCH_LIMIT: usize = 10;
const OUTSIDE_PANEL: f32 = -100000.0;

#[repr(C)]
#[derive(Default)]
pub(super) struct ClientPoint {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}

const _: () = assert!(size_of::<ClientPoint>() == 16);

impl ClientPoint {
    fn has_extent(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

#[derive(Clone, Copy)]
struct Finger {
    id: u32,
    x: i32,
    y: i32,
}

struct Gesture {
    fingers: [Finger; TOUCH_LIMIT],
    count: usize,
    peak: usize,
    started: u64,
    moved: bool,
    open_at_start: bool,
}

impl Gesture {
    const fn new() -> Self {
        Self {
            fingers: [Finger { id: 0, x: 0, y: 0 }; TOUCH_LIMIT],
            count: 0,
            peak: 0,
            started: 0,
            moved: false,
            open_at_start: false,
        }
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.fingers[..self.count]
            .iter()
            .position(|finger| finger.id == id)
    }

    fn add(&mut self, finger: Finger, now: u64, visible: bool) {
        if self.count == 0 {
            self.peak = 0;
            self.moved = false;
            self.started = now;
            self.open_at_start = visible;
        }
        if self.count == TOUCH_LIMIT {
            self.moved = true;
            return;
        }
        self.fingers[self.count] = finger;
        self.count += 1;
        self.peak = self.peak.max(self.count);
    }

    fn remove(&mut self, index: usize) {
        self.count -= 1;
        self.fingers[index] = self.fingers[self.count];
    }
}

struct PointerState {
    position: [f32; 2],
    display: [f32; 2],
    previous_report: Option<[f32; 2]>,
    have_raw: bool,
    buttons: u32,
    polled_buttons: u32,
    scroll: f32,
    contact_id: u32,
    contact_position: bool,
    contact_down: bool,
    contact_press: bool,
    multi_touch: bool,
    gesture: Gesture,
    have_pointer_touch: bool,
}

impl PointerState {
    const fn new() -> Self {
        Self {
            position: [0.0; 2],
            display: [0.0; 2],
            previous_report: None,
            have_raw: false,
            buttons: 0,
            polled_buttons: 0,
            scroll: 0.0,
            contact_id: 0,
            contact_position: false,
            contact_down: false,
            contact_press: false,
            multi_touch: false,
            gesture: Gesture::new(),
            have_pointer_touch: false,
        }
    }

    fn clear_transient(&mut self) {
        self.buttons = 0;
        self.polled_buttons = 0;
        self.scroll = 0.0;
        self.contact_position = false;
        self.contact_down = false;
        self.contact_press = false;
    }

    fn focus_lost(&mut self) {
        self.clear_transient();
        self.gesture = Gesture::new();
        self.multi_touch = false;
        self.previous_report = None;
    }

    fn scale(&self, point: &ClientPoint) -> [f32; 2] {
        let client = [point.width, point.height];
        std::array::from_fn(|i| {
            if client[i] > 0 && self.display[i] > 0.0 {
                self.display[i] / client[i] as f32
            } else {
                1.0
            }
        })
    }

    fn position_at(&mut self, point: &ClientPoint) {
        let [sx, sy] = self.scale(point);
        self.position = [point.x as f32 * sx, point.y as f32 * sy];
    }

    fn bound(&mut self, point: &ClientPoint) {
        let client = [point.width, point.height];
        let bounds: [f32; 2] = std::array::from_fn(|i| {
            if self.display[i] > 0.0 {
                self.display[i]
            } else {
                client[i] as f32
            }
        });
        if bounds.iter().all(|bound| *bound > 0.0) {
            for (coord, bound) in self.position.iter_mut().zip(bounds) {
                *coord = (*coord).clamp(0.0, bound - 1.0);
            }
        }
    }

    fn raw(&mut self, point: &ClientPoint) {
        self.have_raw = true;
        self.position[0] += point.x as f32;
        self.position[1] += point.y as f32;
        self.bound(point);
    }

    fn movement(&mut self, point: &ClientPoint, cursor_freed: bool) {
        self.contact_position = false;
        if cursor_freed || self.have_raw {
            return;
        }
        let report = [point.x as f32, point.y as f32];
        if !point.has_extent() {
            self.position = report;
            return;
        }
        let previous = self.previous_report.replace(report);
        // Ignore the game's warp to the center, but use it as the origin of
        // the next relative report.
        if point.x == point.width / 2 && point.y == point.height / 2 {
            return;
        }
        if let Some(previous) = previous {
            let scale = self.scale(point);
            for i in 0..2 {
                self.position[i] += (report[i] - previous[i]) * scale[i];
            }
            self.bound(point);
        }
    }

    fn contact(&mut self, point: &ClientPoint, id: u32, flags: u32) {
        if !point.has_extent() || self.multi_touch || (self.contact_down && self.contact_id != id) {
            return;
        }
        self.contact_id = id;
        self.contact_position = true;
        self.position_at(point);
        if flags & DOWN != 0 {
            self.contact_down = true;
            self.contact_press = true;
        }
        if flags & UP != 0 {
            self.contact_down = false;
        }
    }

    fn capture_lost(&mut self, id: u32) {
        if self.contact_id == id && self.contact_down {
            self.contact_down = false;
            self.contact_press = false;
            self.contact_position = false;
        }
        if let Some(index) = self.gesture.index(id) {
            self.gesture.moved = true;
            self.gesture.remove(index);
            if self.gesture.count == 0 {
                self.multi_touch = false;
            }
        }
    }

    fn gesture(&mut self, finger: Finger, flags: u32, now: u64, visible: bool) -> Option<bool> {
        if flags & POINTER != 0 {
            self.have_pointer_touch = true;
        } else if self.have_pointer_touch {
            return None;
        }
        let Some(index) = self.gesture.index(finger.id) else {
            if flags & DOWN != 0 && flags & CANCEL == 0 {
                self.gesture.add(finger, now, visible);
                if self.gesture.count == 2 && self.gesture.open_at_start {
                    self.multi_touch = true;
                    self.contact_down = false;
                    self.contact_press = false;
                    self.position = [OUTSIDE_PANEL; 2];
                }
            }
            return None;
        };
        let origin = self.gesture.fingers[index];
        if flags & CANCEL != 0
            || origin.x.abs_diff(finger.x) > TAP_SLOP
            || origin.y.abs_diff(finger.y) > TAP_SLOP
        {
            self.gesture.moved = true;
        }
        if flags & (UP | CANCEL) == 0 {
            return None;
        }
        self.gesture.remove(index);
        if self.gesture.count != 0 {
            return None;
        }
        self.multi_touch = false;
        (self.gesture.peak == 3
            && !self.gesture.moved
            && now.saturating_sub(self.gesture.started) <= TAP_MS)
            .then_some(!self.gesture.open_at_start)
    }

    fn sample(&mut self, display: [f32; 2], cursor: Option<&ClientPoint>) -> ([f32; 2], u32, f32) {
        self.display = display;
        if !self.contact_position
            && !self.multi_touch
            && let Some(cursor) = cursor
        {
            self.position_at(cursor);
        }
        let contact = mem::take(&mut self.contact_press) || self.contact_down;
        let buttons = self.buttons | self.polled_buttons | if contact { LEFT } else { 0 };
        (self.position, buttons, mem::take(&mut self.scroll))
    }
}

static STATE: Mutex<PointerState> = Mutex::new(PointerState::new());

fn lock() -> MutexGuard<'static, PointerState> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn reset() {
    *lock() = PointerState::new();
}

pub(super) extern "C" fn rsf_core_overlay_pointer_clear() {
    lock().clear_transient();
}

pub(super) fn focus_lost() {
    lock().focus_lost();
}

pub(super) extern "C" fn rsf_core_overlay_pointer_raw(point: &ClientPoint) {
    lock().raw(point);
}

pub(super) extern "C" fn rsf_core_overlay_pointer_move(point: &ClientPoint, cursor_freed: u32) {
    lock().movement(point, cursor_freed != 0);
}

pub(super) extern "C" fn rsf_core_overlay_pointer_seed(point: &ClientPoint) {
    let mut state = lock();
    state.position_at(point);
    state.previous_report = None;
}

pub(super) extern "C" fn rsf_core_overlay_pointer_button(bit: u32, down: u32) {
    let mut state = lock();
    if down != 0 {
        state.buttons |= bit;
    } else {
        state.buttons &= !bit;
    }
}

pub(super) extern "C" fn rsf_core_overlay_pointer_polled_buttons(buttons: u32) {
    lock().polled_buttons = buttons;
}

pub(super) fn scroll(notches: f32) {
    lock().scroll += notches;
}

pub(super) extern "C" fn rsf_core_overlay_pointer_contact(
    point: &ClientPoint,
    id: u32,
    flags: u32,
) {
    lock().contact(point, id, flags);
}

pub(super) fn capture_lost(id: u32) {
    lock().capture_lost(id);
}

pub(super) fn gesture(id: u32, x: i32, y: i32, flags: u32, now: u64, visible: u32) -> i32 {
    lock()
        .gesture(Finger { id, x, y }, flags, now, visible != 0)
        .map_or(-1, i32::from)
}

type SampleCursor = unsafe extern "C" fn(*mut c_void, *mut ClientPoint) -> u32;

pub(super) unsafe extern "C" fn rsf_core_overlay_input_collect(
    out: *mut Input,
    display_width: u32,
    display_height: u32,
    delta_seconds: f32,
    sample_cursor: SampleCursor,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    let mut input = Input {
        struct_size: unsafe { out.cast::<u32>().read() },
        display_width,
        display_height,
        delta_seconds: if delta_seconds.is_finite() && delta_seconds > 0.0 {
            delta_seconds
        } else {
            0.0
        },
        ..Input::default()
    };
    let window = rsf_core_overlay_input_window();
    if window.is_null() {
        unsafe { out.write(input) };
        return NOT_INSTALLED;
    }
    input.visible = rsf_core_overlay_visible();
    // Sample outside the accumulator lock. A newer touch or raw input sample
    // can still take priority when the pointer state is read.
    let mut cursor = ClientPoint::default();
    let sampled = input.visible != 0
        && rsf_core_overlay_cursor_ready() != 0
        && unsafe { sample_cursor(window, &mut cursor) } != 0
        && cursor.has_extent();
    let display = [display_width as f32, display_height as f32];
    let ([x, y], buttons, wheel) = lock().sample(display, sampled.then_some(&cursor));
    if input.visible != 0 {
        overlay_input_route::sampled(x, y, buttons);
    }
    input.mouse_x = x;
    input.mouse_y = y;
    input.mouse_buttons = buttons;
    input.scroll_delta = wheel;
    unsafe { out.write(input) };
    OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::from_mut;

    unsafe extern "C" fn no_cursor(_: *mut c_void, _: *mut ClientPoint) -> u32 {
        0
    }

    #[test]
    fn input_collection_checks_capacity_and_returns_idle_for_an_uninstalled_window() {
        let mut short = size_of::<u32>() as u32;
        let short_ptr = from_mut(&mut short).cast();
        assert_eq!(
            unsafe { rsf_core_overlay_input_collect(short_ptr, 800, 600, 0.0, no_cursor) },
            INVALID
        );
        assert_eq!(short, size_of::<u32>() as u32);

        let mut input = Input {
            struct_size: size_of::<Input>() as u32,
            visible: 1,
            mouse_buttons: LEFT,
            ..Input::default()
        };
        assert_eq!(
            unsafe {
                rsf_core_overlay_input_collect(&mut input, 800, 600, f32::INFINITY, no_cursor)
            },
            NOT_INSTALLED
        );
        assert_eq!(input.struct_size, size_of::<Input>() as u32);
        assert_eq!([input.display_width, input.display_height], [800, 600]);
        assert_eq!([input.visible, input.mouse_buttons], [0, 0]);
        assert_eq!(input.delta_seconds, 0.0);
    }

    #[test]
    fn capture_loss_discards_a_queued_press() {
        let mut state = PointerState::new();
        let point = ClientPoint {
            x: 30,
            y: 50,
            width: 100,
            height: 100,
        };
        state.contact(&point, 4, DOWN);
        state.capture_lost(4);
        assert_eq!(state.sample([100.0; 2], None).1, 0);
        state.contact(&point, 5, DOWN);
        state.contact(&point, 5, UP);
        state.capture_lost(5);
        assert_eq!(state.sample([100.0; 2], None).1, LEFT);
        assert_eq!(state.sample([100.0; 2], None).1, 0);
    }

    #[test]
    fn losing_focus_drops_an_incomplete_gesture() {
        let mut state = PointerState::new();
        for id in 1..4 {
            assert_eq!(
                state.gesture(Finger { id, x: 0, y: 0 }, POINTER | DOWN, 0, true),
                None
            );
        }
        assert!(state.multi_touch);
        state.focus_lost();
        for id in 1..4 {
            assert_eq!(
                state.gesture(Finger { id, x: 0, y: 0 }, POINTER | UP, 1, true),
                None
            );
        }
        assert!(!state.multi_touch);
    }
}
