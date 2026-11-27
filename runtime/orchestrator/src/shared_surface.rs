// SPDX-License-Identifier: MIT
use crate::com_ref;
use crate::sr_session::has_capacity;
use crate::sys::rsf_backend_log_fn;
use crate::{allocate_handle, owned_win32_handle};
use std::ffi::{CString, c_void};
use std::fmt;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use windows::Win32::Foundation::{GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
    D3D11_FENCE_FLAG_SHARED, D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device, ID3D11Device5, ID3D11Fence,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Fence, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIResource1,
};
use windows::core::IUnknown;
use windows::core::{Interface, PCWSTR};

type Handle = *mut c_void;
type Log = rsf_backend_log_fn;
type Create = unsafe extern "C" fn(Handle, *mut Handle) -> i32;
type Open = unsafe extern "C" fn(Handle, Handle, *mut Handle) -> i32;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const CREATE_FAILED: i32 = -3;
const NO_HANDLE: i32 = -4;
const OPEN_FAILED: i32 = -5;
pub(super) const ABI_VERSION: u32 = 2;

// rsf_shared_surface_setup of rescaleframe/shared_surface.h.
#[repr(C)]
pub(super) struct SurfaceSetup {
    pub(super) struct_size: u32,
    pub(super) abi_version: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) format: u32,
    pub(super) render_target: u32,
    pub(super) log: Log,
    pub(super) log_user: Handle,
    pub(super) unordered_access: u32,
}

pub(super) struct Platform {
    pub(super) texture: unsafe extern "C" fn(Handle, &SurfaceSetup, *mut Handle) -> i32,
    pub(super) target: Open,
    pub(super) query_resource: Create,
    pub(super) resource_handle: Create,
    pub(super) open_resource: Open,
    pub(super) query_device5: Create,
    pub(super) fence: Create,
    pub(super) fence_handle: Create,
    pub(super) open_fence: Open,
}

const _: () = {
    assert!(size_of::<SurfaceSetup>() == 48);
    assert!(std::mem::offset_of!(SurfaceSetup, log) == 24);
    assert!(std::mem::offset_of!(SurfaceSetup, unordered_access) == 40);
};

// Use the SDK vtables directly so acquire() can adopt a non-null output even on failure.
unsafe extern "C" fn native_texture(device: Handle, setup: &SurfaceSetup, out: *mut Handle) -> i32 {
    let device11 = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    let mut bind = D3D11_BIND_SHADER_RESOURCE.0;
    if setup.render_target != 0 {
        bind |= D3D11_BIND_RENDER_TARGET.0;
    }
    if setup.unordered_access != 0 {
        bind |= D3D11_BIND_UNORDERED_ACCESS.0;
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: setup.width,
        Height: setup.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT(setup.format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind as u32,
        CPUAccessFlags: 0,
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0 | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0) as u32,
    };
    unsafe { (device11.vtable().CreateTexture2D)(device, &desc, null(), out) }.0
}

unsafe extern "C" fn native_target(device: Handle, texture: Handle, out: *mut Handle) -> i32 {
    let device11 = unsafe { ID3D11Device::from_raw_borrowed(&device) }.unwrap();
    unsafe { (device11.vtable().CreateRenderTargetView)(device, texture, null(), out) }.0
}

unsafe extern "C" fn native_query<Source: Interface, Target: Interface>(
    object: Handle,
    out: *mut Handle,
) -> i32 {
    let source = unsafe { Source::from_raw_borrowed(&object) }.unwrap();
    unsafe { source.query(&Target::IID, out) }.0
}

unsafe extern "C" fn native_resource_handle(resource: Handle, out: *mut Handle) -> i32 {
    let resource1 = unsafe { IDXGIResource1::from_raw_borrowed(&resource) }.unwrap();
    unsafe {
        (resource1.vtable().CreateSharedHandle)(
            resource,
            null(),
            DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
            PCWSTR::null(),
            out.cast::<HANDLE>(),
        )
    }
    .0
}

unsafe extern "C" fn native_fence(device: Handle, out: *mut Handle) -> i32 {
    let device5 = unsafe { ID3D11Device5::from_raw_borrowed(&device) }.unwrap();
    unsafe {
        (device5.vtable().CreateFence)(device, 0, D3D11_FENCE_FLAG_SHARED, &ID3D11Fence::IID, out)
    }
    .0
}

unsafe extern "C" fn native_fence_handle(fence: Handle, out: *mut Handle) -> i32 {
    let fence11 = unsafe { ID3D11Fence::from_raw_borrowed(&fence) }.unwrap();
    unsafe {
        (fence11.vtable().CreateSharedHandle)(
            fence,
            null(),
            GENERIC_ALL.0,
            PCWSTR::null(),
            out.cast::<HANDLE>(),
        )
    }
    .0
}

