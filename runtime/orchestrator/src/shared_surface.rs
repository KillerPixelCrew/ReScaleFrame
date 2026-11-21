// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::render_abi::rsf_backend_log_fn;
use crate::sr_session::has_capacity;
use crate::{allocate_handle, owned_win32_handle};
use std::ffi::{CString, c_void};
use std::fmt;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::null_mut;

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

#[repr(C)]
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
    assert!(size_of::<Platform>() == 9 * size_of::<Handle>());
};

// Rust drops fields in declaration order. Close the NT handle after the COM interfaces.
pub(super) struct Surface {
    pub(super) texture12: Option<ComRef>,
    target: Option<ComRef>,
    pub(super) texture11: ComRef,
    handle: OwnedHandle,
}

struct Fence {
    fence12: Option<ComRef>,
    fence11: ComRef,
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

fn com(create: impl FnOnce(*mut Handle) -> i32) -> Result<ComRef, i32> {
    acquire(create, |raw| unsafe { ComRef::from_owned(raw) })
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
            com(|out| unsafe { (p.target)(device11, texture11.as_ptr(), out) })
                .map_err(|_| CREATE_FAILED)?,
        )
    } else {
        None
    };
    let handle = {
        let resource =
            com(|out| unsafe { (p.query_resource)(texture11.as_ptr(), out) }).map_err(|hr| {
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
        nt_handle(|out| unsafe { (p.resource_handle)(resource.as_ptr(), out) }).map_err(|hr| {
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
    let fence11 = com(|out| unsafe { (p.fence)(device5.as_ptr(), out) }).map_err(|hr| {
        say(
            log,
            user,
            format_args!("shared fence: could not be created, hr 0x{:08x}", hr as u32),
        );
        CREATE_FAILED
    })?;
    let handle =
        nt_handle(|out| unsafe { (p.fence_handle)(fence11.as_ptr(), out) }).map_err(|hr| {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_shared_surface_create(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_shared_fence_create(
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
unsafe extern "C" fn rsf_shared_surface_destroy(surface: *mut Surface) {
    if !surface.is_null() {
        drop(unsafe { Box::from_raw(surface) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_surface_d3d11(surface: *const Surface) -> Handle {
    unsafe { surface.as_ref() }.map_or(null_mut(), |surface| surface.texture11.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_surface_d3d12(surface: *const Surface) -> Handle {
    unsafe { surface.as_ref() }
        .and_then(|surface| surface.texture12.as_ref())
        .map_or(null_mut(), ComRef::as_ptr)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_surface_target(surface: *const Surface) -> Handle {
    unsafe { surface.as_ref() }
        .and_then(|surface| surface.target.as_ref())
        .map_or(null_mut(), ComRef::as_ptr)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_fence_destroy(fence: *mut Fence) {
    if !fence.is_null() {
        drop(unsafe { Box::from_raw(fence) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_fence_d3d11(fence: *const Fence) -> Handle {
    unsafe { fence.as_ref() }.map_or(null_mut(), |fence| fence.fence11.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_shared_fence_d3d12(fence: *const Fence) -> Handle {
    unsafe { fence.as_ref() }
        .and_then(|fence| fence.fence12.as_ref())
        .map_or(null_mut(), ComRef::as_ptr)
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
                    rsf_core_shared_surface_create(
                        &PLATFORM,
                        raw(&device),
                        null_mut(),
                        &setup,
                        &mut out,
                    )
                },
                INVALID
            );
            assert_eq!(out, sentinel);
        }
        let short_prefix = 4u32;
        assert_eq!(
            unsafe {
                rsf_core_shared_surface_create(
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
                rsf_core_shared_surface_create(
                    &PLATFORM,
                    device11,
                    null_mut(),
                    &setup(),
                    &mut surface,
                )
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
            unsafe {
                rsf_core_shared_surface_create(
                    &PLATFORM,
                    device11,
                    device11,
                    &setup(),
                    &mut surface,
                )
            },
            OPEN_FAILED
        );
        assert!(surface.is_null());
        assert_eq!(device.refs.get(), 1);

        let mut fence = null_mut();
        assert_eq!(
            unsafe {
                rsf_core_shared_fence_create(
                    &PLATFORM,
                    device11,
                    device11,
                    None,
                    null_mut(),
                    &mut fence,
                )
            },
            OPEN_FAILED
        );
        assert!(fence.is_null());
        assert_eq!(device.refs.get(), 1);
    }
}
