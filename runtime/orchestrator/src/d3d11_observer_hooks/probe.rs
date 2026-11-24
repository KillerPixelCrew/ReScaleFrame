// SPDX-License-Identifier: MIT
use super::{BUFFER, LAYOUT, PRESENT, SLOT_COUNT, STAGE_BASE, Slot, TEXTURE, detours};
use crate::com_ref;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::ID3D11Device_Vtbl;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use windows::Win32::Graphics::Dxgi::{
    DXGI_SWAP_CHAIN_DESC, DXGI_USAGE, IDXGISwapChain_Vtbl, IDXGISwapChain1_Vtbl,
};
use windows::core::BOOL;
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetModuleHandleW,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_IMAGE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_GUARD, PAGE_NOACCESS,
    VirtualQuery,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW,
};

type Handle = *mut c_void;
type Table = *mut Handle;

const HARDWARE: u32 = 1;
const WARP: u32 = 5;
macro_rules! slot {
    ($table:ty, $method:ident) => {
        std::mem::offset_of!($table, $method) / size_of::<Handle>()
    };
}

const CREATE_BUFFER: usize = slot!(ID3D11Device_Vtbl, CreateBuffer);
const CREATE_TEXTURE: usize = slot!(ID3D11Device_Vtbl, CreateTexture2D);
const CREATE_LAYOUT: usize = slot!(ID3D11Device_Vtbl, CreateInputLayout);
const CREATE_SHADER: [usize; SLOT_COUNT - STAGE_BASE] = [
    slot!(ID3D11Device_Vtbl, CreateGeometryShader),
    slot!(ID3D11Device_Vtbl, CreateHullShader),
    slot!(ID3D11Device_Vtbl, CreateDomainShader),
    slot!(ID3D11Device_Vtbl, CreateComputeShader),
    slot!(ID3D11Device_Vtbl, CreateGeometryShaderWithStreamOutput),
    slot!(ID3D11Device_Vtbl, CreateVertexShader),
    slot!(ID3D11Device_Vtbl, CreatePixelShader),
];
const DXGI_PRESENT: usize = slot!(IDXGISwapChain_Vtbl, Present);
const DXGI_PRESENT1: usize = slot!(IDXGISwapChain1_Vtbl, Present1);
const DEVICE_SLOTS: usize = slot!(ID3D11Device_Vtbl, CreateComputeShader) + 1;
const CHAIN_SLOTS: usize = DXGI_PRESENT + 1;
const CHAIN1_SLOTS: usize = DXGI_PRESENT1 + 1;

#[repr(C)]
pub(super) struct Route {
    pub(super) table: Table,
    pub(super) index: usize,
    pub(super) hook: Handle,
}

#[repr(C)]
struct Description {
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    buffer_count: u32,
    sample_count: u32,
    feature_level: u32,
}

const DESCRIPTION: Description = Description {
    width: 8,
    height: 8,
    format: 28,  // DXGI_FORMAT_R8G8B8A8_UNORM
    usage: 0x20, // DXGI_USAGE_RENDER_TARGET_OUTPUT
    buffer_count: 1,
    sample_count: 1,
    feature_level: 0xb000, // D3D_FEATURE_LEVEL_11_0
};

#[repr(C)]
#[derive(Default)]
struct Attempt {
    context: Handle,
    chain: Handle,
    device: Handle,
    result: i32,
    returned: u32,
}

#[repr(C)]
struct Tables {
    device: Table,
    chain: Table,
    chain1: Table,
}

// Production creates the probe window, device and chain; unit tests call same-named fakes.
#[cfg(not(test))]
mod leaf {
    pub(super) use super::resident_vtable as vtable;
    pub(super) use super::rsf_observer11_probe_chain1 as query_chain1;
    pub(super) use super::{create, destroy_window, window};
}
#[cfg(test)]
use tests as leaf;

unsafe extern "C" {
    fn rsf_observer11_guard_probe_create(
        driver: u32,
        feature_level: u32,
        description: &DXGI_SWAP_CHAIN_DESC,
        out: &mut Attempt,
    );
    #[cfg_attr(test, allow(dead_code))]
    fn rsf_observer11_probe_chain1(chain: Handle, extended: &mut Handle, result: &mut i32) -> u32;
}

