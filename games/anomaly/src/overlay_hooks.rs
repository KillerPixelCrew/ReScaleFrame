// SPDX-License-Identifier: MIT
// The overlay calls these installs directly; its unit tests swap in fakes, so under test only the fixtures reach them.
#![cfg_attr(test, allow(dead_code))]
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use windows_sys::Win32::Foundation::{HWND, MAX_PATH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP};
use windows_sys::core::{GUID, HRESULT, w};

use crate::input::gate::{self, DeviceCalls, Gate};
use crate::input::{KEYBOARD, MOUSE};
use crate::lease::Lease;
use crate::native_hooks::{HookOps, Target, enable_batch, transaction};
use crate::overlay::rsf_anomaly_overlay_capture_bits;
use crate::overlay_native::{self as native, ChainObjects, Object};
use crate::runtime_module::Module;
use std::mem::transmute;

const MAX_HOOKS: usize = 32;

type Reason = *const c_char;
type Address = usize;
type SinkCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type InputObserver = unsafe extern "C" fn(u8);
type Refusal = &'static CStr;

// The DXGI detours use these IDs to look up their trampolines.
#[derive(Clone, Copy)]
#[repr(u32)]
enum Method {
    CreateDevice = 0,
    Acquire,
    Unacquire,
    GetState,
    GetData,
    Cooperative,
    Present,
    Present1,
    ResizeBuffers,
    ResizeTarget,
}

const SLOT_CREATE_DEVICE: usize = 3;
const SLOT_ACQUIRE: usize = 7;
const SLOT_UNACQUIRE: usize = 8;
const SLOT_GET_STATE: usize = 9;
const SLOT_GET_DATA: usize = 10;
const SLOT_COOPERATIVE: usize = 13;
const SLOT_PRESENT: usize = 8;
const SLOT_RESIZE_BUFFERS: usize = 13;
const SLOT_RESIZE_TARGET: usize = 14;
const SLOT_PRESENT1: usize = 22;

const IID_DIRECT_INPUT8_A: GUID = GUID::from_u128(0xbf798030_483a_4da2_aa99_5d64ed369700);
const IID_DIRECT_INPUT8_W: GUID = GUID::from_u128(0xbf798031_483a_4da2_aa99_5d64ed369700);
const GUID_SYS_MOUSE: GUID = GUID::from_u128(0x6f1d2b60_d5a0_11cf_bfc7_444553540000);
const GUID_SYS_KEYBOARD: GUID = GUID::from_u128(0x6f1d2b61_d5a0_11cf_bfc7_444553540000);

fn probe_input(registration: &mut Registration<'_>) -> Result<Module, Refusal> {
    const SUFFIX: &str = "\\dinput8.dll\0";
    let mut path = [0u16; MAX_PATH as usize];
    let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), MAX_PATH) } as usize;
    if length == 0 || length > path.len() - SUFFIX.len() {
        return Err(c"System32 directory unavailable");
    }
    for (unit, value) in path[length..].iter_mut().zip(SUFFIX.encode_utf16()) {
        *unit = value;
    }
    // The carrier forwards to the same absolute system path.
    let module = unsafe { Module::system(path.as_ptr()) }
        .ok_or(c"system dinput8.dll could not be loaded")?;
    let create = module
        .symbol(c"DirectInput8Create")
        .ok_or(c"system DirectInput8Create missing")?;

    let mut devices = 0;
    for iid in [&IID_DIRECT_INPUT8_A, &IID_DIRECT_INPUT8_W] {
        let input = unsafe { native::create_input(create, iid, 0x0800) };
        let Some(input) = Lease::new(input, native::release) else {
            continue;
        };
        let create_device = unsafe { native::slot(input.as_ptr(), SLOT_CREATE_DEVICE) };
        registration.add(
            create_device,
            hook_create_device as *const () as Address,
            Method::CreateDevice,
        )?;
        for guid in [&GUID_SYS_MOUSE, &GUID_SYS_KEYBOARD] {
            let device = unsafe { native::create_device(input.as_ptr(), guid) };
            let Some(device) = Lease::new(device, native::release) else {
                continue;
            };
            for (slot, detour, method) in [
                (
                    SLOT_ACQUIRE,
                    hook_acquire as *const () as Address,
                    Method::Acquire,
                ),
                (
                    SLOT_UNACQUIRE,
                    hook_unacquire as *const () as Address,
                    Method::Unacquire,
                ),
                (
                    SLOT_GET_STATE,
                    hook_get_state as *const () as Address,
                    Method::GetState,
                ),
                (
                    SLOT_GET_DATA,
                    hook_get_data as *const () as Address,
                    Method::GetData,
                ),
                (
                    SLOT_COOPERATIVE,
                    hook_cooperative as *const () as Address,
                    Method::Cooperative,
                ),
            ] {
                registration.add(
                    unsafe { native::slot(device.as_ptr(), slot) },
                    detour,
                    method,
                )?;
            }
            devices += 1;
        }
    }
    if devices == 0 {
        return Err(c"no DirectInput mouse or keyboard device could be probed");
    }
    Ok(module)
}

