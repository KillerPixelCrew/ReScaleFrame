// SPDX-License-Identifier: MIT
use crate::overlay_cursor_capture::{self as capture, Effects};
use crate::overlay_cursor_hooks::{self as hooks, Platform as HookPlatform};
use crate::overlay_host_abi::{rsf_overlay_input as Input, rsf_overlay_input_options as Options};
use crate::overlay_input_route::{
    Contact, Disposition, MessageOps, rsf_core_overlay_route_message,
};
use crate::overlay_pointer::{self as pointer, ClientPoint};
use crate::overlay_visibility::{
    rsf_core_overlay_toggle_key, rsf_core_overlay_toggle_poll, rsf_core_overlay_visibility_set,
    rsf_core_overlay_visible,
};
use crate::overlay_window::{self as window, Platform as WindowPlatform};
use std::ffi::{c_char, c_void};
use std::mem::{offset_of, transmute};
use std::ptr::{self, null, null_mut};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    System::SystemInformation::GetTickCount64,
    UI::{
        Input::{
            GetRawInputBuffer, GetRawInputData, HRAWINPUT,
            KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
            MOUSE_MOVE_ABSOLUTE,
            Pointer::{GetPointerInfo, POINTER_FLAG_CANCELED, POINTER_INFO},
            RAWINPUT, RAWINPUTHEADER, RAWMOUSE, RID_INPUT, RIM_TYPEMOUSE,
            Touch::{
                CloseTouchInputHandle, GetTouchInputInfo, TOUCHEVENTF_DOWN, TOUCHEVENTF_UP,
                TOUCHINPUT,
            },
        },
        WindowsAndMessaging::{
            CallWindowProcW, ClipCursor, DefWindowProcW, GA_ROOT, GetAncestor, GetClientRect,
            GetCursorPos, GetForegroundWindow, HCURSOR, IDC_ARROW, LoadCursorW, PT_TOUCH,
            SetCursor, SetCursorPos, ShowCursor, WNDPROC,
        },
    },
};

const RAW_MOUSE_BYTES: usize = offset_of!(RAWINPUT, data) + size_of::<RAWMOUSE>();
const SET_POS: u32 = 0;
const CLIP: u32 = 1;
const GET_POS: u32 = 2;
const SHOW: u32 = 3;
const SET: u32 = 4;
const RAW_DATA: u32 = 5;
const RAW_BUFFER: u32 = 6;

struct CursorCall;
impl CursorCall {
    fn enter() -> Self {
        hooks::rsf_core_overlay_cursor_enter();
        Self
    }
}
impl Drop for CursorCall {
    fn drop(&mut self) {
        hooks::rsf_core_overlay_cursor_leave();
    }
}

// The trampoline is published with the signature of its matching User32 entry.
macro_rules! cursor_call {
    ($index:ident, $entry:ident, $signature:ty, $($arg:expr),+) => {{
        let _call = CursorCall::enter();
        let address = hooks::rsf_core_overlay_cursor_forward($index);
        let entry: $signature = if address.is_null() {
            $entry
        } else {
            unsafe { transmute::<*mut c_void, $signature>(address) }
        };
        unsafe { entry($($arg),+) }
    }};
}

fn client_point(window: HWND, point: POINT) -> ClientPoint {
    let mut rect = RECT::default();
    let valid =
        unsafe { GetClientRect(window, &mut rect) } != 0 && rect.right > 0 && rect.bottom > 0;
    ClientPoint {
        x: point.x,
        y: point.y,
        width: if valid { rect.right } else { 0 },
        height: if valid { rect.bottom } else { 0 },
    }
}

fn button_bit(key: u32) -> u32 {
    if key == u32::from(VK_LBUTTON) {
        1
    } else if key == u32::from(VK_RBUTTON) {
        2
    } else if key == u32::from(VK_MBUTTON) {
        4
    } else {
        0
    }
}

