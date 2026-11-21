// SPDX-License-Identifier: MIT
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError};

type Reason = *const c_char;
type Address = usize;
type SinkCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type InputObserver = unsafe extern "C" fn(u8);
type Add = unsafe extern "C" fn(*mut c_void, Address, Address, u32, *mut Reason) -> bool;
type Probe = unsafe extern "C" fn(*mut c_void, Add, *mut Reason) -> bool;

#[repr(C)]
struct Platform {
    initialize: unsafe extern "C" fn() -> bool,
    create: unsafe extern "C" fn(Address, Address, *mut Address) -> bool,
    enable: unsafe extern "C" fn(Address) -> Reason,
    disable: unsafe extern "C" fn(Address),
}

#[derive(Clone, Copy)]
struct PresentSink {
    before_present: SinkCallback,
    resize: Option<SinkCallback>,
    user: Address,
}

#[derive(Clone, Copy)]
enum Binding {
    Input(Address),
    Present(PresentSink),
}

struct Hook {
    target: Address,
    original: Address,
    method: u32,
}

struct State {
    hooks: Vec<Hook>,
    input_attempted: bool,
    present_attempted: bool,
    gate: Address,
    sink: Option<PresentSink>,
    observer: Option<InputObserver>,
}

struct Hooks {
    installation: Mutex<()>,
    state: Mutex<State>,
}