#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn create(
    window: Handle,
    driver: u32,
    description: &Description,
    out: &mut Attempt,
) {
    let mut desc = DXGI_SWAP_CHAIN_DESC {
        BufferCount: description.buffer_count,
        BufferUsage: DXGI_USAGE(description.usage),
        OutputWindow: HWND(window),
        Windowed: BOOL(1),
        ..Default::default()
    };
    desc.BufferDesc.Width = description.width;
    desc.BufferDesc.Height = description.height;
    desc.BufferDesc.Format = DXGI_FORMAT(description.format as i32);
    desc.SampleDesc.Count = description.sample_count;
    unsafe { rsf_observer11_guard_probe_create(driver, description.feature_level, &desc, out) };
}

#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn window(width: u32, height: u32) -> Handle {
    let instance = unsafe { GetModuleHandleW(null()) };
    let class = windows::core::w!("RsfObserverProbe");
    let title = windows::core::w!("");
    let description = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(DefWindowProcW),
        hInstance: instance,
        lpszClassName: class.0,
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&description);
        CreateWindowExW(
            0,
            class.0,
            title.0,
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            width as i32,
            height as i32,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    }
}

#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn destroy_window(window: Handle) {
    unsafe { DestroyWindow(window) };
}

// An object's private table can disappear when the last interface reference is released.
// A table must have a complete readable span inside a pinned executable image.
#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn resident_vtable(object: Handle, count: usize) -> Table {
    let table = unsafe { object.cast::<Table>().read() };
    let mut info = MEMORY_BASIC_INFORMATION::default();
    if table.is_null()
        || unsafe {
            VirtualQuery(
                table.cast(),
                &mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } != size_of::<MEMORY_BASIC_INFORMATION>()
        || info.Type != MEM_IMAGE
        || info.State != MEM_COMMIT
        || info.Protect & (PAGE_GUARD | PAGE_NOACCESS) != 0
        || info.Protect & 0xff == PAGE_EXECUTE
    {
        return null_mut();
    }
    let Some(offset) = table.addr().checked_sub(info.BaseAddress.addr()) else {
        return null_mut();
    };
    if info
        .RegionSize
        .checked_sub(offset)
        .is_none_or(|bytes| bytes / size_of::<Handle>() < count)
    {
        return null_mut();
    }
    let mut module = null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            table.cast(),
            &mut module,
        )
    } == 0
        || module != info.AllocationBase
    {
        return null_mut();
    }
    table
}

const _: () = {
    assert!(size_of::<Description>() == 28);
    assert!(size_of::<Attempt>() == 32);
    assert!(std::mem::offset_of!(Attempt, result) == 24);
    assert!(size_of::<Tables>() == 3 * size_of::<Handle>());
};

// The context, chain and device retire in the order used by the native probe.
struct Objects {
    _context: Option<IUnknown>,
    chain: Option<IUnknown>,
    device: Option<IUnknown>,
}

enum CreateFailure {
    DriverRefused,
    Exception,
}

impl Objects {
    fn create(window: Handle, driver: u32) -> Result<Self, CreateFailure> {
        let mut attempt = Attempt::default();
        unsafe { leaf::create(window, driver, &DESCRIPTION, &mut attempt) };
        let objects = unsafe {
            Self {
                _context: com_ref::owned(attempt.context),
                chain: com_ref::owned(attempt.chain),
                device: com_ref::owned(attempt.device),
            }
        };
        if attempt.returned == 0 {
            return Err(CreateFailure::Exception);
        }
        if attempt.result < 0 {
            return Err(CreateFailure::DriverRefused);
        }
        Ok(objects)
    }
}

