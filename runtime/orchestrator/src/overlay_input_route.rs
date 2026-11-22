// SPDX-License-Identifier: MIT
use crate::overlay_abi::rsf_overlay_input_counters as Counters;
use crate::overlay_cursor_hooks::rsf_core_overlay_cursor_ready;
use crate::overlay_pointer;
use crate::overlay_visibility::{
    self, Apply, rsf_core_overlay_toggle_key, rsf_core_overlay_visibility_set,
    rsf_core_overlay_visible,
};
use crate::overlay_window::{clear_holds, hold, release, rsf_core_overlay_input_window};
use crate::sr_session::has_capacity;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageExtraInfo, PT_PEN, PT_TOUCH, WHEEL_DELTA, WM_ACTIVATEAPP, WM_CHAR, WM_DEADCHAR,
    WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE, WM_RBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TOUCH, WM_XBUTTONDBLCLK,
    WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON2,
};

const DOWN: u32 = 1;
const SYSTEM: u32 = 2;
const REPEAT: u32 = 4;

static POINTER_RECEIVED: AtomicU64 = AtomicU64::new(0);
static POINTER_CONSUMED: AtomicU64 = AtomicU64::new(0);
static BUTTON_DOWNS: AtomicU64 = AtomicU64::new(0);
static WHEEL: AtomicU64 = AtomicU64::new(0);
static RAW: AtomicU64 = AtomicU64::new(0);
static KEYS: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static FRAMES_WITH_BUTTON: AtomicU64 = AtomicU64::new(0);
static LAST_POSITION: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Contact {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) id: u32,
    pub(super) flags: u32,
    pub(super) kind: i32,
}

#[repr(C)]
pub(super) struct MessageOps {
    pub(super) pointer: unsafe extern "C" fn(u32, &mut Contact) -> u32,
    pub(super) touch: unsafe extern "C" fn(isize, u32, *mut Contact, &mut u64) -> u32,
    pub(super) close_touch: unsafe extern "C" fn(isize),
    pub(super) contact: unsafe extern "C" fn(*mut c_void, &Contact, u32),
    pub(super) mouse_position: unsafe extern "C" fn(*mut c_void, isize),
    pub(super) mouse_button: unsafe extern "C" fn(u32, u32),
    pub(super) raw_mouse: unsafe extern "C" fn(*mut c_void, isize) -> bool,
    pub(super) apply: Apply,
}

const _: () = {
    assert!(size_of::<Contact>() == 20);
    assert!(std::mem::offset_of!(Contact, kind) == 16);
    assert!(size_of::<MessageOps>() == 8 * size_of::<usize>());
};

#[repr(u32)]
#[derive(Clone, Copy)]
pub(super) enum Disposition {
    Forward = 0,
    Consume = 1,
    ConsumeXButton = 2,
    Default = 3,
}

impl Contact {
    fn gesture(self, flags: u32, now: u64) -> i32 {
        overlay_pointer::gesture(
            self.id,
            self.x,
            self.y,
            self.flags | flags,
            now,
            rsf_core_overlay_visible(),
        )
    }
}

fn contact_flags(message: u32) -> u32 {
    match message {
        WM_POINTERDOWN | WM_LBUTTONDOWN => overlay_pointer::DOWN,
        WM_POINTERUP | WM_LBUTTONUP => overlay_pointer::UP,
        _ => 0,
    }
}

fn apply_gesture(desired: i32, apply: Apply) {
    if desired >= 0 {
        rsf_core_overlay_visibility_set(desired as u32, unsafe { GetTickCount64() }, apply);
    }
}

fn route_pointer(
    ops: &MessageOps,
    window: *mut c_void,
    message: u32,
    id: u32,
    visible: bool,
) -> Disposition {
    let mut contact = Contact::default();
    if unsafe { (ops.pointer)(id, &mut contact) } == 0 {
        if visible {
            note_pointer(1, 0);
        }
        return Disposition::Forward;
    }
    contact.flags |= contact_flags(message);
    if contact.kind == PT_TOUCH {
        let desired = contact.gesture(overlay_pointer::POINTER, unsafe { GetTickCount64() });
        apply_gesture(desired, ops.apply);
    }
    let canceled = contact.flags & overlay_pointer::CANCEL != 0;
    if canceled {
        overlay_pointer::capture_lost(contact.id);
    }
    if visible {
        let supported = matches!(contact.kind, PT_TOUCH | PT_PEN);
        note_pointer(1, supported.into());
        if supported {
            if !canceled {
                unsafe { (ops.contact)(window, &contact, 1) };
            }
            return Disposition::Consume;
        }
    }
    Disposition::Forward
}

