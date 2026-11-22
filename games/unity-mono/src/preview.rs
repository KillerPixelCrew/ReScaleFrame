// SPDX-License-Identifier: MIT
//! Unity Editor window selection and visibility. Runtime receives only a destination token.
use std::{
    ptr,
    sync::{Mutex, MutexGuard, PoisonError},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SetLastError,
        WPARAM,
    },
    Graphics::Gdi::ScreenToClient,
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::WindowsAndMessaging::*,
};
use windows_sys::core::w;

const CLASS: *const u16 = w!("ReScaleFrame.Unity.EditorPreview");

#[repr(C)]
#[derive(Clone, Copy)]
struct Target {
    struct_size: u32,
    version: u32,
    handle: HWND,
    generation: u64,
}
const _: () = {
    assert!(size_of::<Target>() == 24);
    assert!(std::mem::offset_of!(Target, generation) == 16);
};

#[derive(Clone, Copy, Default)]
struct Window {
    parent: usize,
    child: usize,
    generation: u64,
    visible: bool,
}

impl Window {
    fn child(self) -> HWND {
        ptr::without_provenance_mut(self.child)
    }
    fn parent(self) -> HWND {
        ptr::without_provenance_mut(self.parent)
    }

    fn owns_child(self) -> bool {
        self.child != 0
            && unsafe { GetWindowLongPtrW(self.child(), GWLP_USERDATA) as u64 } == self.generation
    }

    fn live(self) -> bool {
        self.owns_child()
            && unsafe { IsWindow(self.parent()) != 0 && GetParent(self.child()) == self.parent() }
    }
}

struct State {
    window: Window,
    // Distinguishes a new child even if Windows recycles its HWND.
    generation: u64,
    hide_revision: u64,
}
static STATE: Mutex<State> = Mutex::new(State {
    window: Window {
        parent: 0,
        child: 0,
        generation: 0,
        visible: false,
    },
    generation: 0,
    hide_revision: 0,
});
static CONFIGURE: Mutex<()> = Mutex::new(());

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
            let written = unsafe {
                SetLastError(0);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams.addr() as isize) != 0
                    || GetLastError() == 0
            };
            if !written {
                return 0;
            }
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        }
        WM_NCHITTEST => return HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => return 1,
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct Search {
    region: RECT,
    parent: HWND,
    area: u64,
}

unsafe extern "system" fn consider(candidate: HWND, data: LPARAM) -> i32 {
    let search = unsafe { &mut *(data as *mut Search) };
    let mut process = 0;
    let mut bounds = RECT::default();
    if unsafe {
        GetWindowThreadProcessId(candidate, &mut process);
        process != GetCurrentProcessId()
            || IsWindowVisible(candidate) == 0
            || GetWindowRect(candidate, &mut bounds) == 0
    } {
        return 1;
    }
    let region = &search.region;
    if region.left >= bounds.left
        && region.top >= bounds.top
        && region.right <= bounds.right
        && region.bottom <= bounds.bottom
    {
        let width = i64::from(bounds.right) - i64::from(bounds.left);
        let height = i64::from(bounds.bottom) - i64::from(bounds.top);
        if width > 0 && height > 0 {
            let area = width as u64 * height as u64;
            if area < search.area {
                search.area = area;
                search.parent = candidate;
            }
        }
    }
    1
}

fn create(region: RECT, width: i32, height: i32, generation: u64) -> Option<Window> {
    let mut search = Search {
        region,
        parent: ptr::null_mut(),
        area: u64::MAX,
    };
    unsafe { EnumWindows(Some(consider), (&raw mut search).addr() as LPARAM) };
    let parent = search.parent;
    if unsafe { GetWindowThreadProcessId(parent, ptr::null_mut()) != GetCurrentThreadId() } {
        return None;
    }
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        lpszClassName: CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS } {
        return None;
    }
    let child = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            CLASS,
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            width,
            height,
            parent,
            ptr::null_mut(),
            instance,
            ptr::without_provenance(generation as usize),
        )
    };
    (!child.is_null()).then_some(Window {
        parent: parent.addr(),
        child: child.addr(),
        generation,
        visible: false,
    })
}

fn discard(window: Window) {
    if window.owns_child() {
        unsafe {
            if GetWindowThreadProcessId(window.child(), ptr::null_mut()) == GetCurrentThreadId() {
                DestroyWindow(window.child());
            } else {
                ShowWindowAsync(window.child(), SW_HIDE);
            }
        }
    }
}

fn hide() {
    let mut state = state();
    state.hide_revision = state.hide_revision.wrapping_add(1);
    state.window.visible = false;
    if state.window.live() {
        unsafe { ShowWindowAsync(state.window.child(), SW_HIDE) };
    } else {
        state.window = Window::default();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_preview_region(
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    show: bool,
) -> bool {
    if !show || width <= 0 || height <= 0 {
        hide();
        return true;
    }
    let (Some(right), Some(bottom)) = (left.checked_add(width), top.checked_add(height)) else {
        hide();
        return false;
    };
    // Win32 positioning can dispatch messages, so do not hold STATE across it.
    let Ok(_configure) = CONFIGURE.try_lock() else {
        return false;
    };
    let (mut window, revision) = {
        let state = state();
        (state.window, state.hide_revision)
    };
    if !window.live() {
        state().window = Window::default();
        discard(window);
        let generation = {
            let mut state = state();
            let Some(next) = state.generation.checked_add(1) else {
                return false;
            };
            state.generation = next;
            next
        };
        let Some(new) = create(
            RECT {
                left,
                top,
                right,
                bottom,
            },
            width,
            height,
            generation,
        ) else {
            return false;
        };
        window = new;
    }
    let mut point = POINT { x: left, y: top };
    let positioned = unsafe {
        ScreenToClient(window.parent(), &mut point) != 0
            && SetWindowPos(
                window.child(),
                ptr::null_mut(),
                point.x,
                point.y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            ) != 0
    };
    if !positioned || !window.live() {
        state().window = Window::default();
        discard(window);
        return false;
    }
    let mut state = state();
    window.visible = revision == state.hide_revision;
    state.window = window;
    window.visible
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_preview_parent() -> HWND {
    let state = state();
    if state.window.live() {
        state.window.parent()
    } else {
        ptr::null_mut()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_preview_owns(child: HWND) -> bool {
    let state = state();
    state.window.child() == child && state.window.live()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_preview_hide() {
    hide();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_preview_target(target: *mut Target) -> bool {
    let state = state();
    let window = state.window;
    if !window.visible || !window.live() {
        return false;
    }
    unsafe {
        target.write(Target {
            struct_size: size_of::<Target>() as u32,
            version: 1,
            handle: window.child(),
            generation: window.generation,
        })
    };
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_preview_show(generation: u64) {
    let state = state();
    let window = state.window;
    if window.visible && window.generation == generation && window.live() {
        unsafe { ShowWindowAsync(window.child(), SW_SHOWNOACTIVATE) };
    }
}