fn probe(window: Handle) -> Option<Tables> {
    let objects = match Objects::create(window, HARDWARE) {
        Ok(objects) => objects,
        Err(CreateFailure::DriverRefused) => Objects::create(window, WARP).ok()?,
        Err(CreateFailure::Exception) => return None,
    };
    let device_object = objects.device.as_ref()?.as_raw();
    let chain_object = objects.chain.as_ref()?.as_raw();
    let device = unsafe { leaf::vtable(device_object, DEVICE_SLOTS) };
    let chain = unsafe { leaf::vtable(chain_object, CHAIN_SLOTS) };
    if device.is_null() || chain.is_null() {
        return None;
    }
    let (mut extended, mut result) = (null_mut(), 0);
    let returned = unsafe { leaf::query_chain1(chain_object, &mut extended, &mut result) };
    let extended = unsafe { com_ref::owned(extended) };
    if returned == 0 {
        return None;
    }
    let chain1 = extended
        .filter(|_| result >= 0)
        .map_or(null_mut(), |extended| unsafe {
            leaf::vtable(extended.as_raw(), CHAIN1_SLOTS)
        });
    Some(Tables {
        device,
        chain,
        chain1,
    })
}

pub(super) fn native_slots(slots: &mut [Slot; SLOT_COUNT]) -> Option<[Route; 2]> {
    let mut tables = Tables {
        device: null_mut(),
        chain: null_mut(),
        chain1: null_mut(),
    };
    if run(&mut tables) == 0 {
        return None;
    }
    let present = detours::hook_present as Handle;
    let routes = [
        Route {
            table: tables.chain,
            index: DXGI_PRESENT,
            hook: present,
        },
        Route {
            table: tables.chain1,
            index: DXGI_PRESENT1,
            hook: detours::hook_present1 as Handle,
        },
    ];
    slots[PRESENT] = Slot {
        address: unsafe { tables.chain.add(DXGI_PRESENT) },
        hook: present,
    };
    let shader_hooks = [
        detours::hook_create_geometry_shader as Handle,
        detours::hook_create_hull_shader as Handle,
        detours::hook_create_domain_shader as Handle,
        detours::hook_create_compute_shader as Handle,
        detours::hook_create_geometry_so as Handle,
        detours::hook_create_vertex_shader as Handle,
        detours::hook_create_pixel_shader as Handle,
    ];
    let device_hooks = [
        (
            TEXTURE,
            CREATE_TEXTURE,
            detours::hook_create_texture2d as Handle,
        ),
        (BUFFER, CREATE_BUFFER, detours::hook_create_buffer as Handle),
        (
            LAYOUT,
            CREATE_LAYOUT,
            detours::hook_create_input_layout as Handle,
        ),
    ];
    for (i, index, hook) in device_hooks.into_iter().chain(
        CREATE_SHADER
            .into_iter()
            .zip(shader_hooks)
            .enumerate()
            .map(|(i, (index, hook))| (STAGE_BASE + i, index, hook)),
    ) {
        slots[i] = Slot {
            address: unsafe { tables.device.add(index) },
            hook,
        };
    }
    Some(routes)
}

