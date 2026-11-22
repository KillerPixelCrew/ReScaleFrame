// SPDX-License-Identifier: MIT
//! AMD Anti-Lag 2 driver calls for Direct3D 11 and 12.
//!
//! Behaviour, structure layouts and driver entry points follow AMD's MIT-licensed Anti-Lag 2.0
//! SDK headers `ffx_antilag2_dx11.h` and `ffx_antilag2_dx12.h` (GPUOpen AntiLag2-SDK revision
//! `390aa4a8c8655d0ae6e90079db2c85e103a96da3`). The licence is kept in
//! `ANTILAG2-LICENSE.txt` and ships with every package. The headers are inline C++ over driver
//! COM interfaces and offer no C ABI, so this module makes the same calls through the interface
//! tables. A native build no longer needs the SDK headers.
//!
//! Deviations from the headers, all on failure paths:
//! - An interface pointer that comes back null after a successful create call is a failure.
//! - A failed DX11 create call clears the request identifier the SDK leaves in the context
//!   before cleanup would call `Release` through it.
//!
//! The two 16-byte contexts keep the SDK `Context` layout because the pinned FSR DX12 swap chain
//! reads the DX12 context through a pointer stored in DXGI private data.

use std::ffi::c_void;
use std::ptr::{addr_of_mut, null, null_mut};
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::core::{GUID, s};

pub(super) type Hresult = i32;
const S_OK: Hresult = 0;
const S_FALSE: Hresult = 1;
const E_NOINTERFACE: Hresult = 0x8000_4002_u32 as i32;
const E_INVALIDARG: Hresult = 0x8007_0057_u32 as i32;
const E_HANDLE: Hresult = 0x8007_0006_u32 as i32;

const MODE_ENABLED: u32 = 1;
const MODE_DISABLED: u32 = 2;
// APIData_v2::Flags bit 5, signalEndOfFrameIdx.
const FLAG_END_OF_FRAME: u32 = 1 << 5;
// The DX11 interface request identifier the driver reads from the output slot.
const DX11_REQUEST_ID: usize = 0xbf38_0ebc_5ab4_d0a6;
const DX11_CONTROL: &[u8; 37] = b"delag_next_osd_supported_in_dxxp = 1\0";
const DX12_INTERFACE: GUID = GUID::from_u128(0x44085fbe_e839_40c5_bf38_0ebc5ab4d0a6);

/// The SDK `Context`: interface pointer, enabled flag and frame-rate limit.
#[repr(C)]
pub(super) struct Context {
    api: *mut c_void,
    enabled: bool,
    max_fps: u32,
}

const ZERO: Context = Context {
    api: null_mut(),
    enabled: false,
    max_fps: 0,
};

/// Both API contexts. The storage is never freed: DXGI can keep the DX12 address after retirement.
#[repr(C)]
pub(super) struct Sdk {
    dx11: Context,
    dx12: Context,
}

#[repr(C)]
struct DataV1 {
    size: u32,
    version: u32,
    mode: u32,
    control: *const u8,
    control_len: u32,
    max_fps: u32,
}

#[repr(C)]
struct DataV2 {
    size: u32,
    version: u32,
    flags: u32,
    frame_index: u64,
    reserved: [u64; 19],
}

const _: () = {
    assert!(size_of::<Context>() == 16);
    assert!(std::mem::offset_of!(Context, enabled) == 8);
    assert!(std::mem::offset_of!(Context, max_fps) == 12);
    assert!(size_of::<DataV1>() == 32);
    assert!(std::mem::offset_of!(DataV1, control) == 16);
    assert!(std::mem::offset_of!(DataV1, max_fps) == 28);
    assert!(size_of::<DataV2>() == 176);
    assert!(std::mem::offset_of!(DataV2, frame_index) == 16);
};

// IAmdExtAntiLagApi: IUnknown, then UpdateAntiLagState.
#[repr(C)]
struct Dx12Vtable {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    update: unsafe extern "system" fn(*mut c_void, *const c_void) -> Hresult,
}