fn route_touch(
    ops: &MessageOps,
    window: *mut c_void,
    handle: isize,
    count: u32,
    visible: bool,
) -> Disposition {
    let mut contacts = [Contact::default(); 32];
    let mut now = 0;
    if count as usize > contacts.len()
        || unsafe { (ops.touch)(handle, count, contacts.as_mut_ptr(), &mut now) } == 0
    {
        return Disposition::Forward;
    }
    let contacts = &contacts[..count as usize];
    let mut desired = -1;
    for contact in contacts {
        let change = contact.gesture(0, now);
        if change >= 0 {
            desired = change;
        }
    }
    apply_gesture(desired, ops.apply);
    if !visible {
        return Disposition::Forward;
    }
    note_pointer(count, count);
    for contact in contacts {
        unsafe { (ops.contact)(window, contact, 1) };
    }
    unsafe { (ops.close_touch)(handle) };
    Disposition::Consume
}

fn mouse_button(message: u32, wparam: usize) -> Option<(u32, bool)> {
    let x_key = if (wparam >> 16) as u16 == XBUTTON2 {
        VK_XBUTTON2
    } else {
        VK_XBUTTON1
    };
    let (key, down) = match message {
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => (VK_LBUTTON, true),
        WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => (VK_RBUTTON, true),
        WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => (VK_MBUTTON, true),
        WM_XBUTTONDOWN | WM_XBUTTONDBLCLK => (x_key, true),
        WM_LBUTTONUP => (VK_LBUTTON, false),
        WM_RBUTTONUP => (VK_RBUTTON, false),
        WM_MBUTTONUP => (VK_MBUTTON, false),
        WM_XBUTTONUP => (x_key, false),
        _ => return None,
    };
    Some((key.into(), down))
}

pub(super) extern "C" fn rsf_core_overlay_route_message(
    window: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
    ops: &MessageOps,
) -> Disposition {
    // Use the visibility at entry for delivery, even if this message toggles the overlay.
    let visible = rsf_core_overlay_visible() != 0;
    let extra = unsafe { GetMessageExtraInfo() } as usize;
    if visible
        && extra & 0xffffff80 == 0xff515780
        && matches!(message, WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP)
    {
        let contact = Contact {
            x: i32::from(lparam as i16),
            y: i32::from((lparam >> 16) as i16),
            flags: contact_flags(message),
            ..Contact::default()
        };
        note_pointer(1, 1);
        unsafe { (ops.contact)(window, &contact, 0) };
        return Disposition::Consume;
    }
    match message {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
            return route_pointer(ops, window, message, u32::from(wparam as u16), visible);
        }
        WM_POINTERCAPTURECHANGED => {
            overlay_pointer::capture_lost(u32::from(wparam as u16));
            if visible {
                return Disposition::Consume;
            }
        }
        WM_TOUCH => return route_touch(ops, window, lparam, u32::from(wparam as u16), visible),
        WM_KILLFOCUS => focus_lost(),
        WM_ACTIVATEAPP if wparam == 0 => focus_lost(),
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let mut flags = 0;
            if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
                flags |= DOWN;
            }
            if matches!(message, WM_SYSKEYDOWN | WM_SYSKEYUP) {
                flags |= SYSTEM;
            }
            if lparam & 0x40000000 != 0 {
                flags |= REPEAT;
            }
            if route_key(wparam as u32, flags, visible, ops.apply) != 0 {
                return Disposition::Consume;
            }
        }
        WM_CHAR | WM_DEADCHAR | WM_MOUSEHWHEEL if visible => return Disposition::Consume,
        WM_MOUSEMOVE if visible => {
            note_pointer(1, 1);
            unsafe { (ops.mouse_position)(window, lparam) };
            return Disposition::Consume;
        }
        WM_MOUSEWHEEL if visible => {
            let notches = f32::from((wparam >> 16) as i16) / WHEEL_DELTA as f32;
            overlay_pointer::scroll(notches);
            note_pointer(1, 1);
            WHEEL.fetch_add(1, Ordering::Relaxed);
            return Disposition::Consume;
        }
        WM_INPUT if visible => {
            if unsafe { (ops.raw_mouse)(window, lparam) } {
                RAW.fetch_add(1, Ordering::Relaxed);
            }
            // DefWindowProc releases the consumed raw-input record.
            return Disposition::Default;
        }
        _ => {
            if let Some((key, down)) = mouse_button(message, wparam) {
                let consumed = route_button(key, down, visible);
                if visible {
                    unsafe {
                        (ops.mouse_position)(window, lparam);
                        (ops.mouse_button)(key, down.into());
                    }
                }
                if consumed {
                    return if matches!(message, WM_XBUTTONDOWN | WM_XBUTTONDBLCLK | WM_XBUTTONUP) {
                        Disposition::ConsumeXButton
                    } else {
                        Disposition::Consume
                    };
                }
            }
        }
    }
    Disposition::Forward
}

