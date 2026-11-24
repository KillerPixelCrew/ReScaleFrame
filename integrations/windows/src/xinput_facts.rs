// SPDX-License-Identifier: MIT
//! Observes successful public XInput reads from modules the process already loaded.
use crate::minhook;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::SeqCst};
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::Foundation::{ERROR_DEVICE_NOT_CONNECTED, ERROR_SUCCESS};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XUSER_MAX_COUNT};
use windows_sys::core::{s, w};

type Receipt = unsafe extern "C" fn();
type Read = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

const DETOURS: [Read; 6] = [
    read::<0>, read::<1>, read::<2>, read::<3>, read::<4>, read::<5>,
];
const SLOT_COUNT: usize = DETOURS.len();

// One observation owner per adapter DLL. Lifecycle calls serialize hook mutations.
static TARGETS: Mutex<[usize; SLOT_COUNT]> = Mutex::new([0; SLOT_COUNT]);
static ORIGINALS: [AtomicPtr<c_void>; SLOT_COUNT] =
    [const { AtomicPtr::new(ptr::null_mut()) }; SLOT_COUNT];
static READERS: AtomicUsize = AtomicUsize::new(0);
static RECEIVED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

struct Reader;

impl Reader {
    fn enter() -> Self {
        READERS.fetch_add(1, SeqCst);
        Self
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        READERS.fetch_sub(1, SeqCst);
    }
}

unsafe extern "system" fn read<const SLOT: usize>(index: u32, state: *mut XINPUT_STATE) -> u32 {
    let _reader = Reader::enter();
    let original = ORIGINALS[SLOT].load(SeqCst);
    if original.is_null() {
        return ERROR_DEVICE_NOT_CONNECTED;
    }
    // MinHook returned a trampoline for this slot's XInput entry.
    let original = unsafe { std::mem::transmute::<*mut c_void, Read>(original) };
    let result = unsafe { original(index, state) };
    if result == ERROR_SUCCESS && index < XUSER_MAX_COUNT && !state.is_null() {
        let callback = RECEIVED.load(SeqCst);
        if !callback.is_null() {
            let callback = unsafe { std::mem::transmute::<*mut c_void, Receipt>(callback) };
            unsafe { callback() };
        }
    }
    result
}

// The callback belongs to the adapter and must remain callable until its readers drain.
#[unsafe(no_mangle)]
extern "C" fn rsf_xinput_facts_install_loaded(callback: Option<Receipt>) {
    let mut targets = TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
    RECEIVED.store(
        callback.map_or(ptr::null_mut(), |f| f as *mut c_void),
        SeqCst,
    );
    for name in [
        w!("xinput1_4.dll"),
        w!("xinput1_3.dll"),
        w!("xinput9_1_0.dll"),
    ] {
        let module = unsafe { GetModuleHandleW(name) };
        if module.is_null() {
            continue;
        }
        // MAKEINTRESOURCEA(100) identifies XInputGetStateEx by ordinal.
        for symbol in [s!("XInputGetState"), 100usize as *const u8] {
            let Some(target) = (unsafe { GetProcAddress(module, symbol) }) else {
                continue;
            };
            if targets.contains(&(target as usize)) {
                continue;
            }
            let Some(slot) = targets.iter().position(|&address| address == 0) else {
                return;
            };
            let address = target as *mut c_void;
            let mut original = ptr::null_mut();
            if !unsafe { minhook::create(address, DETOURS[slot] as *mut c_void, &mut original) } {
                continue;
            }
            targets[slot] = address as usize;
            ORIGINALS[slot].store(original, SeqCst);
            if !unsafe { minhook::enable(address) } && unsafe { minhook::remove(address) } {
                targets[slot] = 0;
                ORIGINALS[slot].store(ptr::null_mut(), SeqCst);
            }
        }
    }
}

fn disable(targets: &[usize; SLOT_COUNT]) -> bool {
    RECEIVED.store(ptr::null_mut(), SeqCst);
    let mut disabled = true;
    for &target in targets.iter().filter(|&&address| address != 0) {
        disabled &= unsafe { minhook::disable(target as *mut c_void) };
    }
    disabled
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xinput_facts_disable() {
    let targets = TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
    disable(&targets);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_xinput_facts_remove() -> bool {
    let mut targets = TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
    if !disable(&targets) || READERS.load(SeqCst) != 0 {
        return false;
    }
    let mut removed = true;
    for (target, original) in targets.iter_mut().zip(&ORIGINALS) {
        if *target == 0 {
            continue;
        }
        if unsafe { minhook::remove(*target as *mut c_void) } {
            original.store(ptr::null_mut(), SeqCst);
            *target = 0;
        } else {
            removed = false;
        }
    }
    removed
}