unsafe extern "C" fn record_raw_mouse(window: HWND, lparam: LPARAM) -> bool {
    // A freed cursor is sampled in absolute coordinates at collection time.
    if hooks::rsf_core_overlay_cursor_ready() != 0 {
        return false;
    }
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    let read = unsafe {
        GetRawInputData(
            lparam as HRAWINPUT,
            RID_INPUT,
            ptr::from_mut(&mut raw).cast(),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || (read as usize) < RAW_MOUSE_BYTES || raw.header.dwType != RIM_TYPEMOUSE {
        return false;
    }
    let mouse = unsafe { raw.data.mouse };
    if mouse.usFlags & MOUSE_MOVE_ABSOLUTE != 0 || (mouse.lLastX == 0 && mouse.lLastY == 0) {
        return false;
    }
    pointer::rsf_core_overlay_pointer_raw(&client_point(
        window,
        POINT {
            x: mouse.lLastX,
            y: mouse.lLastY,
        },
    ));
    true
}

unsafe extern "C" fn record_mouse_position(window: HWND, lparam: LPARAM) {
    let p = POINT {
        x: i32::from(lparam as i16),
        y: i32::from((lparam >> 16) as i16),
    };
    let freed = hooks::rsf_core_overlay_cursor_ready();
    let facts = if freed != 0 {
        ClientPoint {
            x: p.x,
            y: p.y,
            ..ClientPoint::default()
        }
    } else {
        client_point(window, p)
    };
    pointer::rsf_core_overlay_pointer_move(&facts, freed);
}

unsafe extern "C" fn record_button(key: u32, down: u32) {
    let bit = button_bit(key);
    if bit != 0 {
        pointer::rsf_core_overlay_pointer_button(bit, down);
    }
}

unsafe extern "C" fn genuine_position(point: *mut POINT) -> i32 {
    cursor_call!(
        GET_POS,
        GetCursorPos,
        unsafe extern "system" fn(*mut POINT) -> i32,
        point
    )
}

unsafe extern "C" fn genuine_clip(rect: *const RECT) {
    cursor_call!(
        CLIP,
        ClipCursor,
        unsafe extern "system" fn(*const RECT) -> i32,
        rect
    );
}

unsafe extern "C" fn genuine_show(show: i32) -> i32 {
    cursor_call!(
        SHOW,
        ShowCursor,
        unsafe extern "system" fn(i32) -> i32,
        show
    )
}

fn genuine_set(cursor: HCURSOR) -> HCURSOR {
    cursor_call!(
        SET,
        SetCursor,
        unsafe extern "system" fn(HCURSOR) -> HCURSOR,
        cursor
    )
}

fn arrow() -> HCURSOR {
    unsafe { LoadCursorW(null_mut(), IDC_ARROW) }
}

unsafe extern "C" fn set_arrow() {
    genuine_set(arrow());
}

static EFFECTS: Effects = Effects {
    get: genuine_position,
    clip: genuine_clip,
    show: genuine_show,
    arrow: set_arrow,
};

unsafe extern "system" fn hooked_set_position(x: i32, y: i32) -> i32 {
    if capture::rsf_core_overlay_cursor_swallow_warp() != 0 {
        return 1;
    }
    cursor_call!(
        SET_POS,
        SetCursorPos,
        unsafe extern "system" fn(i32, i32) -> i32,
        x,
        y
    )
}

unsafe extern "system" fn hooked_clip(rect: *const RECT) -> i32 {
    let captured = unsafe { capture::rsf_core_overlay_cursor_clip_request(rect) } != 0;
    cursor_call!(
        CLIP,
        ClipCursor,
        unsafe extern "system" fn(*const RECT) -> i32,
        if captured { null() } else { rect }
    )
}

unsafe extern "system" fn hooked_get_position(point: *mut POINT) -> i32 {
    if unsafe { capture::rsf_core_overlay_cursor_frozen(point) } != 0 {
        1
    } else {
        unsafe { genuine_position(point) }
    }
}

unsafe extern "system" fn hooked_show(show: i32) -> i32 {
    capture::rsf_core_overlay_cursor_show(show, genuine_show)
}

unsafe extern "system" fn hooked_set(cursor: HCURSOR) -> HCURSOR {
    genuine_set(
        if cursor.is_null() && capture::rsf_core_overlay_cursor_captured() != 0 {
            arrow()
        } else {
            cursor
        },
    )
}

unsafe fn neutralize_raw_mouse(raw: *mut RAWINPUT, bytes: usize) {
    if bytes >= RAW_MOUSE_BYTES && unsafe { (*raw).header.dwType } == RIM_TYPEMOUSE {
        let mouse = unsafe { &mut (*raw).data.mouse };
        mouse.lLastX = 0;
        mouse.lLastY = 0;
        mouse.Anonymous.ulButtons = 0;
    }
}

unsafe extern "system" fn hooked_raw_data(
    handle: HRAWINPUT,
    command: u32,
    data: *mut c_void,
    size: *mut u32,
    header: u32,
) -> u32 {
    let result = cursor_call!(
        RAW_DATA,
        GetRawInputData,
        unsafe extern "system" fn(HRAWINPUT, u32, *mut c_void, *mut u32, u32) -> u32,
        handle,
        command,
        data,
        size,
        header
    );
    if result != u32::MAX
        && !data.is_null()
        && command == RID_INPUT
        && capture::rsf_core_overlay_cursor_captured() != 0
    {
        unsafe { neutralize_raw_mouse(data.cast(), result as usize) };
    }
    result
}

unsafe extern "system" fn hooked_raw_buffer(
    data: *mut RAWINPUT,
    size: *mut u32,
    header: u32,
) -> u32 {
    let capacity = if data.is_null() || size.is_null() {
        0
    } else {
        (unsafe { *size }) as usize
    };
    let result = cursor_call!(
        RAW_BUFFER,
        GetRawInputBuffer,
        unsafe extern "system" fn(*mut RAWINPUT, *mut u32, u32) -> u32,
        data,
        size,
        header
    );
    if result == u32::MAX || data.is_null() || capture::rsf_core_overlay_cursor_captured() == 0 {
        return result;
    }
    let mut offset = 0;
    for _ in 0..result {
        let remaining = capacity.saturating_sub(offset);
        if remaining < size_of::<RAWINPUTHEADER>() {
            break;
        }
        let record = unsafe { data.cast::<u8>().add(offset).cast::<RAWINPUT>() };
        let bytes = unsafe { (*record).header.dwSize } as usize;
        if !(size_of::<RAWINPUTHEADER>()..=remaining).contains(&bytes) {
            break;
        }
        unsafe { neutralize_raw_mouse(record, bytes) };
        // Same alignment as the SDK's NEXTRAWINPUTBLOCK macro.
        offset += bytes.next_multiple_of(size_of::<usize>());
    }
    result
}

#[cfg(not(test))]
mod minhook {
    use super::*;
    // Values from the pinned MinHook.h. All calls share the carrier's MinHook instance.
    const OK: i32 = 0;
    const ALREADY_INITIALIZED: i32 = 1;
    const NOT_CREATED: i32 = 4;
    const ENABLED: i32 = 5;
    const DISABLED: i32 = 6;

    unsafe extern "system" {
        fn MH_Initialize() -> i32;
        fn MH_CreateHook(
            target: *mut c_void,
            detour: *mut c_void,
            original: *mut *mut c_void,
        ) -> i32;
        fn MH_EnableHook(target: *mut c_void) -> i32;
        fn MH_DisableHook(target: *mut c_void) -> i32;
        fn MH_RemoveHook(target: *mut c_void) -> i32;
    }

    pub(super) static PLATFORM: HookPlatform = HookPlatform {
        initialize,
        create,
        enable,
        disable,
        remove,
    };

    unsafe extern "C" fn initialize() -> i32 {
        match unsafe { MH_Initialize() } {
            ALREADY_INITIALIZED => OK,
            status => status,
        }
    }

    unsafe extern "C" fn create(
        target: *mut c_void,
        detour: *mut c_void,
        original: *mut *mut c_void,
    ) -> i32 {
        unsafe { MH_CreateHook(target, detour, original) }
    }

    unsafe extern "C" fn enable(target: *mut c_void) -> i32 {
        match unsafe { MH_EnableHook(target) } {
            ENABLED => OK,
            status => status,
        }
    }

    unsafe extern "C" fn disable(target: *mut c_void) -> i32 {
        i32::from(matches!(
            unsafe { MH_DisableHook(target) },
            OK | DISABLED | NOT_CREATED
        ))
    }

    unsafe extern "C" fn remove(target: *mut c_void) -> i32 {
        i32::from(matches!(unsafe { MH_RemoveHook(target) }, OK | NOT_CREATED))
    }
}

#[cfg(test)]
mod minhook {
    use super::*;
    unsafe extern "C" fn unavailable() -> i32 {
        -1
    }
    unsafe extern "C" fn create(_: *mut c_void, _: *mut c_void, _: *mut *mut c_void) -> i32 {
        -1
    }
    unsafe extern "C" fn modify(_: *mut c_void) -> i32 {
        0
    }
    pub(super) static PLATFORM: HookPlatform = HookPlatform {
        initialize: unavailable,
        create,
        enable: modify,
        disable: modify,
        remove: modify,
    };
}

unsafe extern "C" fn install_cursor() {
    let detours = [
        hooked_set_position as *mut c_void,
        hooked_clip as *mut c_void,
        hooked_get_position as *mut c_void,
        hooked_show as *mut c_void,
        hooked_set as *mut c_void,
        hooked_raw_data as *mut c_void,
        hooked_raw_buffer as *mut c_void,
    ];
    hooks::rsf_core_overlay_cursor_install(&detours, &minhook::PLATFORM);
}

unsafe extern "C" fn retire_cursor(visible: u32, window_alive: u32) -> i32 {
    if visible != 0 {
        capture::rsf_core_overlay_cursor_capture(0, window_alive, &EFFECTS);
    }
    let retired = hooks::rsf_core_overlay_cursor_retire(&minhook::PLATFORM);
    capture::rsf_core_overlay_cursor_reset_capture();
    retired
}

unsafe extern "C" fn apply_visibility(visible: u32, route: u32) {
    pointer::rsf_core_overlay_pointer_clear();
    capture::rsf_core_overlay_cursor_capture(visible, 1, &EFFECTS);
    if visible != 0 {
        let mut point = ClientPoint::default();
        if unsafe { sample_cursor(window::rsf_core_overlay_input_window(), &mut point) } != 0 {
            pointer::rsf_core_overlay_pointer_seed(&point);
        }
    }
    let source = match route {
        1 => "window-message",
        2 => "foreground-poll",
        _ => "external",
    };
    let state = if visible != 0 { "visible" } else { "hidden" };
    window::diagnostic(&format!(
        "overlay input: toggle pressed, now {state} (route={source})"
    ));
}

unsafe extern "C" fn pointer_contact(id: u32, out: &mut Contact) -> u32 {
    let mut info = POINTER_INFO::default();
    if unsafe { GetPointerInfo(id, &mut info) } == 0 {
        return 0;
    }
    *out = Contact {
        x: info.ptPixelLocation.x,
        y: info.ptPixelLocation.y,
        id: info.pointerId,
        flags: if info.pointerFlags & POINTER_FLAG_CANCELED != 0 {
            pointer::CANCEL
        } else {
            0
        },
        kind: info.pointerType,
    };
    1
}

unsafe extern "C" fn touch_contacts(
    handle: isize,
    count: u32,
    out: *mut Contact,
    now: &mut u64,
) -> u32 {
    let mut contacts = [TOUCHINPUT::default(); 32];
    if count as usize > contacts.len()
        || unsafe {
            GetTouchInputInfo(
                handle as _,
                count,
                contacts.as_mut_ptr(),
                size_of::<TOUCHINPUT>() as i32,
            )
        } == 0
    {
        return 0;
    }
    *now = unsafe { GetTickCount64() };
    for (i, contact) in contacts[..count as usize].iter().enumerate() {
        let flags = (if contact.dwFlags & TOUCHEVENTF_DOWN != 0 {
            pointer::DOWN
        } else {
            0
        }) | (if contact.dwFlags & TOUCHEVENTF_UP != 0 {
            pointer::UP
        } else {
            0
        });
        unsafe {
            out.add(i).write(Contact {
                x: contact.x / 100,
                y: contact.y / 100,
                id: contact.dwID,
                flags,
                kind: PT_TOUCH,
            })
        };
    }
    1
}

unsafe extern "C" fn close_touch(handle: isize) {
    unsafe { CloseTouchInputHandle(handle as _) };
}

unsafe extern "C" fn deliver_contact(window: HWND, contact: &Contact, screen: u32) {
    let mut point = POINT {
        x: contact.x,
        y: contact.y,
    };
    if screen != 0 && unsafe { ScreenToClient(window, &mut point) } == 0 {
        return;
    }
    pointer::rsf_core_overlay_pointer_contact(
        &client_point(window, point),
        contact.id,
        contact.flags,
    );
}

static MESSAGE_OPS: MessageOps = MessageOps {
    pointer: pointer_contact,
    touch: touch_contacts,
    close_touch,
    contact: deliver_contact,
    mouse_position: record_mouse_position,
    mouse_button: record_button,
    raw_mouse: record_raw_mouse,
    apply: apply_visibility,
};

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let original = window::rsf_core_overlay_input_original();
    match rsf_core_overlay_route_message(window, message, wparam, lparam, &MESSAGE_OPS) {
        Disposition::Consume => 0,
        Disposition::ConsumeXButton => 1,
        Disposition::Forward if original != 0 => {
            let forward = unsafe { transmute::<isize, WNDPROC>(original) };
            unsafe { CallWindowProcW(forward, window, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "C" fn invoke_log(
    log: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    user: *mut c_void,
    message: *const c_char,
) {
    unsafe { crate::graphics_d3d11::rsf_call_native_log(log, user, message) };
}

static WINDOW_PLATFORM: WindowPlatform = WindowPlatform {
    install_cursor,
    retire_cursor,
    log: invoke_log,
};

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_overlay_input_install(
    window: HWND,
    options: *const Options,
) -> i32 {
    unsafe {
        window::rsf_core_overlay_input_install(
            window,
            options,
            window_proc as *const () as isize,
            &WINDOW_PLATFORM,
        )
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_overlay_input_uninstall() -> i32 {
    unsafe {
        window::rsf_core_overlay_input_uninstall(
            window_proc as *const () as isize,
            &WINDOW_PLATFORM,
        )
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_overlay_input_visible() -> u32 {
    rsf_core_overlay_visible()
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_overlay_input_poll_toggle() {
    let window = window::rsf_core_overlay_input_window();
    if window.is_null() {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    let focused = !foreground.is_null()
        && unsafe { GetAncestor(window, GA_ROOT) == GetAncestor(foreground, GA_ROOT) };
    let down = unsafe { GetAsyncKeyState(rsf_core_overlay_toggle_key() as i32) } < 0;
    rsf_core_overlay_toggle_poll(
        focused.into(),
        down.into(),
        unsafe { GetTickCount64() },
        apply_visibility,
    );
    let mut buttons = 0;
    if focused && rsf_core_overlay_visible() != 0 {
        for key in [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON] {
            if unsafe { GetAsyncKeyState(i32::from(key)) } < 0 {
                buttons |= button_bit(u32::from(key));
            }
        }
    }
    pointer::rsf_core_overlay_pointer_polled_buttons(buttons);
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_overlay_input_set_visible(visible: u32) {
    rsf_core_overlay_visibility_set(visible, unsafe { GetTickCount64() }, apply_visibility);
}

unsafe extern "C" fn sample_cursor(window: HWND, out: *mut ClientPoint) -> u32 {
    let mut point = POINT::default();
    if window.is_null()
        || unsafe { genuine_position(&mut point) == 0 || ScreenToClient(window, &mut point) == 0 }
    {
        return 0;
    }
    unsafe { out.write(client_point(window, point)) };
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_overlay_input_collect(
    out: *mut Input,
    width: u32,
    height: u32,
    delta: f32,
) -> i32 {
    unsafe { pointer::rsf_core_overlay_input_collect(out, width, height, delta, sample_cursor) }
}