fn route_key(key: u32, flags: u32, visible: bool, apply: Apply) -> u32 {
    let system = flags & SYSTEM != 0;
    let toggle = key == rsf_core_overlay_toggle_key();
    if flags & DOWN != 0 {
        if toggle {
            // The hidden panel still passes its opening press through so
            // the game's matching release must be forwarded as well.
            let now = unsafe { GetTickCount64() };
            if overlay_visibility::message_press(flags & REPEAT != 0, now, apply) && !visible {
                hold(key);
            }
            return visible.into();
        }
        if !visible || system {
            hold(key);
            return 0;
        }
    } else {
        if toggle {
            overlay_visibility::message_release();
        }
        let owed = release(key);
        // Never swallow a system release or a release for a press the game saw.
        if !visible || system || owed {
            return 0;
        }
    }
    KEYS.fetch_add(1, Ordering::Relaxed);
    1
}

fn route_button(key: u32, down: bool, visible: bool) -> bool {
    if down {
        if !visible {
            hold(key);
            return false;
        }
        BUTTON_DOWNS.fetch_add(1, Ordering::Relaxed);
        note_pointer(1, 1);
        return true;
    }
    let consumed = !release(key) && visible;
    if visible {
        note_pointer(1, consumed.into());
    }
    consumed
}

fn focus_lost() {
    clear_holds();
    overlay_pointer::focus_lost();
    overlay_visibility::focus_lost();
}

fn note_pointer(received: u32, consumed: u32) {
    if received != 0 {
        POINTER_RECEIVED.fetch_add(received.into(), Ordering::Relaxed);
    }
    if consumed != 0 {
        POINTER_CONSUMED.fetch_add(consumed.into(), Ordering::Relaxed);
    }
}

pub(super) fn sampled(x: f32, y: f32, buttons: u32) {
    FRAMES.fetch_add(1, Ordering::Relaxed);
    if buttons != 0 {
        FRAMES_WITH_BUTTON.fetch_add(1, Ordering::Relaxed);
    }
    LAST_POSITION.store(
        (u64::from(x as u32) << 32) | u64::from(y as u32),
        Ordering::Relaxed,
    );
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_overlay_input_counters_get(out: *mut Counters) -> i32 {
    if !unsafe { has_capacity(out) } {
        return -1;
    }
    let position = LAST_POSITION.load(Ordering::Relaxed);
    let record = Counters {
        struct_size: unsafe { out.cast::<u32>().read() },
        installed: (!rsf_core_overlay_input_window().is_null()).into(),
        visible: rsf_core_overlay_visible(),
        cursor_freed: rsf_core_overlay_cursor_ready(),
        pointer_received: POINTER_RECEIVED.load(Ordering::Relaxed),
        pointer_consumed: POINTER_CONSUMED.load(Ordering::Relaxed),
        button_downs_consumed: BUTTON_DOWNS.load(Ordering::Relaxed),
        wheel_consumed: WHEEL.load(Ordering::Relaxed),
        raw_mouse_reports: RAW.load(Ordering::Relaxed),
        keys_consumed: KEYS.load(Ordering::Relaxed),
        frames_visible: FRAMES.load(Ordering::Relaxed),
        frames_with_button: FRAMES_WITH_BUTTON.load(Ordering::Relaxed),
        last_pointer_x: (position >> 32) as u32,
        last_pointer_y: position as u32,
    };
    unsafe { out.write(record) };
    0
}
