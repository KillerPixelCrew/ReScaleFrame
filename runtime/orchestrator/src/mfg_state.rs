// SPDX-License-Identifier: MIT
use crate::sdk_module::SdkModule;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

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
    prepare_provider: unsafe extern "C" fn(Handle, Handle, u32, &mut [c_char; REPORT_BYTES]) -> i32,
}

const _: () = assert!(size_of::<Platform>() == 4 * size_of::<Handle>());

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
    let Some(wrapper) =
        SdkModule::adopt_system(unsafe { (platform.load_wrapper)(directory, require_signature) })
    else {
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
    let prepared = unsafe { (platform.prepare_provider)(wrapper, get_state, maximum, report) };
    if prepared < 0 {
        return 0;
    }
    owner.state().status = if prepared == 0 { REFUSED } else { READY };
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
