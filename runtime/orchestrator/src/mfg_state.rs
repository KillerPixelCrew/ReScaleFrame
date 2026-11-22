// SPDX-License-Identifier: MIT
use crate::sdk_module::SdkModule;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExW,
};

type Handle = *mut c_void;

const NOT_APPLICABLE: u32 = 0;
const READY: u32 = 1;
const REFUSED: u32 = 2;
const REPORT_BYTES: usize = 512;

#[repr(C)]
struct Platform {
    supported: unsafe extern "C" fn(Handle) -> u32,
    load_wrapper: unsafe extern "C" fn(*const u16, u32) -> Handle,
    prepare_wrapper: unsafe extern "C" fn(Handle) -> u32,
    provider_module: unsafe extern "C" fn() -> Handle,
    prepare_provider: unsafe extern "C" fn(Handle, u32, &mut [c_char; REPORT_BYTES]) -> u32,
}

const _: () = assert!(size_of::<Platform>() == 5 * size_of::<Handle>());

struct State {
    wrapper: Option<SdkModule>,
    maximum: u32,
    status: u32,
}

impl State {
    const fn new() -> Self {
        Self {
            wrapper: None,
            maximum: 1,
            status: NOT_APPLICABLE,
        }
    }
}

struct Owner(Mutex<State>);

impl Owner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn release(&self) {
        let _update = update();
        let previous = std::mem::replace(&mut *self.state(), State::new());
        drop(previous);
    }
}

// The native compatibility calls operate on process-wide SDK modules.
static UPDATE: Mutex<()> = Mutex::new(());

fn update() -> MutexGuard<'static, ()> {
    UPDATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn try_update() -> Option<MutexGuard<'static, ()>> {
    match UPDATE.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_mfg_create() -> *mut Owner {
    crate::allocate_handle(Owner(Mutex::new(State::new())))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_prime(
    owner: &Owner,
    platform: &Platform,
    device: Handle,
    directory: *const u16,
    require_signature: u32,
) -> u32 {
    let Some(_update) = try_update() else {
        return 0;
    };
    if device.is_null() || unsafe { (platform.supported)(device) } == 0 {
        owner.state().status = NOT_APPLICABLE;
        return 0;
    }
    {
        let mut state = owner.state();
        if state.wrapper.is_some() {
            return 0;
        }
        state.status = REFUSED;
    }
    // The host owns a validated, NUL-terminated directory of at most 1024 units.
    let path: Vec<_> = (0..)
        .map(|index| unsafe { *directory.add(index) })
        .take_while(|&unit| unit != 0)
        .chain("\\sl.dlss_g.dll".encode_utf16())
        .chain([0])
        .collect();
    let Some(wrapper) = SdkModule::adopt_system(unsafe {
        (platform.load_wrapper)(path.as_ptr(), require_signature)
    }) else {
        return 0;
    };
    let maximum = unsafe { (platform.prepare_wrapper)(wrapper.as_ptr()) }.max(1);
    let mut state = owner.state();
    state.maximum = maximum;
    state.wrapper = Some(wrapper);
    // Zero means no report. Any loaded wrapper logs its effective capacity.
    state.maximum
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_prepare(
    owner: &Owner,
    platform: &Platform,
    device: Handle,
    get_state: Handle,
    report: &mut [c_char; REPORT_BYTES],
) -> u32 {
    let Some(_update) = try_update() else {
        return 0;
    };
    let (wrapper, maximum) = {
        let state = owner.state();
        if state.maximum <= 1 || device.is_null() || get_state.is_null() {
            return 0;
        }
        let Some(wrapper) = state.wrapper.as_ref() else {
            return 0;
        };
        // The update guard prevents releasing the module during the call.
        (wrapper.as_ptr(), state.maximum)
    };
    let mut get_state_owner = std::ptr::null_mut();
    let resolved = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            get_state.cast(),
            &mut get_state_owner,
        )
    };
    let get_state_owner = if resolved != 0 {
        SdkModule::adopt_system(get_state_owner)
    } else {
        None
    };
    let prepared = get_state_owner.is_some_and(|module| {
        if module.as_ptr() != wrapper {
            return false;
        }
        let provider = SdkModule::adopt_system(unsafe { (platform.provider_module)() });
        provider.is_some_and(|provider| {
            (unsafe { (platform.prepare_provider)(provider.as_ptr(), maximum, report) }) != 0
        })
    });
    if report[0] == 0 {
        for (dst, &byte) in report.iter_mut().zip(
            c"RTX 40 MFG unlock refused: wrapper or provider ownership mismatch"
                .to_bytes_with_nul(),
        ) {
            *dst = byte as c_char;
        }
    }
    owner.state().status = if prepared { READY } else { REFUSED };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_status(owner: *const Owner, status: &mut u32, maximum: &mut u32) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        let state = owner.state();
        *status = state.status;
        *maximum = state.maximum;
    } else {
        *status = NOT_APPLICABLE;
        *maximum = 1;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_release(owner: *const Owner) {
    if let Some(owner) = unsafe { owner.as_ref() } {
        owner.release();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_mfg_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        let owner = unsafe { Box::from_raw(owner) };
        owner.release();
    }
}