unsafe extern "C" fn native_open<Target: Interface>(
    device: Handle,
    shared: Handle,
    out: *mut Handle,
) -> i32 {
    let device12 = unsafe { ID3D12Device::from_raw_borrowed(&device) }.unwrap();
    unsafe { (device12.vtable().OpenSharedHandle)(device, HANDLE(shared), &Target::IID, out) }.0
}

static NATIVE_PLATFORM: Platform = Platform {
    texture: native_texture,
    target: native_target,
    query_resource: native_query::<ID3D11Texture2D, IDXGIResource1>,
    resource_handle: native_resource_handle,
    open_resource: native_open::<ID3D12Resource>,
    query_device5: native_query::<ID3D11Device, ID3D11Device5>,
    fence: native_fence,
    fence_handle: native_fence_handle,
    open_fence: native_open::<ID3D12Fence>,
};

#[cfg_attr(test, allow(dead_code))] // The presentation tests use their own surface fakes.
pub(crate) extern "C" fn rsf_shared_surface_native_platform() -> *const Platform {
    &NATIVE_PLATFORM
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_surface_create(
    device11: Handle,
    device12: Handle,
    setup: *const SurfaceSetup,
    out: *mut *mut Surface,
) -> i32 {
    unsafe { shared_surface_create(&NATIVE_PLATFORM, device11, device12, setup, out) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_fence_create(
    device11: Handle,
    device12: Handle,
    log: Log,
    user: Handle,
    out: *mut *mut Fence,
) -> i32 {
    unsafe { shared_fence_create(&NATIVE_PLATFORM, device11, device12, log, user, out) }
}

// Rust drops fields in declaration order. Close the NT handle after the COM interfaces.
pub(super) struct Surface {
    pub(super) texture12: Option<IUnknown>,
    target: Option<IUnknown>,
    pub(super) texture11: IUnknown,
    handle: OwnedHandle,
}

pub(super) struct Fence {
    fence12: Option<IUnknown>,
    fence11: IUnknown,
    handle: OwnedHandle,
}

fn typeless(format: u32) -> bool {
    // All typeless resource and partial depth-view formats in dxgiformat.h.
    matches!(
        format,
        1 | 5
            | 9
            | 15
            | 19
            | 21
            | 22
            | 23
            | 27
            | 33
            | 39
            | 44
            | 46
            | 47
            | 48
            | 53
            | 60
            | 70
            | 73
            | 76
            | 79
            | 82
            | 90
            | 92
            | 94
            | 97
    )
}

fn acquire<T>(
    create: impl FnOnce(*mut Handle) -> i32,
    adopt: impl FnOnce(Handle) -> Option<T>,
) -> Result<T, i32> {
    let mut raw = null_mut();
    let hr = create(&mut raw);
    let owned = adopt(raw);
    if hr < 0 { Err(hr) } else { owned.ok_or(hr) }
}

fn com(create: impl FnOnce(*mut Handle) -> i32) -> Result<IUnknown, i32> {
    acquire(create, |raw| unsafe { com_ref::owned(raw) })
}

fn nt_handle(create: impl FnOnce(*mut Handle) -> i32) -> Result<OwnedHandle, i32> {
    acquire(create, |raw| unsafe { owned_win32_handle(raw) })
}

fn say(log: Log, user: Handle, message: fmt::Arguments<'_>) {
    if let Some(log) = log
        && let Ok(message) = CString::new(message.to_string())
    {
        unsafe { log(user, message.as_ptr()) };
    }
}

pub(super) fn valid_surface(device11: Handle, setup: &SurfaceSetup) -> bool {
    !device11.is_null()
        && setup.width != 0
        && setup.height != 0
        && setup.format != 0
        && !typeless(setup.format)
}

// The caller validates the setup and keeps the devices alive across the native leaves.
pub(super) unsafe fn create_surface(
    p: &Platform,
    device11: Handle,
    device12: Handle,
    setup: &SurfaceSetup,
) -> Result<Surface, i32> {
    let texture11 = com(|out| unsafe { (p.texture)(device11, setup, out) }).map_err(|hr| {
        say(
            setup.log,
            setup.log_user,
            format_args!(
                "shared surface: {}x{} format {} could not be created shareable, hr 0x{:08x}",
                setup.width, setup.height, setup.format, hr as u32
            ),
        );
        CREATE_FAILED
    })?;
    let target = if setup.render_target != 0 {
        Some(
            com(|out| unsafe { (p.target)(device11, texture11.as_raw(), out) })
                .map_err(|_| CREATE_FAILED)?,
        )
    } else {
        None
    };
    let handle = {
        let resource =
            com(|out| unsafe { (p.query_resource)(texture11.as_raw(), out) }).map_err(|hr| {
                say(
                    setup.log,
                    setup.log_user,
                    format_args!(
                        "shared surface: the texture is not an IDXGIResource1, hr 0x{:08x}",
                        hr as u32
                    ),
                );
                NO_HANDLE
            })?;
        nt_handle(|out| unsafe { (p.resource_handle)(resource.as_raw(), out) }).map_err(|hr| {
            say(
                setup.log,
                setup.log_user,
                format_args!("shared surface: no NT handle, hr 0x{:08x}", hr as u32),
            );
            NO_HANDLE
        })?
    };
    let mut surface = Surface {
        texture12: None,
        target,
        texture11,
        handle,
    };
    if !device12.is_null() {
        let shared = surface.handle.as_raw_handle();
        surface.texture12 = Some(
            com(|out| unsafe { (p.open_resource)(device12, shared, out) }).map_err(|hr| {
                say(
                    setup.log,
                    setup.log_user,
                    format_args!(
                        "shared surface: D3D12 would not open the handle, hr 0x{:08x}",
                        hr as u32
                    ),
                );
                OPEN_FAILED
            })?,
        );
    }
    Ok(surface)
}

fn create_fence(
    p: &Platform,
    device11: Handle,
    device12: Handle,
    log: Log,
    user: Handle,
) -> Result<Fence, i32> {
    let device5 = com(|out| unsafe { (p.query_device5)(device11, out) }).map_err(|hr| {
        say(
            log,
            user,
            format_args!("shared fence: no ID3D11Device5, hr 0x{:08x}", hr as u32),
        );
        CREATE_FAILED
    })?;
    let fence11 = com(|out| unsafe { (p.fence)(device5.as_raw(), out) }).map_err(|hr| {
        say(
            log,
            user,
            format_args!("shared fence: could not be created, hr 0x{:08x}", hr as u32),
        );
        CREATE_FAILED
    })?;
    let handle =
        nt_handle(|out| unsafe { (p.fence_handle)(fence11.as_raw(), out) }).map_err(|hr| {
            say(
                log,
                user,
                format_args!("shared fence: no NT handle, hr 0x{:08x}", hr as u32),
            );
            NO_HANDLE
        })?;
    let mut fence = Fence {
        fence12: None,
        fence11,
        handle,
    };
    if !device12.is_null() {
        let shared = fence.handle.as_raw_handle();
        fence.fence12 = Some(
            com(|out| unsafe { (p.open_fence)(device12, shared, out) }).map_err(|hr| {
                say(
                    log,
                    user,
                    format_args!(
                        "shared fence: D3D12 would not open the handle, hr 0x{:08x}",
                        hr as u32
                    ),
                );
                OPEN_FAILED
            })?,
        );
    }
    Ok(fence)
}

unsafe fn shared_surface_create(
    platform: &Platform,
    device11: Handle,
    device12: Handle,
    setup: *const SurfaceSetup,
    out: *mut *mut Surface,
) -> i32 {
    if device11.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI_VERSION {
        return ABI_MISMATCH;
    }
    if !valid_surface(device11, setup) {
        return INVALID;
    }
    unsafe { out.write(null_mut()) };
    match unsafe { create_surface(platform, device11, device12, setup) } {
        Ok(surface) => {
            let surface = allocate_handle(surface);
            unsafe { out.write(surface) };
            if surface.is_null() { CREATE_FAILED } else { OK }
        }
        Err(result) => result,
    }
}

unsafe fn shared_fence_create(
    platform: &Platform,
    device11: Handle,
    device12: Handle,
    log: Log,
    user: Handle,
    out: *mut *mut Fence,
) -> i32 {
    if device11.is_null() || out.is_null() {
        return INVALID;
    }
    unsafe { out.write(null_mut()) };
    match create_fence(platform, device11, device12, log, user) {
        Ok(fence) => {
            let fence = allocate_handle(fence);
            unsafe { out.write(fence) };
            if fence.is_null() { CREATE_FAILED } else { OK }
        }
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_surface_destroy(surface: *mut Surface) {
    if !surface.is_null() {
        drop(unsafe { Box::from_raw(surface) });
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_surface_d3d11(surface: *const Surface) -> Handle {
    unsafe { surface.as_ref() }.map_or(null_mut(), |surface| surface.texture11.as_raw())
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_surface_d3d12(surface: *const Surface) -> Handle {
    unsafe { surface.as_ref() }
        .and_then(|surface| surface.texture12.as_ref())
        .map_or(null_mut(), Interface::as_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_surface_target(surface: *const Surface) -> Handle {
    unsafe { surface.as_ref() }
        .and_then(|surface| surface.target.as_ref())
        .map_or(null_mut(), Interface::as_raw)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_fence_destroy(fence: *mut Fence) {
    if !fence.is_null() {
        drop(unsafe { Box::from_raw(fence) });
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_fence_d3d11(fence: *const Fence) -> Handle {
    unsafe { fence.as_ref() }.map_or(null_mut(), |fence| fence.fence11.as_raw())
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_shared_fence_d3d12(fence: *const Fence) -> Handle {
    unsafe { fence.as_ref() }
        .and_then(|fence| fence.fence12.as_ref())
        .map_or(null_mut(), Interface::as_raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_ref::test::{add_ref, fake, raw};
    use std::ptr::NonNull;
    use windows_sys::Win32::System::Threading::CreateEventW;

    unsafe extern "C" fn retain(object: Handle, out: *mut Handle) -> i32 {
        unsafe {
            add_ref(object.cast());
            out.write(object);
        }
        OK
    }

    unsafe extern "C" fn texture(device: Handle, _: &SurfaceSetup, out: *mut Handle) -> i32 {
        unsafe { retain(device, out) }
    }

    unsafe extern "C" fn target(_: Handle, texture11: Handle, out: *mut Handle) -> i32 {
        unsafe { retain(texture11, out) }
    }

    unsafe extern "C" fn event(_: Handle, out: *mut Handle) -> i32 {
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        unsafe { out.write(handle) };
        if handle.is_null() { -1 } else { OK }
    }

    unsafe extern "C" fn refuse_open(_: Handle, _: Handle, _: *mut Handle) -> i32 {
        -1
    }

    const PLATFORM: Platform = Platform {
        texture,
        target,
        query_resource: retain,
        resource_handle: event,
        open_resource: refuse_open,
        query_device5: retain,
        fence: retain,
        fence_handle: event,
        open_fence: refuse_open,
    };

    fn setup() -> SurfaceSetup {
        SurfaceSetup {
            struct_size: size_of::<SurfaceSetup>() as u32,
            abi_version: ABI_VERSION,
            width: 64,
            height: 32,
            format: 28, // DXGI_FORMAT_R8G8B8A8_UNORM
            render_target: 1,
            log: None,
            log_user: null_mut(),
            unordered_access: 0,
        }
    }

    #[test]
    fn refuses_untyped_storage_before_publishing_output() {
        let device = fake();
        let mut setup = setup();
        let sentinel = NonNull::<Surface>::dangling().as_ptr();
        let mut out = sentinel;
        for format in [19, 21, 22, 46, 47, 79, 82, 94, 97] {
            setup.format = format;
            assert_eq!(
                unsafe {
                    shared_surface_create(&PLATFORM, raw(&device), null_mut(), &setup, &mut out)
                },
                INVALID
            );
            assert_eq!(out, sentinel);
        }
        let short_prefix = 4u32;
        assert_eq!(
            unsafe {
                shared_surface_create(
                    &PLATFORM,
                    raw(&device),
                    null_mut(),
                    std::ptr::from_ref(&short_prefix).cast(),
                    &mut out,
                )
            },
            INVALID
        );
        assert_eq!(out, sentinel);
    }

    #[test]
    fn keeps_d3d11_leases_without_peer_and_releases_on_refused_open() {
        let device = fake();
        let device11 = raw(&device);
        let mut surface = null_mut();
        assert_eq!(
            unsafe {
                shared_surface_create(&PLATFORM, device11, null_mut(), &setup(), &mut surface)
            },
            OK
        );
        assert_eq!(unsafe { rsf_shared_surface_d3d11(surface) }, device11);
        assert_eq!(unsafe { rsf_shared_surface_target(surface) }, device11);
        assert!(unsafe { rsf_shared_surface_d3d12(surface) }.is_null());
        assert_eq!(device.refs.get(), 3);
        unsafe { rsf_shared_surface_destroy(surface) };
        assert_eq!(device.refs.get(), 1);

        assert_eq!(
            unsafe { shared_surface_create(&PLATFORM, device11, device11, &setup(), &mut surface) },
            OPEN_FAILED
        );
        assert!(surface.is_null());
        assert_eq!(device.refs.get(), 1);

        let mut fence = null_mut();
        assert_eq!(
            unsafe {
                shared_fence_create(&PLATFORM, device11, device11, None, null_mut(), &mut fence)
            },
            OPEN_FAILED
        );
        assert!(fence.is_null());
        assert_eq!(device.refs.get(), 1);
    }
}