// IAmdDxExtAntiLagApi: AddRef, Release, a virtual destructor, then UpdateAntiLagStateDx11.
#[repr(C)]
struct Dx11Vtable {
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    destructor: usize,
    update: unsafe extern "system" fn(*mut c_void, *const c_void) -> Hresult,
}

type CreateDx12 = unsafe extern "C" fn(*mut c_void, *const GUID, *mut *mut c_void) -> Hresult;
type CreateDx11 = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Hresult;

/// What the driver module offers: no module loaded, a module without the entry, or the entry.
#[derive(Clone, Copy)]
enum Driver<F> {
    NoModule,
    NoEntry,
    Entry(F),
}

unsafe fn vtable<T>(api: *mut c_void) -> &'static T {
    unsafe { &**api.cast::<*const T>() }
}

unsafe fn update12(api: *mut c_void, data: *const c_void) -> Hresult {
    unsafe { (vtable::<Dx12Vtable>(api).update)(api, data) }
}

unsafe fn update11(api: *mut c_void, data: *const c_void) -> Hresult {
    unsafe { (vtable::<Dx11Vtable>(api).update)(api, data) }
}

unsafe fn release12(api: *mut c_void) -> u32 {
    unsafe { (vtable::<Dx12Vtable>(api).release)(api) }
}

unsafe fn release11(api: *mut c_void) -> u32 {
    unsafe { (vtable::<Dx11Vtable>(api).release)(api) }
}

fn disabled_data(control: *const u8, control_len: u32) -> DataV1 {
    DataV1 {
        size: size_of::<DataV1>() as u32,
        version: 1,
        mode: MODE_DISABLED,
        control,
        control_len,
        max_fps: 0,
    }
}

unsafe fn deinitialize(context: *mut Context, release: unsafe fn(*mut c_void) -> u32) -> u32 {
    let mut count = 0;
    if !context.is_null() {
        unsafe {
            if !(*context).api.is_null() {
                count = release((*context).api);
                (*context).api = null_mut();
            }
            (*context).enabled = false;
        }
    }
    count
}

unsafe fn initialize_dx12_with(
    context: *mut Context,
    device: *mut c_void,
    driver: Driver<CreateDx12>,
) -> Hresult {
    if context.is_null() || device.is_null() || !unsafe { (*context).api.is_null() } {
        return E_INVALIDARG;
    }
    let create = match driver {
        Driver::NoModule => return E_HANDLE,
        Driver::NoEntry => return E_INVALIDARG,
        Driver::Entry(create) => create,
    };
    let mut hr = unsafe { create(device, &DX12_INTERFACE, addr_of_mut!((*context).api).cast()) };
    if hr == S_OK {
        let api = unsafe { (*context).api };
        // Anti-Lag 2 stays disabled until the first Update.
        hr = if api.is_null() {
            E_NOINTERFACE
        } else {
            let data = disabled_data(null(), 0);
            unsafe { update12(api, std::ptr::from_ref(&data).cast()) }
        };
    }
    if hr != S_OK {
        unsafe { deinitialize(context, release12) };
    }
    hr
}

unsafe fn initialize_dx11_with(context: *mut Context, driver: Driver<CreateDx11>) -> Hresult {
    if context.is_null() || !unsafe { (*context).api.is_null() } {
        return E_INVALIDARG;
    }
    let create = match driver {
        Driver::NoModule => return E_HANDLE,
        Driver::NoEntry => return E_INVALIDARG,
        Driver::Entry(create) => create,
    };
    let slot = unsafe { addr_of_mut!((*context).api) };
    // The driver reads the interface request identifier from the output slot.
    unsafe { slot.write(DX11_REQUEST_ID as *mut c_void) };
    let mut hr = unsafe { create(null_mut(), slot) };
    if hr != S_OK && unsafe { slot.read() } as usize == DX11_REQUEST_ID {
        unsafe { slot.write(null_mut()) };
    }
    if hr == S_OK {
        let api = unsafe { slot.read() };
        hr = if api.is_null() {
            E_NOINTERFACE
        } else {
            let data = disabled_data(null(), 0);
            unsafe { update11(api, std::ptr::from_ref(&data).cast()) }
        };
    }
    if hr != S_OK {
        unsafe { deinitialize(context, release11) };
    }
    hr
}

