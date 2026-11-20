// SPDX-License-Identifier: MIT
#[cfg(windows)]
use super::stage::abi::ShaderObjects;
#[cfg(windows)]
use super::stage::shaders::Chain;
#[cfg(windows)]
use std::cell::Cell;
use std::ffi::c_void;

#[derive(Default)]
pub(super) struct Window {
    pub(super) context: *mut c_void,
    pub(super) position_rtv: *mut c_void,
    pub(super) position_surface: *mut c_void,
    pub(super) generation: u64,
    pub(super) frame: u32,
    pub(super) view: u8,
    pub(super) open: bool,
    pub(super) have_frame: bool,
    pub(super) target_ok: bool,
}

#[cfg(windows)]
pub(super) enum Action {
    Replay,
    Restore {
        stock: ShaderObjects,
        // Pins the stock and replacement shaders until restoration, including
        // when a reload publishes a new registry while this draw is pending.
        chain: Chain,
    },
}

#[cfg(windows)]
pub(super) struct Draw {
    pub context: *mut c_void,
    pub action: Action,
}

#[cfg(windows)]
thread_local! {
    static PENDING: Cell<Option<Draw>> = const { Cell::new(None) };
}

pub(super) fn begin(
    window: &mut Window,
    context: *mut c_void,
    rtv: *mut c_void,
    surface: *mut c_void,
    frame: u32,
    view: u8,
    generation: u64,
) -> bool {
    let fresh = !window.have_frame
        || window.frame != frame
        || window.view != view
        || window.generation != generation
        || window.context != context;
    window.context = context;
    window.position_rtv = rtv;
    window.position_surface = surface;
    window.frame = frame;
    window.view = view;
    window.generation = generation;
    window.have_frame = true;
    window.open = false;
    if fresh {
        window.target_ok = false;
    }
    fresh
}

pub(super) fn open(window: &mut Window, target_ok: bool) {
    window.target_ok = target_ok;
    window.open = true;
}

pub(super) fn close(window: &mut Window) -> bool {
    std::mem::take(&mut window.open)
}

pub(super) fn reset(window: &mut Window) {
    *window = Window::default();
}

#[cfg(windows)]
pub(super) fn arm(draw: Draw) {
    PENDING.set(Some(draw));
}

#[cfg(windows)]
pub(super) fn take(context: *mut c_void) -> Option<Draw> {
    PENDING.with(|pending| {
        let draw = pending.take()?;
        if context.is_null() || context == draw.context {
            Some(draw)
        } else {
            pending.set(Some(draw));
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn a_repeated_window_keeps_the_target_until_view_or_reset_changes() {
        let mut window = Window::default();
        let context = ptr::dangling_mut::<u8>().cast();
        let begin = |window: &mut Window, view, generation| {
            begin(window, context, context, context, 7, view, generation)
        };
        assert!(begin(&mut window, 0, 0));
        open(&mut window, true);
        assert!(close(&mut window));
        assert!(window.target_ok);
        assert!(!begin(&mut window, 0, 0));
        assert!(window.target_ok);
        assert!(begin(&mut window, 0, 1));
        assert!(!window.target_ok);
        open(&mut window, true);
        assert!(begin(&mut window, 1, 1));
        assert!(!window.target_ok);
        reset(&mut window);
        assert!(!window.have_frame);
    }

    #[cfg(windows)]
    #[test]
    fn foreign_context_cannot_consume_a_pending_draw_and_close_can_cancel_it() {
        let context = ptr::dangling_mut::<u8>().cast();
        let other = ptr::dangling_mut::<u64>().cast();
        let arm_replay = || {
            arm(Draw {
                context,
                action: Action::Replay,
            })
        };
        arm_replay();
        assert!(take(other).is_none());
        assert!(matches!(
            take(ptr::null_mut()).unwrap().action,
            Action::Replay
        ));
        assert!(take(context).is_none());
        arm_replay();
        assert!(take(context).is_some());
        assert!(take(context).is_none());
    }
}
