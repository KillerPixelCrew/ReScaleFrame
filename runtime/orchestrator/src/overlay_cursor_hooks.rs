// SPDX-License-Identifier: MIT
use crate::overlay_window::diagnostic;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows_sys::{
    Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    core::w,
};

type Handle = *mut c_void;
const NAMES: [&CStr; 7] = [
    c"SetCursorPos",
    c"ClipCursor",
    c"GetCursorPos",
    c"ShowCursor",
    c"SetCursor",
    c"GetRawInputData",
    c"GetRawInputBuffer",
];
const COUNT: usize = NAMES.len();

#[repr(C)]
struct Platform {
    initialize: unsafe extern "C" fn() -> i32,
    create: unsafe extern "C" fn(Handle, Handle, *mut Handle) -> i32,
    enable: unsafe extern "C" fn(Handle) -> i32,
    disable: unsafe extern "C" fn(Handle) -> i32,
    remove: unsafe extern "C" fn(Handle) -> i32,
}

const _: () = assert!(size_of::<Platform>() == 5 * size_of::<Handle>());

struct Binding {
    target: Handle,
    disabled: bool,
}

// This address is a MinHook target. Only the cursor hook lifecycle lock
// allows using it for creation, disable and removal.
unsafe impl Send for Binding {}

static BINDINGS: Mutex<[Option<Binding>; COUNT]> = Mutex::new([const { None }; COUNT]);
static FORWARD: [AtomicPtr<c_void>; COUNT] = [const { AtomicPtr::new(null_mut()) }; COUNT];
static CALLS: AtomicU32 = AtomicU32::new(0);
static READY: AtomicBool = AtomicBool::new(false);

fn lock() -> MutexGuard<'static, [Option<Binding>; COUNT]> {
    BINDINGS.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn has_hooks() -> bool {
    lock().iter().any(Option::is_some)
}

fn retire(bindings: &mut [Option<Binding>; COUNT], platform: &Platform) -> bool {
    READY.store(false, Ordering::Release);
    for (index, entry) in bindings.iter_mut().enumerate().rev() {
        let Some(binding) = entry else { continue };
        if !binding.disabled {
            if unsafe { (platform.disable)(binding.target) } == 0 {
                continue;
            }
            // Late calls into a disabled detour can forward to user32 itself.
            // Keep the trampoline until any callers that read it have left.
            FORWARD[index].store(binding.target, Ordering::SeqCst);
            binding.disabled = true;
        }
        if CALLS.load(Ordering::SeqCst) == 0 && unsafe { (platform.remove)(binding.target) } != 0 {
            *entry = None;
        }
    }
    bindings.iter().all(Option::is_none)
}

