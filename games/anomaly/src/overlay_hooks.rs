// SPDX-License-Identifier: MIT
use std::cell::Cell;
use std::ffi::{c_char, c_void};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::native_hooks::{HookOps, Target, enable_batch, transaction};

const MAX_HOOKS: usize = 32;

type Reason = *const c_char;
type Address = usize;
type SinkCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type InputObserver = unsafe extern "C" fn(u8);
type Add = unsafe extern "C" fn(*mut c_void, Address, Address, u32, *mut Reason) -> bool;
type Probe = unsafe extern "C" fn(*mut c_void, Add, *mut Reason) -> bool;

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

    fn install(&self, binding: Binding, platform: &HookOps, probe: Probe) -> Result<(), Reason> {
        let Ok(_installation) = self.installation.try_lock() else {
            return Err(c"another overlay hook installation is in progress".as_ptr());
        };
        {
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
        }
        let mut reason = c"overlay hook discovery failed".as_ptr();
        let mut registration = Registration {
            hooks: self,
            pending: [Pending::default(); MAX_HOOKS],
            count: 0,
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

        let batch = &registration.pending[..registration.count];
        if batch.is_empty() {
            return Err(c"no overlay hook targets found".as_ptr());
        }
        self.state()
            .hooks
            .try_reserve_exact(batch.len())
            .map_err(|_| c"overlay hook table allocation failed".as_ptr())?;
        let _transaction = transaction();
        if !unsafe { (platform.initialize)() } {
            return Err(c"MinHook initialization failed".as_ptr());
        }
        let mut targets = [std::ptr::null_mut(); MAX_HOOKS];
        let mut originals = [std::ptr::null_mut(); MAX_HOOKS];
        let mut sites = [Target::default(); MAX_HOOKS];
        for (i, hook) in batch.iter().enumerate() {
            targets[i] = std::ptr::with_exposed_provenance_mut(hook.target);
            sites[i] = Target {
                address: targets[i],
                replacement: std::ptr::with_exposed_provenance_mut(hook.detour),
                original: &mut originals[i],
            };
        }
        if !unsafe { platform.create_unqueued(&sites[..batch.len()]) } {
            return Err(c"MinHook refused to create a hook".as_ptr());
        }
        // Publish only after the full batch exists, before any detour can run.
        {
            let mut state = self.state();
            state
                .hooks
                .extend(batch.iter().zip(originals).map(|(hook, original)| Hook {
                    target: hook.target,
                    original: original.expose_provenance(),
                    method: hook.method,
                }));
            state.publish(binding, true);
        }
        if !enable_batch(
            &targets[..batch.len()],
            platform.queue,
            platform.apply,
            || {
                self.state().publish(binding, false);
            },
        ) {
            return Err(c"MinHook refused to enable the overlay hook batch".as_ptr());
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
    pending: [Pending; MAX_HOOKS],
    count: usize,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    target: Address,
    detour: Address,
    method: u32,
}

unsafe extern "C" fn add(
    registration: *mut c_void,
    target: Address,
    detour: Address,
    method: u32,
    reason: *mut Reason,
) -> bool {
    let registration = unsafe { &mut *registration.cast::<Registration<'_>>() };
    let result = {
        let state = registration.hooks.state();
        if target == 0 || detour == 0 {
            Err(c"a hook target resolved to null")
        } else {
            let existing = state
                .hooks
                .iter()
                .map(|hook| (hook.target, hook.method))
                .chain(
                    registration.pending[..registration.count]
                        .iter()
                        .map(|hook| (hook.target, hook.method)),
                )
                .find(|&(address, _)| address == target);
            match existing {
                Some((_, mapped)) if mapped == method => return true,
                Some(_) => Err(c"two different methods share one code address"),
                None if state.hooks.len() + registration.count == MAX_HOOKS => {
                    Err(c"hook table is full")
                }
                None => Ok(()),
            }
        }
    };
    if let Err(message) = result {
        unsafe { reason.write(message.as_ptr()) };
        return false;
    }
    registration.pending[registration.count] = Pending {
        target,
        detour,
        method,
    };
    registration.count += 1;
    true
}

static HOOKS: Hooks = Hooks::new();
thread_local! {
    static IN_PRESENT: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_overlay_install_input(
    gate: Address,
    platform: &HookOps,
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
    platform: &HookOps,
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
    use std::ptr::null_mut;

    thread_local! {
        static CALLS: RefCell<Vec<(char, Address)>> = const { RefCell::new(Vec::new()) };
        static REFUSE_CREATE: Cell<Address> = const { Cell::new(0) };
    }

    unsafe extern "C" fn initialize() -> bool {
        true
    }

    unsafe extern "C" fn create(
        target: *mut c_void,
        _detour: *mut c_void,
        out: *mut *mut c_void,
    ) -> bool {
        CALLS.with_borrow_mut(|calls| calls.push(('c', target.addr())));
        if REFUSE_CREATE.get() == target.addr() {
            return false;
        }
        unsafe { out.write(target.wrapping_byte_add(100)) };
        true
    }

    unsafe extern "C" fn remove_unqueued(target: *mut c_void) {
        CALLS.with_borrow_mut(|calls| calls.push(('r', target.addr())));
    }

    unsafe extern "C" fn queue(target: *mut c_void, enable: bool) -> bool {
        CALLS.with_borrow_mut(|calls| calls.push((if enable { 'e' } else { 'd' }, target.addr())));
        true
    }

    unsafe extern "C" fn apply() -> bool {
        CALLS.with_borrow_mut(|calls| {
            calls.push(('a', 0));
            calls
                .iter()
                .filter(|(operation, _)| *operation == 'a')
                .count()
                != 2
        })
    }

    unsafe extern "C" fn input(registration: *mut c_void, add: Add, reason: *mut Reason) -> bool {
        unsafe { add(registration, 11, 1, 0, reason) && add(registration, 11, 1, 0, reason) }
    }

    unsafe extern "C" fn present(registration: *mut c_void, add: Add, reason: *mut Reason) -> bool {
        unsafe { add(registration, 21, 1, 6, reason) && add(registration, 22, 1, 7, reason) }
    }

    unsafe extern "C" fn aliased(registration: *mut c_void, add: Add, reason: *mut Reason) -> bool {
        unsafe { add(registration, 21, 1, 6, reason) && add(registration, 21, 1, 7, reason) }
    }

    unsafe extern "C" fn callback(_user: *mut c_void, _chain: *mut c_void) {
        // A callback can reenter without waiting for the registry or routing itself twice.
        CALLS.with_borrow_mut(|calls| calls.push(('p', 0)));
        unsafe { rsf_anomaly_overlay_before_present(null_mut(), 0) };
    }

    const PLATFORM: HookOps = HookOps {
        initialize,
        create,
        remove_unqueued,
        queue,
        apply,
    };

    const PRESENT: Binding = Binding::Present(PresentSink {
        before_present: callback,
        resize: None,
        user: 0,
    });

    #[test]
    fn a_failed_probe_creates_nothing_and_a_failed_create_removes_unqueued_hooks() {
        CALLS.with_borrow_mut(Vec::clear);
        let hooks = Hooks::new();
        assert!(hooks.install(PRESENT, &PLATFORM, aliased).is_err());
        assert!(CALLS.with_borrow(Vec::is_empty));
        assert!(hooks.state().hooks.is_empty());

        let hooks = Hooks::new();
        REFUSE_CREATE.set(22);
        assert!(hooks.install(PRESENT, &PLATFORM, present).is_err());
        REFUSE_CREATE.set(0);
        assert_eq!(
            CALLS.with_borrow(|calls| calls.clone()),
            [('c', 21), ('c', 22), ('r', 21)]
        );
        assert!(hooks.state().hooks.is_empty());
        assert!(hooks.state().sink.is_none());
    }

    #[test]
    fn refused_present_install_keeps_input_and_trampolines() {
        CALLS.with_borrow_mut(Vec::clear);
        assert!(HOOKS.install(Binding::Input(9), &PLATFORM, input).is_ok());
        assert!(HOOKS.install(Binding::Input(9), &PLATFORM, input).is_ok());
        let refusal = HOOKS.install(PRESENT, &PLATFORM, present).unwrap_err();
        assert_eq!(
            unsafe { CStr::from_ptr(refusal) },
            c"MinHook refused to enable the overlay hook batch"
        );
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
                ('a', 0),
                ('c', 21),
                ('c', 22),
                ('e', 21),
                ('e', 22),
                ('a', 0),
                ('d', 21),
                ('d', 22),
                ('a', 0)
            ]
        );
        HOOKS.state().publish(PRESENT, true);
        unsafe {
            rsf_anomaly_overlay_before_present(null_mut(), 1);
            rsf_anomaly_overlay_before_present(null_mut(), 0);
        }
        assert_eq!(CALLS.with_borrow(|calls| calls.len()), 12);
    }
}
