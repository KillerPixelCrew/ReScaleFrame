// SPDX-License-Identifier: MIT
use super::{Handle, Window, check, require, send};
use crate::abi::*;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::thread::sleep;
use std::time::Duration;
use windows::{
    Win32::{
        Foundation::HWND,
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
        UI::{
            Input::KeyboardAndMouse::VK_INSERT,
            WindowsAndMessaging::{WM_KEYDOWN, WM_KEYUP},
        },
    },
    core::{s, w},
};

static DOWN: AtomicBool = AtomicBool::new(false);
static FOREGROUND: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());

unsafe extern "system" fn key_state(key: i32) -> i16 {
    if key == i32::from(VK_INSERT.0) && DOWN.load(Ordering::Relaxed) {
        i16::MIN
    } else {
        0
    }
}

unsafe extern "system" fn foreground_window() -> HWND {
    HWND(FOREGROUND.load(Ordering::Relaxed))
}

unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: Handle, detour: Handle, original: *mut Handle) -> i32;
    fn MH_EnableHook(target: Handle) -> i32;
    fn MH_DisableHook(target: Handle) -> i32;
    fn MH_RemoveHook(target: Handle) -> i32;
}

fn poll(down: bool) {
    DOWN.store(down, Ordering::Relaxed);
    unsafe { rsf_overlay_input_poll_toggle() };
}

fn visible() -> bool {
    unsafe { rsf_overlay_input_visible() != 0 }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_overlay_hotkey() -> i32 {
    let window = Window::new(w!("RsfHotkeyFixture"), 64, 64);
    // Replace only this process's focus and key queries.
    require(
        unsafe { MH_Initialize() } == 0,
        "initialize MinHook fixture",
    );
    let module = check(unsafe { GetModuleHandleW(w!("user32.dll")) }, "get user32");
    let keys =
        unsafe { GetProcAddress(module, s!("GetAsyncKeyState")) }.expect("get key query") as Handle;
    let focus = unsafe { GetProcAddress(module, s!("GetForegroundWindow")) }
        .expect("get focus query") as Handle;
    for (target, detour) in [
        (keys, key_state as Handle),
        (focus, foreground_window as Handle),
    ] {
        require(
            unsafe { MH_CreateHook(target, detour, null_mut()) } == 0,
            "create poll detour",
        );
        require(unsafe { MH_EnableHook(target) } == 0, "enable poll detour");
    }
    let options = rsf_overlay_input_options {
        struct_size: size_of::<rsf_overlay_input_options>() as u32,
        abi_version: RSF_OVERLAY_INPUT_ABI_VERSION,
        toggle_virtual_key: u32::from(VK_INSERT.0),
        ..Default::default()
    };
    require(
        unsafe { rsf_overlay_input_install(window.hwnd.0, &options) } == 0,
        "install on hidden window",
    );
    let insert = usize::from(VK_INSERT.0);
    FOREGROUND.store(window.hwnd.0, Ordering::Relaxed);
    poll(false);
    require(!visible(), "poll starts closed");
    poll(true);
    require(visible(), "Insert opens without a window message");
    sleep(Duration::from_millis(280));
    poll(true);
    send(window.hwnd, WM_KEYDOWN, insert, 1);
    require(
        visible(),
        "held key and delayed message do not toggle twice",
    );

    poll(false);
    send(window.hwnd, WM_KEYUP, insert, 0xc000_0001);
    poll(true);
    require(!visible(), "second physical edge closes without messages");
    poll(false);
    FOREGROUND.store(null_mut(), Ordering::Relaxed);
    poll(true);
    sleep(Duration::from_millis(260));
    require(!visible(), "background presses do not open panel");
    FOREGROUND.store(window.hwnd.0, Ordering::Relaxed);
    poll(true);
    require(
        !visible(),
        "regaining focus with Insert held is not another press",
    );
    poll(false);
    poll(true);
    require(visible(), "fresh foreground press opens");
    poll(false);
    sleep(Duration::from_millis(260));

    DOWN.store(true, Ordering::Relaxed);
    send(window.hwnd, WM_KEYDOWN, insert, 1);
    poll(true);
    require(!visible(), "message-first route and polling share the edge");
    send(window.hwnd, WM_KEYDOWN, insert, 0x4000_0001);
    require(!visible(), "auto repeat stays closed");
    DOWN.store(false, Ordering::Relaxed);
    send(window.hwnd, WM_KEYUP, insert, 0xc000_0001);
    poll(false);

    unsafe { rsf_overlay_input_set_visible(1) };
    require(
        visible(),
        "explicit open is admitted immediately after a toggle",
    );
    unsafe { rsf_overlay_input_set_visible(0) };
    require(
        !visible(),
        "explicit close is admitted immediately after an open",
    );
    require(
        unsafe { rsf_overlay_input_uninstall() } == 0,
        "restore fixture window",
    );

    for target in [keys, focus] {
        require(
            unsafe { MH_DisableHook(target) } == 0,
            "disable poll detour",
        );
        require(unsafe { MH_RemoveHook(target) } == 0, "remove poll detour");
    }
    0
}