fn run(tables: &mut Tables) -> u32 {
    let window = unsafe { leaf::window(DESCRIPTION.width, DESCRIPTION.height) };
    if window.is_null() {
        return 0;
    }
    let result = probe(window);
    unsafe { leaf::destroy_window(window) };
    if let Some(result) = result {
        *tables = result;
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{Fake, add_ref, fake, raw};
    use std::cell::RefCell;
    use std::ptr::without_provenance_mut;

    struct Fixture {
        context: Box<Fake>,
        chain: Box<Fake>,
        device: Box<Fake>,
        extended: Box<Fake>,
        drivers: Vec<u32>,
        throw_in_hardware: bool,
        windows_destroyed: u32,
        resident_device: bool,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                context: fake(),
                chain: fake(),
                device: fake(),
                extended: fake(),
                drivers: Vec::new(),
                throw_in_hardware: false,
                windows_destroyed: 0,
                resident_device: true,
            }
        }

        fn refs(&self) -> [u32; 4] {
            [
                self.context.refs.get(),
                self.chain.refs.get(),
                self.device.refs.get(),
                self.extended.refs.get(),
            ]
        }
    }

    thread_local! {
        static FIXTURE: RefCell<Fixture> = RefCell::new(Fixture::new());
    }

    fn owned(fake: &Fake) -> Handle {
        let object = raw(fake);
        unsafe { add_ref(object.cast()) };
        object
    }

    pub(super) unsafe extern "C" fn window(width: u32, height: u32) -> Handle {
        assert_eq!((width, height), (8, 8));
        without_provenance_mut(1)
    }

    pub(super) unsafe extern "C" fn destroy_window(window: Handle) {
        assert_eq!(window.addr(), 1);
        FIXTURE.with_borrow_mut(|fixture| {
            assert_eq!(fixture.refs(), [1; 4]);
            fixture.windows_destroyed += 1;
        });
    }

    pub(super) unsafe extern "C" fn create(
        window: Handle,
        driver: u32,
        desc: &Description,
        out: &mut Attempt,
    ) {
        assert_eq!(window.addr(), 1);
        assert_eq!(
            (desc.width, desc.height, desc.format, desc.usage),
            (8, 8, 28, 0x20)
        );
        assert_eq!(
            (desc.buffer_count, desc.sample_count, desc.feature_level),
            (1, 1, 0xb000)
        );
        FIXTURE.with_borrow_mut(|fixture| {
            // The WARP retry must see every hardware reference released.
            assert_eq!(fixture.refs(), [1; 4]);
            fixture.drivers.push(driver);
            *out = Attempt {
                context: owned(&fixture.context),
                chain: owned(&fixture.chain),
                device: if driver == WARP {
                    owned(&fixture.device)
                } else {
                    null_mut()
                },
                result: if driver == WARP { 0 } else { -1 },
                returned: u32::from(driver == WARP || !fixture.throw_in_hardware),
            };
        });
    }

    pub(super) unsafe extern "C" fn query_chain1(
        chain: Handle,
        out: &mut Handle,
        result: &mut i32,
    ) -> u32 {
        FIXTURE.with_borrow(|fixture| {
            assert_eq!(chain, raw(&fixture.chain));
            *out = owned(&fixture.extended);
        });
        *result = 0;
        1
    }

    pub(super) unsafe extern "C" fn vtable(object: Handle, slot_count: usize) -> Table {
        FIXTURE.with_borrow(|fixture| {
            let (expected, resident) = if object == raw(&fixture.device) {
                (DEVICE_SLOTS, fixture.resident_device)
            } else if object == raw(&fixture.chain) {
                (CHAIN_SLOTS, true)
            } else {
                assert_eq!(object, raw(&fixture.extended));
                (CHAIN1_SLOTS, true)
            };
            assert_eq!(slot_count, expected);
            if resident { object.cast() } else { null_mut() }
        })
    }

    #[test]
    fn probe_refuses_unverified_device_table() {
        FIXTURE.with_borrow_mut(|fixture| {
            *fixture = Fixture::new();
            fixture.resident_device = false;
        });
        let mut tables = Tables {
            device: null_mut(),
            chain: null_mut(),
            chain1: null_mut(),
        };
        assert_eq!(run(&mut tables), 0);
        assert!(tables.device.is_null());
        FIXTURE.with_borrow(|fixture| {
            assert_eq!(fixture.windows_destroyed, 1);
            assert_eq!(fixture.refs(), [1; 4]);
        });
    }

    #[test]
    fn probe_releases_partial_hardware_results_before_retry_or_exception_return() {
        for interrupted in [false, true] {
            FIXTURE.with_borrow_mut(|fixture| {
                *fixture = Fixture::new();
                fixture.throw_in_hardware = interrupted;
            });
            let mut tables = Tables {
                device: null_mut(),
                chain: null_mut(),
                chain1: null_mut(),
            };
            assert_eq!(run(&mut tables), u32::from(!interrupted));
            FIXTURE.with_borrow(|fixture| {
                assert_eq!(fixture.windows_destroyed, 1);
                if interrupted {
                    assert_eq!(fixture.drivers.as_slice(), &[HARDWARE]);
                    assert!(
                        tables.device.is_null()
                            && tables.chain.is_null()
                            && tables.chain1.is_null()
                    );
                } else {
                    assert_eq!(fixture.drivers.as_slice(), &[HARDWARE, WARP]);
                    assert_eq!(tables.device, raw(&fixture.device).cast());
                    assert_eq!(tables.chain, raw(&fixture.chain).cast());
                    assert_eq!(tables.chain1, raw(&fixture.extended).cast());
                }
            });
        }
    }
}
