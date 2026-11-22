// SPDX-License-Identifier: MIT
//! Observes dequeued window messages and successful public XInput reads.
use super::{Scope, lock, minhook::*, trampoline};
use rsf_windows::rsf_win32_input_kind;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::{ffi::c_void, ptr};
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_HOOK_HANDLE, GetLastError, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::w;

type Message = unsafe extern "C" fn(u32, u32);
type Controller = unsafe extern "C" fn();
type XInputRead = unsafe extern "system" fn(u32, *mut c_void) -> u32;

static MESSAGE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CONTROLLER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static MESSAGE_READERS: AtomicU32 = AtomicU32::new(0);
static CONTROLLER_READERS: AtomicU32 = AtomicU32::new(0);
static INPUT_ENABLED: AtomicBool = AtomicBool::new(false);
static MESSAGE_HOOK: Mutex<usize> = Mutex::new(0);
static TARGETS: Mutex<[usize; 6]> = Mutex::new([0; 6]);
static ORIGINALS: [AtomicPtr<c_void>; 6] = [const { AtomicPtr::new(ptr::null_mut()) }; 6];

unsafe extern "system" fn received_message(code: i32, removed: WPARAM, arg: LPARAM) -> LRESULT {
    let _reader = Scope::enter(&MESSAGE_READERS);
    if code >= 0
        && removed == PM_REMOVE as usize
        && let Some(message) = unsafe { (arg as *const MSG).as_ref() }
        && let Some(callback) = unsafe { trampoline::<Message>(&MESSAGE) }
    {
        unsafe {
            callback(
                rsf_win32_input_kind(message.message, message.lParam),
                message.message,
            )
        };
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, removed, arg) }
}

unsafe extern "system" fn controller_read<const SLOT: usize>(
    index: u32,
    state: *mut c_void,
) -> u32 {
    let _reader = Scope::enter(&CONTROLLER_READERS);
    let Some(native) = (unsafe { trampoline::<XInputRead>(&ORIGINALS[SLOT]) }) else {
        return 1167; // ERROR_DEVICE_NOT_CONNECTED. Only reachable without a trampoline.
    };
    let result = unsafe { native(index, state) };
    if INPUT_ENABLED.load(Ordering::SeqCst)
        && result == 0
        && index < 4
        && !state.is_null()
        && let Some(callback) = unsafe { trampoline::<Controller>(&CONTROLLER) }
    {
        unsafe { callback() };
    }
    result
}

pub(super) unsafe extern "C" fn install(message: Message, controller: Controller) {
    MESSAGE.store(message as *mut c_void, Ordering::Release);
    CONTROLLER.store(controller as *mut c_void, Ordering::Release);
    INPUT_ENABLED.store(true, Ordering::SeqCst);
    {
        let mut hook = lock(&MESSAGE_HOOK);
        if *hook == 0 {
            *hook = unsafe {
                SetWindowsHookExW(
                    WH_GETMESSAGE,
                    Some(received_message),
                    ptr::null_mut(),
                    GetCurrentThreadId(),
                )
            } as usize;
        }
    }
    let detours: [XInputRead; 6] = [
        controller_read::<0>,
        controller_read::<1>,
        controller_read::<2>,
        controller_read::<3>,
        controller_read::<4>,
        controller_read::<5>,
    ];
    let mut targets = lock(&TARGETS);
    for module in [
        w!("xinput1_4.dll"),
        w!("xinput1_3.dll"),
        w!("xinput9_1_0.dll"),
    ] {
        let module = unsafe { GetModuleHandleW(module) };
        if module.is_null() {
            continue;
        }
        for symbol in [c"XInputGetState".as_ptr().cast(), 100usize as *const u8] {
            let Some(target) = (unsafe { GetProcAddress(module, symbol) }) else {
                continue;
            };
            let address = target as usize;
            if targets.contains(&address) {
                continue;
            }
            let Some(slot) = targets.iter().position(|&slot| slot == 0) else {
                return;
            };
            let mut original = ptr::null_mut();
            if unsafe { MH_CreateHook(address as _, detours[slot] as *mut c_void, &mut original) }
                == 0
            {
                ORIGINALS[slot].store(original, Ordering::Release);
                targets[slot] = address;
                if unsafe { MH_EnableHook(address as _) } != 0
                    && unsafe { MH_RemoveHook(address as _) } == 0
                {
                    targets[slot] = 0;
                    ORIGINALS[slot].store(ptr::null_mut(), Ordering::Release);
                }
            }
        }
    }
}

pub(super) unsafe extern "C" fn disable() {
    INPUT_ENABLED.store(false, Ordering::SeqCst);
    for &target in lock(&TARGETS).iter().filter(|&&target| target != 0) {
        unsafe { MH_DisableHook(target as _) };
    }
}

pub(super) unsafe extern "C" fn idle() -> bool {
    MESSAGE_READERS.load(Ordering::SeqCst) == 0 && CONTROLLER_READERS.load(Ordering::SeqCst) == 0
}

pub(super) unsafe extern "C" fn unhook() -> bool {
    let mut hook = lock(&MESSAGE_HOOK);
    if *hook == 0 {
        return true;
    }
    if unsafe { UnhookWindowsHookEx(*hook as _) } == 0
        && unsafe { GetLastError() } != ERROR_INVALID_HOOK_HANDLE
    {
        return false;
    }
    *hook = 0;
    true
}

pub(super) unsafe extern "C" fn remove() -> bool {
    unsafe { disable() };
    if !unsafe { idle() } {
        return false;
    }
    let mut targets = lock(&TARGETS);
    for (slot, target) in targets
        .iter_mut()
        .enumerate()
        .filter(|(_, target)| **target != 0)
    {
        if !matches!(unsafe { MH_RemoveHook(*target as _) }, 0 | 4) {
            return false;
        }
        *target = 0;
        ORIGINALS[slot].store(ptr::null_mut(), Ordering::Release);
    }
    true
}