/// Pushes changed settings, then inserts the latency-reducing delay with a null update.
unsafe fn update_with(
    context: *mut Context,
    enabled: bool,
    max_fps: u32,
    control: Option<&'static [u8; 37]>,
    update: unsafe fn(*mut c_void, *const c_void) -> Hresult,
) -> Hresult {
    if context.is_null() || unsafe { (*context).api.is_null() } {
        return E_NOINTERFACE;
    }
    unsafe {
        let api = (*context).api;
        if (*context).enabled != enabled || (*context).max_fps != max_fps {
            (*context).enabled = enabled;
            (*context).max_fps = max_fps;
            let data = DataV1 {
                size: size_of::<DataV1>() as u32,
                version: 1,
                mode: if enabled { MODE_ENABLED } else { MODE_DISABLED },
                control: control.map_or(null(), |text| text.as_ptr()),
                control_len: control.map_or(0, |text| text.len() as u32 - 1),
                max_fps,
            };
            update(api, std::ptr::from_ref(&data).cast());
        }
        match update(api, null()) {
            S_OK | S_FALSE => S_OK,
            failure => failure,
        }
    }
}

unsafe fn mark_end_of_frame(context: *mut Context) -> Hresult {
    if context.is_null() || unsafe { (*context).api.is_null() } {
        return E_NOINTERFACE;
    }
    let data = DataV2 {
        size: size_of::<DataV2>() as u32,
        version: 2,
        flags: FLAG_END_OF_FRAME,
        frame_index: 0,
        reserved: [0; 19],
    };
    unsafe { update12((*context).api, std::ptr::from_ref(&data).cast()) }
}

fn driver_dx12() -> Driver<CreateDx12> {
    let Ok(module) = (unsafe { GetModuleHandleA(s!("amdxc64.dll")) }) else {
        return Driver::NoModule;
    };
    match unsafe { GetProcAddress(module, s!("AmdExtD3DCreateInterface")) } {
        Some(entry) => Driver::Entry(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateDx12>(entry)
        }),
        None => Driver::NoEntry,
    }
}

fn driver_dx11() -> Driver<CreateDx11> {
    // Only 64-bit is supported.
    let Ok(module) = (unsafe { GetModuleHandleA(s!("amdxx64.dll")) }) else {
        return Driver::NoModule;
    };
    match unsafe { GetProcAddress(module, s!("AmdDxExtCreate11")) } {
        Some(entry) => Driver::Entry(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateDx11>(entry)
        }),
        None => Driver::NoEntry,
    }
}

impl Sdk {
    /// Leaked, zeroed storage. The address stays valid for the process.
    pub(super) fn leak() -> *mut Sdk {
        Box::into_raw(Box::new(Sdk {
            dx11: ZERO,
            dx12: ZERO,
        }))
    }

    pub(super) unsafe fn initialize_dx11(this: *mut Sdk) -> Hresult {
        unsafe { initialize_dx11_with(addr_of_mut!((*this).dx11), driver_dx11()) }
    }

    pub(super) unsafe fn initialize_dx12(this: *mut Sdk, device: *mut c_void) -> Hresult {
        unsafe { initialize_dx12_with(addr_of_mut!((*this).dx12), device, driver_dx12()) }
    }

    pub(super) unsafe fn update_dx11(this: *mut Sdk, enabled: bool, max_fps: u32) -> Hresult {
        unsafe {
            update_with(
                addr_of_mut!((*this).dx11),
                enabled,
                max_fps,
                Some(DX11_CONTROL),
                update11,
            )
        }
    }

    pub(super) unsafe fn update_dx12(this: *mut Sdk, enabled: bool, max_fps: u32) -> Hresult {
        unsafe { update_with(addr_of_mut!((*this).dx12), enabled, max_fps, None, update12) }
    }

