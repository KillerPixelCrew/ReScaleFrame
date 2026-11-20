// SPDX-License-Identifier: MIT
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::MaybeUninit;

#[repr(C)]
#[derive(Default)]
struct Window {
    context: *mut c_void,
    position_rtv: *mut c_void,
    position_surface: *mut c_void,
    generation: u64,
    frame: u32,
    view: u8,
    open: bool,
    have_frame: bool,
    target_ok: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Draw {
    context: *mut c_void,
    vs: *mut c_void,
    ps: *mut c_void,
    hs: *mut c_void,
    ds: *mut c_void,
    kind: i32,
    tessellated: bool,
}

const _: () = {
    assert!(size_of::<Window>() == 40);
    assert!(std::mem::offset_of!(Window, target_ok) == 39);
    assert!(size_of::<Draw>() == 48);
    assert!(std::mem::offset_of!(Draw, kind) == 40);
};

thread_local! {
    static PENDING: Cell<Option<Draw>> = const { Cell::new(None) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_window_begin(
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

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_window_open(window: &mut Window, target_ok: bool) {
    window.target_ok = target_ok;
    window.open = true;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_window_close(window: &mut Window) -> bool {
    std::mem::take(&mut window.open)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_window_reset(window: &mut Window) {
    *window = Window::default();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_draw_arm(draw: &Draw) {
    PENDING.set(Some(*draw));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_draw_take(
    context: *mut c_void,
    out: &mut MaybeUninit<Draw>,
) -> bool {
    PENDING.with(|pending| {
        if let Some(draw) = pending.get()
            && (context.is_null() || context == draw.context)
        {
            pending.set(None);
            out.write(draw);
            return true;
        }
        false
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
            rsf_anomaly_motion_window_begin(window, context, context, context, 7, view, generation)
        };
        assert!(begin(&mut window, 0, 0));
        rsf_anomaly_motion_window_open(&mut window, true);
        assert!(rsf_anomaly_motion_window_close(&mut window));
        assert!(window.target_ok);
        assert!(!begin(&mut window, 0, 0));
        assert!(window.target_ok);
        assert!(begin(&mut window, 0, 1));
        assert!(!window.target_ok);
        rsf_anomaly_motion_window_open(&mut window, true);
        assert!(begin(&mut window, 1, 1));
        assert!(!window.target_ok);
        rsf_anomaly_motion_window_reset(&mut window);
        assert!(!window.have_frame);
    }

    #[test]
    fn foreign_context_cannot_consume_a_pending_draw_and_close_can_cancel_it() {
        let context = ptr::dangling_mut::<u8>().cast();
        let other = ptr::dangling_mut::<u64>().cast();
        let draw = Draw {
            context,
            vs: context,
            ps: context,
            hs: other,
            ds: other,
            kind: 2,
            tessellated: true,
        };
        rsf_anomaly_motion_draw_arm(&draw);
        let mut taken = MaybeUninit::uninit();
        assert!(!rsf_anomaly_motion_draw_take(other, &mut taken));
        assert!(rsf_anomaly_motion_draw_take(ptr::null_mut(), &mut taken));
        assert!(unsafe { taken.assume_init() }.tessellated);
        assert!(!rsf_anomaly_motion_draw_take(context, &mut taken));
        rsf_anomaly_motion_draw_arm(&draw);
        assert!(rsf_anomaly_motion_draw_take(context, &mut taken));
        assert!(!rsf_anomaly_motion_draw_take(context, &mut taken));
    }
}