enum InstallError {
    PendingRetirement,
    Module,
    Initialize(i32),
    Export(&'static CStr),
    Hook(&'static CStr, i32),
    Trampoline(&'static CStr),
}

fn install(
    bindings: &mut [Option<Binding>; COUNT],
    detours: &[Handle; COUNT],
    platform: &Platform,
) -> Result<(), InstallError> {
    if !retire(bindings, platform) {
        return Err(InstallError::PendingRetirement);
    }
    let module = unsafe { GetModuleHandleW(w!("user32.dll")) };
    if module.is_null() {
        return Err(InstallError::Module);
    }
    let status = unsafe { (platform.initialize)() };
    if status != 0 {
        return Err(InstallError::Initialize(status));
    }
    for (index, (&name, &detour)) in NAMES.iter().zip(detours).enumerate() {
        let Some(target) = (unsafe { GetProcAddress(module, name.as_ptr().cast()) }) else {
            return Err(InstallError::Export(name));
        };
        let target = target as *mut c_void;
        let mut trampoline = null_mut();
        let status = unsafe { (platform.create)(target, detour, &mut trampoline) };
        if status != 0 {
            return Err(InstallError::Hook(name, status));
        }
        bindings[index] = Some(Binding {
            target,
            disabled: false,
        });
        if trampoline.is_null() {
            return Err(InstallError::Trampoline(name));
        }
        FORWARD[index].store(trampoline, Ordering::SeqCst);
        let status = unsafe { (platform.enable)(target) };
        if status != 0 {
            return Err(InstallError::Hook(name, status));
        }
    }
    READY.store(true, Ordering::Release);
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_cursor_install(detours: &[Handle; COUNT], platform: &Platform) {
    let (result, rollback_pending) = {
        let mut bindings = lock();
        let result = install(&mut bindings, detours, platform);
        let rollback_pending = result.is_err()
            && !matches!(result, Err(InstallError::PendingRetirement))
            && !retire(&mut bindings, platform);
        (result, rollback_pending)
    };
    match result {
        Ok(()) => diagnostic(&format!(
            "overlay input: cursor detours in place, {COUNT} user32 functions"
        )),
        Err(InstallError::PendingRetirement) => {
            diagnostic("overlay input: previous cursor hooks are still retiring")
        }
        Err(InstallError::Module) => {
            diagnostic("overlay input: user32 is not loaded, so the cursor stays the game's")
        }
        Err(InstallError::Initialize(status)) => diagnostic(&format!(
            "overlay input: MinHook did not initialise ({status}), so the cursor stays the game's"
        )),
        Err(InstallError::Export(name)) => diagnostic(&format!(
            "overlay input: {} is unavailable, so the cursor stays the game's",
            name.to_string_lossy()
        )),
        Err(InstallError::Hook(name, status)) => diagnostic(&format!(
            "overlay input: {} could not be detoured ({status}), so the cursor stays the game's",
            name.to_string_lossy()
        )),
        Err(InstallError::Trampoline(name)) => diagnostic(&format!(
            "overlay input: {} did not return a forwarding target",
            name.to_string_lossy()
        )),
    }
    if rollback_pending {
        diagnostic("overlay input: cursor hook rollback is pending");
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_cursor_retire(platform: &Platform) -> i32 {
    i32::from(retire(&mut lock(), platform))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_cursor_ready() -> u32 {
    READY.load(Ordering::Acquire).into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_cursor_enter() {
    CALLS.fetch_add(1, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_cursor_leave() {
    CALLS.fetch_sub(1, Ordering::SeqCst);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_cursor_forward(index: u32) -> Handle {
    FORWARD[index as usize].load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::without_provenance_mut;

    static REMOVE_ALLOWED: AtomicBool = AtomicBool::new(false);
    static REMOVALS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn initialize() -> i32 {
        0
    }
    unsafe extern "C" fn create(_: Handle, _: Handle, _: *mut Handle) -> i32 {
        0
    }
    unsafe extern "C" fn enable(_: Handle) -> i32 {
        0
    }
    unsafe extern "C" fn disable(_: Handle) -> i32 {
        1
    }
    unsafe extern "C" fn remove(_: Handle) -> i32 {
        REMOVALS.fetch_add(1, Ordering::Relaxed);
        i32::from(REMOVE_ALLOWED.load(Ordering::Relaxed))
    }

    #[test]
    fn retirement_keeps_a_busy_or_refused_target_for_retry() {
        let platform = Platform {
            initialize,
            create,
            enable,
            disable,
            remove,
        };
        let target = without_provenance_mut(10);
        let trampoline = without_provenance_mut(20);
        let mut bindings = [const { None }; COUNT];
        bindings[0] = Some(Binding {
            target,
            disabled: false,
        });
        FORWARD[0].store(trampoline, Ordering::SeqCst);

        rsf_core_overlay_cursor_enter();
        assert_eq!(rsf_core_overlay_cursor_forward(0), trampoline);
        assert!(!retire(&mut bindings, &platform));
        assert_eq!(rsf_core_overlay_cursor_forward(0), target);
        assert_eq!(REMOVALS.load(Ordering::Relaxed), 0);
        rsf_core_overlay_cursor_leave();

        assert!(!retire(&mut bindings, &platform));
        assert!(bindings[0].is_some());
        REMOVE_ALLOWED.store(true, Ordering::Relaxed);
        assert!(retire(&mut bindings, &platform));
        assert_eq!(rsf_core_overlay_cursor_forward(0), target);
    }
}