    /// The address the FSR swap chain reads through DXGI private data.
    pub(super) unsafe fn context_dx12(this: *mut Sdk) -> *mut c_void {
        unsafe { addr_of_mut!((*this).dx12) }.cast()
    }

    pub(super) unsafe fn mark_dx12(this: *mut Sdk) -> Hresult {
        unsafe { mark_end_of_frame(addr_of_mut!((*this).dx12)) }
    }

    /// Releases whichever interface is live, then zeroes both contexts.
    pub(super) unsafe fn shutdown(this: *mut Sdk) {
        unsafe {
            deinitialize(addr_of_mut!((*this).dx11), release11);
            deinitialize(addr_of_mut!((*this).dx12), release12);
            this.write(Sdk {
                dx11: ZERO,
                dx12: ZERO,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[repr(C)]
    struct Fake {
        vtable: *const c_void,
        updates: Vec<Option<Vec<u8>>>,
        releases: u32,
        fail_update: bool,
    }

    unsafe fn record(api: *mut c_void, data: *const c_void) -> Hresult {
        let fake = unsafe { &mut *api.cast::<Fake>() };
        fake.updates.push((!data.is_null()).then(|| {
            let size = unsafe { data.cast::<u32>().read() } as usize;
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec()
        }));
        if fake.fail_update {
            E_INVALIDARG
        } else if data.is_null() {
            S_FALSE
        } else {
            S_OK
        }
    }

    unsafe extern "system" fn fake_update(api: *mut c_void, data: *const c_void) -> Hresult {
        unsafe { record(api, data) }
    }

    unsafe extern "system" fn fake_release(api: *mut c_void) -> u32 {
        unsafe { (*api.cast::<Fake>()).releases += 1 };
        0
    }

    static VTABLE12: Dx12Vtable = Dx12Vtable {
        query_interface: 0,
        add_ref: 0,
        release: fake_release,
        update: fake_update,
    };
    static VTABLE11: Dx11Vtable = Dx11Vtable {
        add_ref: 0,
        release: fake_release,
        destructor: 0,
        update: fake_update,
    };

    fn fake(vtable: *const c_void) -> Box<Fake> {
        Box::new(Fake {
            vtable,
            updates: Vec::new(),
            releases: 0,
            fail_update: false,
        })
    }

    fn fake12() -> Box<Fake> {
        fake(std::ptr::from_ref(&VTABLE12).cast())
    }

    fn fake11() -> Box<Fake> {
        fake(std::ptr::from_ref(&VTABLE11).cast())
    }

    thread_local! {
        static OFFERED: Cell<*mut c_void> = const { Cell::new(null_mut()) };
        static CREATE_RESULT: Cell<Hresult> = const { Cell::new(S_OK) };
        static SEEN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    unsafe extern "C" fn create12(
        outer: *mut c_void,
        id: *const GUID,
        out: *mut *mut c_void,
    ) -> Hresult {
        assert_eq!(unsafe { *id }, DX12_INTERFACE);
        SEEN.with(|seen| seen.borrow_mut().push(outer as usize));
        if CREATE_RESULT.get() == S_OK {
            unsafe { out.write(OFFERED.get()) };
        }
        CREATE_RESULT.get()
    }

    unsafe extern "C" fn create11(outer: *mut c_void, out: *mut *mut c_void) -> Hresult {
        assert!(outer.is_null());
        // The driver reads the request identifier from the slot it fills.
        SEEN.with(|seen| seen.borrow_mut().push(unsafe { out.read() } as usize));
        if CREATE_RESULT.get() == S_OK {
            unsafe { out.write(OFFERED.get()) };
        }
        CREATE_RESULT.get()
    }

    fn offer(api: *mut Fake, result: Hresult) {
        OFFERED.set(api.cast());
        CREATE_RESULT.set(result);
        SEEN.with(|seen| seen.borrow_mut().clear());
    }

    fn data(update: &Option<Vec<u8>>) -> &[u8] {
        update.as_deref().expect("a populated update")
    }

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn dx12_initialization_disables_then_updates_follow_the_sdk() {
        let mut api = fake12();
        offer(&mut *api, S_OK);
        let mut context = ZERO;
        let device = 0x1000 as *mut c_void;
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::Entry(create12)) },
            S_OK
        );
        assert_eq!(SEEN.with(|seen| seen.borrow().clone()), [0x1000]);
        let first = data(&api.updates[0]);
        assert_eq!((word(first, 0), word(first, 4), word(first, 8)), (32, 1, 2));
        assert_eq!(word(first, 28), 0);
        // First update with the stored state unchanged is only the delay call.
        assert_eq!(
            unsafe { update_with(&mut context, false, 0, None, update12) },
            S_OK
        );
        assert_eq!(api.updates.len(), 2);
        assert!(api.updates[1].is_none());
        // A new setting pushes state, then delays. A repeat only delays.
        assert_eq!(
            unsafe { update_with(&mut context, true, 120, None, update12) },
            S_OK
        );
        assert_eq!(
            unsafe { update_with(&mut context, true, 120, None, update12) },
            S_OK
        );
        assert_eq!(api.updates.len(), 5);
        let pushed = data(&api.updates[2]);
        assert_eq!((word(pushed, 8), word(pushed, 28)), (1, 120));
        assert_eq!(pushed[16..24], [0; 8]);
        assert_eq!(word(pushed, 24), 0);
        assert!(api.updates[3].is_none() && api.updates[4].is_none());
        // Disabling returns the SDK disabled mode.
        unsafe { update_with(&mut context, false, 0, None, update12) };
        assert_eq!(word(data(&api.updates[5]), 8), 2);
    }

    #[test]
    fn dx12_end_of_frame_marker_signals_bit_five() {
        let mut api = fake12();
        offer(&mut *api, S_OK);
        let mut context = ZERO;
        unsafe {
            initialize_dx12_with(&mut context, 0x1000 as *mut c_void, Driver::Entry(create12))
        };
        assert_eq!(unsafe { mark_end_of_frame(&mut context) }, S_OK);
        let marker = data(api.updates.last().unwrap());
        assert_eq!(marker.len(), 176);
        assert_eq!(
            (word(marker, 0), word(marker, 4), word(marker, 8)),
            (176, 2, 0x20)
        );
        assert!(marker[16..].iter().all(|byte| *byte == 0));
        let mut empty = ZERO;
        assert_eq!(unsafe { mark_end_of_frame(&mut empty) }, E_NOINTERFACE);
    }

    #[test]
    fn dx12_initialization_refusals_and_cleanup() {
        let device = 0x1000 as *mut c_void;
        let mut context = ZERO;
        assert_eq!(
            unsafe { initialize_dx12_with(null_mut(), device, Driver::Entry(create12)) },
            E_INVALIDARG
        );
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, null_mut(), Driver::Entry(create12)) },
            E_INVALIDARG
        );
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::NoModule) },
            E_HANDLE
        );
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::NoEntry) },
            E_INVALIDARG
        );
        // A failed create call leaves the context empty. A null interface is a failure too.
        offer(null_mut(), E_NOINTERFACE);
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::Entry(create12)) },
            E_NOINTERFACE
        );
        offer(null_mut(), S_OK);
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::Entry(create12)) },
            E_NOINTERFACE
        );
        assert!(context.api.is_null());
        // A refused initial update releases the interface once.
        let mut api = fake12();
        api.fail_update = true;
        offer(&mut *api, S_OK);
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::Entry(create12)) },
            E_INVALIDARG
        );
        assert_eq!(api.releases, 1);
        assert!(context.api.is_null());
        // An initialized context refuses a second initialization.
        let mut live = fake12();
        offer(&mut *live, S_OK);
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::Entry(create12)) },
            S_OK
        );
        assert_eq!(
            unsafe { initialize_dx12_with(&mut context, device, Driver::Entry(create12)) },
            E_INVALIDARG
        );
        assert_eq!(live.releases, 0);
    }

    #[test]
    fn update_requires_an_interface_and_reports_driver_refusal() {
        let mut empty = ZERO;
        assert_eq!(
            unsafe { update_with(&mut empty, true, 60, None, update12) },
            E_NOINTERFACE
        );
        assert_eq!(
            unsafe { update_with(null_mut(), true, 60, None, update12) },
            E_NOINTERFACE
        );
        let mut api = fake12();
        let mut context = Context {
            api: (&mut *api as *mut Fake).cast(),
            ..ZERO
        };
        api.fail_update = true;
        assert_eq!(
            unsafe { update_with(&mut context, true, 60, None, update12) },
            E_INVALIDARG
        );
        // The settings were recorded before the refusal, as in the SDK.
        assert!(context.enabled && context.max_fps == 60);
    }

    #[test]
    fn dx11_uses_the_request_identifier_and_control_string() {
        let mut api = fake11();
        offer(&mut *api, S_OK);
        let mut context = ZERO;
        assert_eq!(
            unsafe { initialize_dx11_with(&mut context, Driver::Entry(create11)) },
            S_OK
        );
        assert_eq!(SEEN.with(|seen| seen.borrow().clone()), [DX11_REQUEST_ID]);
        let first = data(&api.updates[0]);
        assert_eq!((word(first, 8), word(first, 24)), (2, 0));
        assert_eq!(
            unsafe { update_with(&mut context, true, 60, Some(DX11_CONTROL), update11) },
            S_OK
        );
        let pushed = data(&api.updates[1]);
        assert_eq!(
            (word(pushed, 8), word(pushed, 24), word(pushed, 28)),
            (1, 36, 60)
        );
        assert_eq!(api.updates[2], None);
        assert_eq!(DX11_CONTROL.len() - 1, 36);
    }

    #[test]
    fn dx11_failures_never_release_through_the_request_identifier() {
        let mut context = ZERO;
        assert_eq!(
            unsafe { initialize_dx11_with(&mut context, Driver::NoModule) },
            E_HANDLE
        );
        assert_eq!(
            unsafe { initialize_dx11_with(&mut context, Driver::NoEntry) },
            E_INVALIDARG
        );
        offer(null_mut(), E_NOINTERFACE);
        assert_eq!(
            unsafe { initialize_dx11_with(&mut context, Driver::Entry(create11)) },
            E_NOINTERFACE
        );
        assert!(context.api.is_null());
        let mut api = fake11();
        api.fail_update = true;
        offer(&mut *api, S_OK);
        assert_eq!(
            unsafe { initialize_dx11_with(&mut context, Driver::Entry(create11)) },
            E_INVALIDARG
        );
        assert_eq!(api.releases, 1);
        assert!(context.api.is_null() && !context.enabled);
    }

    #[test]
    fn shutdown_releases_live_interfaces_and_zeroes_both_contexts() {
        let mut dx11 = fake11();
        let mut dx12 = fake12();
        let mut sdk = Sdk {
            dx11: Context {
                api: (&mut *dx11 as *mut Fake).cast(),
                enabled: true,
                max_fps: 30,
            },
            dx12: Context {
                api: (&mut *dx12 as *mut Fake).cast(),
                enabled: true,
                max_fps: 90,
            },
        };
        unsafe { Sdk::shutdown(&mut sdk) };
        assert_eq!((dx11.releases, dx12.releases), (1, 1));
        for context in [&sdk.dx11, &sdk.dx12] {
            assert!(context.api.is_null() && !context.enabled && context.max_fps == 0);
        }
        unsafe { Sdk::shutdown(&mut sdk) };
        assert_eq!((dx11.releases, dx12.releases), (1, 1));
        let sdk = Sdk::leak();
        assert_eq!(
            unsafe { Sdk::context_dx12(sdk) } as usize,
            sdk as usize + size_of::<Context>()
        );
        // No driver is loaded in the test process.
        assert_eq!(unsafe { Sdk::initialize_dx11(sdk) }, E_HANDLE);
        assert_eq!(
            unsafe { Sdk::initialize_dx12(sdk, 0x1000 as *mut c_void) },
            E_HANDLE
        );
    }
}
