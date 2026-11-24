// SPDX-License-Identifier: MIT
//! Unity Editor window selection, visibility and ownership of the Runtime output handle.
use crate::sys::{
    RSF_D3D12_WINDOW_TARGET_ABI_VERSION, rsf_d3d12_window_create, rsf_d3d12_window_destroy,
    rsf_d3d12_window_drained, rsf_d3d12_window_observer, rsf_d3d12_window_prepare,
    rsf_d3d12_window_present, rsf_d3d12_window_presenter, rsf_d3d12_window_target,
};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::rsf_game_render_pass;
use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{Mutex, MutexGuard, TryLockError},
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

struct Output(NonNull<rsf_d3d12_window_presenter>);

// The package can use the handle from different threads only under OUTPUT.
// Runtime owns and synchronizes the D3D12 state behind the handle.
unsafe impl Send for Output {}

static OUTPUT: Mutex<Option<Output>> = Mutex::new(None);

fn try_output() -> Option<MutexGuard<'static, Option<Output>>> {
    match OUTPUT.try_lock() {
        Ok(output) => Some(output),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

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
    lock_or_recover(&STATE)
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

pub(crate) fn hide() {
    let mut state = state();
    state.hide_revision = state.hide_revision.wrapping_add(1);
    state.window.visible = false;
    if state.window.live() {
        unsafe { ShowWindowAsync(state.window.child(), SW_HIDE) };
    } else {
        state.window = Window::default();
    }
}

pub(crate) fn rsf_unity_preview_region(
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

pub(crate) fn rsf_unity_preview_parent() -> HWND {
    let state = state();
    if state.window.live() {
        state.window.parent()
    } else {
        ptr::null_mut()
    }
}

pub(crate) fn rsf_unity_preview_owns(child: HWND) -> bool {
    let state = state();
    state.window.child() == child && state.window.live()
}

fn target() -> Option<rsf_d3d12_window_target> {
    let state = state();
    let window = state.window;
    (window.visible && window.live()).then(|| rsf_d3d12_window_target {
        struct_size: size_of::<rsf_d3d12_window_target>() as u32,
        abi_version: RSF_D3D12_WINDOW_TARGET_ABI_VERSION,
        window: window.child(),
        generation: window.generation,
    })
}

fn show(generation: u64) {
    let state = state();
    let window = state.window;
    if window.visible && window.generation == generation && window.live() {
        unsafe { ShowWindowAsync(window.child(), SW_SHOWNOACTIVATE) };
    }
}

pub(crate) unsafe fn rsf_unity_preview_prepare(
    device: *mut c_void,
    queue: *mut c_void,
    width: u32,
    height: u32,
    observer: rsf_d3d12_window_observer,
) -> u32 {
    let Some(mut output) = try_output() else {
        return 0;
    };
    let Some(target) = target() else {
        return 0;
    };
    if output.is_none() {
        *output = NonNull::new(unsafe { rsf_d3d12_window_create(observer) }).map(Output);
    }
    let Some(output) = output.as_ref() else {
        return 0;
    };
    // The bridge lends the device and queue for this call. STATE is unlocked, so the
    // Runtime can ask the adapter whether it owns the HWND during DXGI creation.
    u32::from(
        unsafe {
            rsf_d3d12_window_prepare(output.0.as_ptr(), &target, device, queue, width, height)
        } != 0,
    )
}

pub(crate) unsafe fn rsf_unity_preview_present(
    texture: *mut c_void,
    pass: *const rsf_game_render_pass,
    common_state: u32,
) -> u32 {
    let Some(output) = try_output() else {
        return 0;
    };
    let (Some(output), Some(target)) = (output.as_ref(), target()) else {
        return 0;
    };
    // Keep the output gate across the observer and DXGI Present. Nested preview
    // calls refuse at try_output, while the window guard can still read STATE.
    if unsafe { rsf_d3d12_window_present(output.0.as_ptr(), &target, texture, pass, common_state) }
        == 0
    {
        return 0;
    }
    show(target.generation);
    1
}

pub(crate) fn rsf_unity_preview_drained() -> bool {
    try_output().is_some_and(|output| {
        output
            .as_ref()
            .is_none_or(|output| unsafe { rsf_d3d12_window_drained(output.0.as_ptr()) != 0 })
    })
}

pub(crate) fn rsf_unity_preview_release() {
    // The package closes bridge admission and proves the drain before committing.
    let mut output = lock_or_recover(&OUTPUT);
    if output
        .as_ref()
        .is_some_and(|output| unsafe { rsf_d3d12_window_destroy(output.0.as_ptr()) != 0 })
    {
        *output = None;
    }
}