unsafe extern "C" fn destroy_dummy_window(window: Object) {
    unsafe { DestroyWindow(window) };
}

fn probe_present(registration: &mut Registration<'_>) -> Result<Module, Refusal> {
    const EXTENT: i32 = 8;
    let module = unsafe { Module::system(w!("d3d11.dll")) }.ok_or(c"d3d11.dll unavailable")?;
    let create = module
        .symbol(c"D3D11CreateDeviceAndSwapChain")
        .ok_or(c"d3d11.dll unavailable")?;
    let window = unsafe {
        CreateWindowExW(
            0,
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            EXTENT,
            EXTENT,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    let window = Lease::new(window, destroy_dummy_window).ok_or(c"dummy window creation failed")?;
    let mut objects = ChainObjects::default();
    let result =
        unsafe { native::create_warp_chain(create, window.as_ptr(), EXTENT as u32, &mut objects) };
    // Release all returned objects in reverse order before destroying the window, even on failure.
    let _device = Lease::new(objects.device, native::release);
    let _context = Lease::new(objects.context, native::release);
    let chain = Lease::new(objects.chain, native::release);
    const CHAIN_FAILED: Refusal = c"dummy swap chain creation failed";
    if result < 0 {
        return Err(CHAIN_FAILED);
    }
    let chain = chain.ok_or(CHAIN_FAILED)?;

    let chain1 = unsafe { native::query_chain1(chain.as_ptr()) };
    let present1 = Lease::new(chain1, native::release).map_or(0, |chain1| unsafe {
        native::slot(chain1.as_ptr(), SLOT_PRESENT1)
    });
    for (slot, detour, method) in [
        (
            SLOT_PRESENT,
            hook_present as *const () as Address,
            Method::Present,
        ),
        (
            SLOT_RESIZE_BUFFERS,
            hook_resize_buffers as *const () as Address,
            Method::ResizeBuffers,
        ),
        (
            SLOT_RESIZE_TARGET,
            hook_resize_target as *const () as Address,
            Method::ResizeTarget,
        ),
    ] {
        registration.add(
            unsafe { native::slot(chain.as_ptr(), slot) },
            detour,
            method,
        )?;
    }
    if present1 != 0 {
        registration.add(
            present1,
            hook_present1 as *const () as Address,
            Method::Present1,
        )?;
    }
    Ok(module)
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
    // Once activation can reach a trampoline, keep its source library mapped.
    modules: [Option<Module>; 2],
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
                modules: [None, None],
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

    fn install(
        &self,
        binding: Binding,
        platform: &HookOps,
        probe: impl FnOnce(&mut Registration<'_>) -> Result<Option<Module>, Refusal>,
    ) -> Result<(), Refusal> {
        let Ok(_installation) = self.installation.try_lock() else {
            return Err(c"another overlay hook installation is in progress");
        };
        {
            let mut state = self.state();
            match binding {
                Binding::Input(gate) if state.input_attempted => {
                    return if state.gate == gate {
                        Ok(())
                    } else if state.gate == 0 {
                        Err(c"input hook installation already failed")
                    } else {
                        Err(c"input hooks already own a different gate")
                    };
                }
                Binding::Input(_) => state.input_attempted = true,
                Binding::Present(_) if state.present_attempted => {
                    return Err(c"Present hook installation already attempted");
                }
                Binding::Present(_) => state.present_attempted = true,
            }
        }
        let mut registration = Registration {
            hooks: self,
            pending: [Pending::default(); MAX_HOOKS],
            count: 0,
        };
        let module = probe(&mut registration)?;

        let batch = &registration.pending[..registration.count];
        if batch.is_empty() {
            return Err(c"no overlay hook targets found");
        }
        self.state()
            .hooks
            .try_reserve_exact(batch.len())
            .map_err(|_| c"overlay hook table allocation failed")?;
        let _transaction = transaction();
        if !unsafe { (platform.initialize.unwrap())() } {
            return Err(c"MinHook initialization failed");
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
            return Err(c"MinHook refused to create a hook");
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
            state.modules[usize::from(matches!(binding, Binding::Present(_)))] = module;
            state.publish(binding, true);
        }
        if !enable_batch(
            &targets[..batch.len()],
            platform.queue.unwrap(),
            platform.apply.unwrap(),
            || {
                self.state().publish(binding, false);
            },
        ) {
            return Err(c"MinHook refused to enable the overlay hook batch");
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

impl Registration<'_> {
    fn add(&mut self, target: Address, detour: Address, method: Method) -> Result<(), Refusal> {
        if target == 0 || detour == 0 {
            return Err(c"a hook target resolved to null");
        }
        let method = method as u32;
        {
            let state = self.hooks.state();
            let existing = state
                .hooks
                .iter()
                .map(|hook| (hook.target, hook.method))
                .chain(
                    self.pending[..self.count]
                        .iter()
                        .map(|hook| (hook.target, hook.method)),
                )
                .find(|&(address, _)| address == target);
            match existing {
                Some((_, mapped)) if mapped == method => return Ok(()),
                Some(_) => return Err(c"two different methods share one code address"),
                None if state.hooks.len() + self.count == MAX_HOOKS => {
                    return Err(c"hook table is full");
                }
                None => (),
            }
        }
        self.pending[self.count] = Pending {
            target,
            detour,
            method,
        };
        self.count += 1;
        Ok(())
    }
}

static HOOKS: Hooks = Hooks::new();
thread_local! {
    static IN_PRESENT: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn install_input(platform: &HookOps, reason: &mut Reason) -> bool {
    static GATE: LazyLock<Gate> = LazyLock::new(|| Gate::new(rsf_anomaly_overlay_capture_bits));
    let gate = std::ptr::from_ref(&*GATE).expose_provenance();
    match HOOKS.install(Binding::Input(gate), platform, |registration| {
        probe_input(registration).map(Some)
    }) {
        Ok(()) => true,
        Err(message) => {
            *reason = message.as_ptr();
            false
        }
    }
}

pub(super) fn install_present(
    before_present: SinkCallback,
    resize: SinkCallback,
    platform: &HookOps,
    reason: &mut Reason,
) -> bool {
    let sink = PresentSink {
        before_present,
        resize: Some(resize),
        user: 0,
    };
    match HOOKS.install(Binding::Present(sink), platform, |registration| {
        probe_present(registration).map(Some)
    }) {
        Ok(()) => true,
        Err(message) => {
            *reason = message.as_ptr();
            false
        }
    }
}

extern "C" fn rsf_anomaly_overlay_original(target: Address) -> Address {
    HOOKS
        .state()
        .hooks
        .iter()
        .find(|hook| hook.target == target)
        .map_or(target, |hook| hook.original)
}

extern "C" fn rsf_anomaly_overlay_present_original(method: u32) -> Address {
    HOOKS
        .state()
        .hooks
        .iter()
        .find(|hook| hook.method == method)
        .map_or(0, |hook| hook.original)
}

extern "C" fn rsf_anomaly_overlay_input_gate() -> Address {
    HOOKS.state().gate
}

pub(super) fn rsf_anomaly_overlay_set_input_observer(observer: Option<InputObserver>) {
    HOOKS.state().observer = observer;
}

extern "C" fn rsf_anomaly_overlay_input_observer() -> Option<InputObserver> {
    HOOKS.state().observer
}

// Each ANSI or Unicode DirectInput class reaches the original from its own vtable.
unsafe fn real_method(object: Object, slot: usize) -> Address {
    rsf_anomaly_overlay_original(unsafe { native::slot(object, slot) })
}

unsafe fn device_calls(device: Object) -> DeviceCalls {
    unsafe {
        DeviceCalls {
            acquire: transmute::<Address, unsafe extern "system" fn(Object) -> HRESULT>(
                real_method(device, SLOT_ACQUIRE),
            ),
            unacquire: transmute::<Address, unsafe extern "system" fn(Object) -> HRESULT>(
                real_method(device, SLOT_UNACQUIRE),
            ),
            get_state: transmute::<
                Address,
                unsafe extern "system" fn(Object, u32, Object) -> HRESULT,
            >(real_method(device, SLOT_GET_STATE)),
            get_data: transmute::<
                Address,
                unsafe extern "system" fn(Object, u32, *mut u8, *mut u32, u32) -> HRESULT,
            >(real_method(device, SLOT_GET_DATA)),
        }
    }
}

pub(super) fn active_gate() -> Option<&'static Gate> {
    // The adapter publishes one gate for the process before enabling any input hook.
    unsafe { std::ptr::with_exposed_provenance::<Gate>(rsf_anomaly_overlay_input_gate()).as_ref() }
}

fn device_kind(guid: &GUID) -> Option<u8> {
    if guid.data2 != GUID_SYS_MOUSE.data2
        || guid.data3 != GUID_SYS_MOUSE.data3
        || guid.data4 != GUID_SYS_MOUSE.data4
    {
        return None;
    }
    match guid.data1 {
        0x6f1d2b60 | 0x6f1d2b80 | 0x6f1d2b81 => Some(MOUSE),
        0x6f1d2b61 | 0x6f1d2b82 | 0x6f1d2b83 => Some(KEYBOARD),
        _ => None,
    }
}

unsafe extern "system" fn hook_create_device(
    input: Object,
    guid: *const GUID,
    device: *mut Object,
    outer: Object,
) -> HRESULT {
    let original: native::CreateDevice =
        unsafe { transmute(real_method(input, SLOT_CREATE_DEVICE)) };
    let result = unsafe { original(input, guid, device, outer) };
    if result >= 0
        && !device.is_null()
        && let (Some(gate), Some(kind)) = (
            active_gate(),
            unsafe { guid.as_ref() }.and_then(device_kind),
        )
    {
        gate::gate_add(gate, unsafe { device.read() }, kind);
    }
    result
}

unsafe extern "system" fn hook_acquire(device: Object) -> HRESULT {
    let calls = unsafe { device_calls(device) };
    match active_gate() {
        Some(gate) => unsafe { gate::gate_acquire(gate, device, &calls) },
        None => unsafe { (calls.acquire)(device) },
    }
}

unsafe extern "system" fn hook_unacquire(device: Object) -> HRESULT {
    let calls = unsafe { device_calls(device) };
    match active_gate() {
        Some(gate) => unsafe { gate::gate_unacquire(gate, device, &calls) },
        None => unsafe { (calls.unacquire)(device) },
    }
}

unsafe extern "system" fn hook_get_state(device: Object, size: u32, data: Object) -> HRESULT {
    let calls = unsafe { device_calls(device) };
    match active_gate() {
        Some(gate) => unsafe { gate::gate_get_state(gate, device, &calls, size, data) },
        None => unsafe { (calls.get_state)(device, size, data) },
    }
}

unsafe extern "system" fn hook_get_data(
    device: Object,
    size: u32,
    data: *mut u8,
    count: *mut u32,
    flags: u32,
) -> HRESULT {
    let calls = unsafe { device_calls(device) };
    match active_gate() {
        Some(gate) => unsafe {
            gate::gate_get_data(
                gate,
                device,
                &calls,
                size,
                data,
                count,
                flags,
                Some(rsf_anomaly_overlay_input_observer),
            )
        },
        None => unsafe { (calls.get_data)(device, size, data, count, flags) },
    }
}

unsafe extern "system" fn hook_cooperative(device: Object, window: HWND, flags: u32) -> HRESULT {
    let original: unsafe extern "system" fn(Object, HWND, u32) -> HRESULT =
        unsafe { transmute(real_method(device, SLOT_COOPERATIVE)) };
    let result = unsafe { original(device, window, flags) };
    if result >= 0
        && let Some(gate) = active_gate()
    {
        gate::gate_cooperative_level(gate, device, flags);
    }
    result
}

unsafe extern "system" fn hook_present(chain: Object, sync: u32, flags: u32) -> HRESULT {
    let original: unsafe extern "system" fn(Object, u32, u32) -> HRESULT =
        unsafe { transmute(rsf_anomaly_overlay_present_original(Method::Present as u32)) };
    unsafe {
        rsf_anomaly_overlay_before_present(chain, flags);
        original(chain, sync, flags)
    }
}

unsafe extern "system" fn hook_present1(
    chain: Object,
    sync: u32,
    flags: u32,
    parameters: *const c_void,
) -> HRESULT {
    let original: unsafe extern "system" fn(Object, u32, u32, *const c_void) -> HRESULT = unsafe {
        transmute(rsf_anomaly_overlay_present_original(
            Method::Present1 as u32,
        ))
    };
    unsafe {
        rsf_anomaly_overlay_before_present(chain, flags);
        original(chain, sync, flags, parameters)
    }
}

unsafe extern "system" fn hook_resize_buffers(
    chain: Object,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
) -> HRESULT {
    let original: unsafe extern "system" fn(Object, u32, u32, u32, u32, u32) -> HRESULT = unsafe {
        transmute(rsf_anomaly_overlay_present_original(
            Method::ResizeBuffers as u32,
        ))
    };
    unsafe {
        rsf_anomaly_overlay_before_resize(chain);
        original(chain, count, width, height, format, flags)
    }
}

unsafe extern "system" fn hook_resize_target(chain: Object, mode: *const c_void) -> HRESULT {
    let original: unsafe extern "system" fn(Object, *const c_void) -> HRESULT = unsafe {
        transmute(rsf_anomaly_overlay_present_original(
            Method::ResizeTarget as u32,
        ))
    };
    unsafe {
        rsf_anomaly_overlay_before_resize(chain);
        original(chain, mode)
    }
}

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

    fn input(registration: &mut Registration<'_>) -> Result<Option<Module>, Refusal> {
        registration.add(11, 1, Method::CreateDevice)?;
        registration.add(11, 1, Method::CreateDevice)?;
        Ok(None)
    }

    fn present(registration: &mut Registration<'_>) -> Result<Option<Module>, Refusal> {
        registration.add(21, 1, Method::Present)?;
        registration.add(22, 1, Method::Present1)?;
        Ok(None)
    }

    fn aliased(registration: &mut Registration<'_>) -> Result<Option<Module>, Refusal> {
        registration.add(21, 1, Method::Present)?;
        registration.add(21, 1, Method::Present1)?;
        Ok(None)
    }

    unsafe extern "C" fn callback(_user: *mut c_void, _chain: *mut c_void) {
        // A callback can reenter without waiting for the registry or routing itself twice.
        CALLS.with_borrow_mut(|calls| calls.push(('p', 0)));
        unsafe { rsf_anomaly_overlay_before_present(null_mut(), 0) };
    }

    const PLATFORM: HookOps = HookOps {
        initialize: Some(initialize),
        create: Some(create),
        remove_unqueued: Some(remove_unqueued),
        queue: Some(queue),
        apply: Some(apply),
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
        assert_eq!(refusal, c"MinHook refused to enable the overlay hook batch");
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