impl Hooks {
    const fn new() -> Self {
        Self {
            installation: Mutex::new(()),
            state: Mutex::new(State {
                hooks: Vec::new(),
                input_attempted: false,
                present_attempted: false,
                gate: 0,
                sink: None,
                observer: None,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn install(&self, binding: Binding, platform: &Platform, probe: Probe) -> Result<(), Reason> {
        let Ok(_installation) = self.installation.try_lock() else {
            return Err(c"another overlay hook installation is in progress".as_ptr());
        };
        let first = {
            let mut state = self.state();
            match binding {
                Binding::Input(gate) if state.input_attempted => {
                    return if state.gate == gate {
                        Ok(())
                    } else if state.gate == 0 {
                        Err(c"input hook installation already failed".as_ptr())
                    } else {
                        Err(c"input hooks already own a different gate".as_ptr())
                    };
                }
                Binding::Input(_) => state.input_attempted = true,
                Binding::Present(_) if state.present_attempted => {
                    return Err(c"Present hook installation already attempted".as_ptr());
                }
                Binding::Present(_) => state.present_attempted = true,
            }
            state.hooks.len()
        };
        if !unsafe { (platform.initialize)() } {
            return Err(c"MinHook initialization failed".as_ptr());
        }
        let mut reason = c"overlay hook discovery failed".as_ptr();
        let mut registration = Registration {
            hooks: self,
            platform,
        };
        if !unsafe {
            probe(
                (&mut registration as *mut Registration<'_>).cast(),
                add,
                &mut reason,
            )
        } {
            return Err(reason);
        }

        // Publish the complete table and binding before the first enabled detour can run.
        let end = {
            let mut state = self.state();
            state.publish(binding, true);
            state.hooks.len()
        };
        for index in first..end {
            let target = self.state().hooks[index].target;
            let refusal = unsafe { (platform.enable)(target) };
            if !refusal.is_null() {
                self.state().publish(binding, false);
                for index in first..end {
                    let target = self.state().hooks[index].target;
                    unsafe { (platform.disable)(target) };
                }
                // An already-entered detour can still read its original after DisableHook.
                return Err(refusal);
            }
        }
        Ok(())
    }
}

impl State {
    fn publish(&mut self, binding: Binding, active: bool) {
        match binding {
            Binding::Input(gate) => self.gate = if active { gate } else { 0 },
            Binding::Present(sink) => self.sink = active.then_some(sink),
        }
    }
}

struct Registration<'a> {
    hooks: &'a Hooks,
    platform: &'a Platform,
}

unsafe extern "C" fn add(
    registration: *mut c_void,
    target: Address,
    detour: Address,
    method: u32,
    reason: *mut Reason,
) -> bool {
    let registration = unsafe { &*registration.cast::<Registration<'_>>() };
    let result = {
        let state = registration.hooks.state();
        if target == 0 || detour == 0 {
            Err(c"a hook target resolved to null")
        } else if let Some(hook) = state.hooks.iter().find(|hook| hook.target == target) {
            if hook.method == method {
                return true;
            }
            Err(c"two different methods share one code address")
        } else if state.hooks.len() == 32 {
            Err(c"hook table is full")
        } else {
            Ok(())
        }
    };
    if let Err(message) = result {
        unsafe { reason.write(message.as_ptr()) };
        return false;
    }
    let mut original = 0;
    if !unsafe { (registration.platform.create)(target, detour, &mut original) } {
        unsafe { reason.write(c"MinHook refused to create a hook".as_ptr()) };
        return false;
    }
    registration.hooks.state().hooks.push(Hook {
        target,
        original,
        method,
    });
    true
}

static HOOKS: Hooks = Hooks::new();
thread_local! {
    static IN_PRESENT: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_overlay_install_input(
    gate: Address,
    platform: &Platform,
    probe: Probe,
    reason: &mut Reason,
) -> bool {
    match HOOKS.install(Binding::Input(gate), platform, probe) {
        Ok(()) => true,
        Err(message) => {
            *reason = message;
            false
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_overlay_install_present(
    before_present: Option<SinkCallback>,
    resize: Option<SinkCallback>,
    user: Address,
    platform: &Platform,
    probe: Probe,
    reason: &mut Reason,
) -> bool {
    let Some(before_present) = before_present else {
        *reason = c"no Present sink".as_ptr();
        return false;
    };
    let sink = PresentSink {
        before_present,
        resize,
        user,
    };
    match HOOKS.install(Binding::Present(sink), platform, probe) {
        Ok(()) => true,
        Err(message) => {
            *reason = message;
            false
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_original(target: Address) -> Address {
    HOOKS
        .state()
        .hooks
        .iter()
        .find(|hook| hook.target == target)
        .map_or(target, |hook| hook.original)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_present_original(method: u32) -> Address {
    HOOKS
        .state()
        .hooks
        .iter()
        .find(|hook| hook.method == method)
        .map_or(0, |hook| hook.original)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_input_gate() -> Address {
    HOOKS.state().gate
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_set_input_observer(observer: Option<InputObserver>) {
    HOOKS.state().observer = observer;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_overlay_input_observer() -> Option<InputObserver> {
    HOOKS.state().observer
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_overlay_before_present(chain: *mut c_void, flags: u32) {
    const DXGI_PRESENT_TEST: u32 = 1;
    if flags & DXGI_PRESENT_TEST != 0 || IN_PRESENT.get() {
        return;
    }
    let Some(sink) = HOOKS.state().sink else {
        return;
    };
    IN_PRESENT.set(true);
    unsafe { (sink.before_present)(sink.user as *mut c_void, chain) };
    IN_PRESENT.set(false);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_overlay_before_resize(chain: *mut c_void) {
    let Some(sink) = HOOKS.state().sink else {
        return;
    };
    if let Some(callback) = sink.resize {
        unsafe { callback(sink.user as *mut c_void, chain) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::CStr;
    use std::ptr::{null, null_mut};

    thread_local! {
        static CALLS: RefCell<Vec<(char, Address)>> = const { RefCell::new(Vec::new()) };
    }

    unsafe extern "C" fn initialize() -> bool {
        true
    }

    unsafe extern "C" fn create(target: Address, _detour: Address, out: *mut Address) -> bool {
        CALLS.with_borrow_mut(|calls| calls.push(('c', target)));
        unsafe { out.write(target + 100) };
        true
    }

    unsafe extern "C" fn enable(target: Address) -> Reason {
        CALLS.with_borrow_mut(|calls| calls.push(('e', target)));
        if target == 22 {
            c"enable refused".as_ptr()
        } else {
            null()
        }
    }

    unsafe extern "C" fn disable(target: Address) {
        CALLS.with_borrow_mut(|calls| calls.push(('d', target)));
    }

    unsafe extern "C" fn input(registration: *mut c_void, add: Add, reason: *mut Reason) -> bool {
        unsafe { add(registration, 11, 1, 0, reason) && add(registration, 11, 1, 0, reason) }
    }

    unsafe extern "C" fn present(registration: *mut c_void, add: Add, reason: *mut Reason) -> bool {
        unsafe { add(registration, 21, 1, 6, reason) && add(registration, 22, 1, 7, reason) }
    }

    unsafe extern "C" fn callback(_user: *mut c_void, _chain: *mut c_void) {
        // A callback can reenter without waiting for the registry or routing itself twice.
        CALLS.with_borrow_mut(|calls| calls.push(('p', 0)));
        unsafe { rsf_anomaly_overlay_before_present(null_mut(), 0) };
    }

    #[test]
    fn refused_present_install_keeps_input_and_trampolines() {
        let platform = Platform {
            initialize,
            create,
            enable,
            disable,
        };
        assert!(HOOKS.install(Binding::Input(9), &platform, input).is_ok());
        assert!(HOOKS.install(Binding::Input(9), &platform, input).is_ok());
        let binding = Binding::Present(PresentSink {
            before_present: callback,
            resize: None,
            user: 0,
        });
        let refusal = HOOKS.install(binding, &platform, present).unwrap_err();
        assert_eq!(unsafe { CStr::from_ptr(refusal) }, c"enable refused");
        assert!(HOOKS.state().sink.is_none());
        assert_eq!(rsf_anomaly_overlay_input_gate(), 9);
        assert_eq!(rsf_anomaly_overlay_original(11), 111);
        assert_eq!(rsf_anomaly_overlay_present_original(6), 121);
        assert_eq!(rsf_anomaly_overlay_present_original(7), 122);
        assert_eq!(
            CALLS.with_borrow(|calls| calls.clone()),
            [
                ('c', 11),
                ('e', 11),
                ('c', 21),
                ('c', 22),
                ('e', 21),
                ('e', 22),
                ('d', 21),
                ('d', 22)
            ]
        );
        HOOKS.state().publish(binding, true);
        unsafe {
            rsf_anomaly_overlay_before_present(null_mut(), 1);
            rsf_anomaly_overlay_before_present(null_mut(), 0);
        }
        assert_eq!(CALLS.with_borrow(|calls| calls.len()), 9);
    }
}
