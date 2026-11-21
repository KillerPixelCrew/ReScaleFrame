// SPDX-License-Identifier: MIT
use super::{ComRef, Handle, NOT_READY, OK, Plan, Startup, Support};
use crate::streamline_sr::dlss_mode;
use std::ptr::{addr_of, addr_of_mut, null_mut};

const NOT_SUPPORTED: i32 = -7;
const DRIVER_OUT_OF_DATE: u32 = 2;
const OS_OUT_OF_DATE: u32 = 3;
const NO_SUPPORTED_ADAPTER: u32 = 6;

#[repr(C)]
#[derive(Default)]
struct DriverVersions {
    required_major: u32,
    required_minor: u32,
    detected_major: u32,
    detected_minor: u32,
}

#[repr(C)]
#[derive(Default)]
struct RenderSizes {
    optimal: [u32; 2],
    minimum: [u32; 2],
    maximum: [u32; 2],
}

type Query = unsafe extern "C" fn(Handle, &mut Handle) -> i32;
type GetLuid = unsafe extern "C" fn(Handle, &mut u64) -> i32;

#[repr(C)]
pub(super) struct Platform {
    query_dxgi_device: Query,
    get_adapter: Query,
    adapter_luid: GetLuid,
    device_luid: GetLuid,
    query_support: unsafe extern "C" fn(Handle, u64, &mut u32) -> i32,
    requirements: unsafe extern "C" fn(Handle, &mut DriverVersions) -> i32,
    optimal: unsafe extern "C" fn(Handle, u32, u32, u32, &mut RenderSizes) -> i32,
}

const _: () = {
    assert!(size_of::<Platform>() == 7 * size_of::<Handle>());
    assert!(size_of::<DriverVersions>() == 16);
    assert!(size_of::<RenderSizes>() == 24);
};

fn query_owned(query: Query, source: Handle) -> Result<ComRef, i32> {
    let mut object = null_mut();
    let result = unsafe { query(source, &mut object) };
    let owned = unsafe { ComRef::from_owned(object) };
    if result != OK {
        return Err(result);
    }
    owned.ok_or(NOT_READY)
}

impl Platform {
    fn luid(&self, device: Handle, shared: bool) -> Result<u64, i32> {
        let mut luid = 0;
        let result = if shared {
            unsafe { (self.device_luid)(device, &mut luid) }
        } else {
            // Query the adapter of the caller's D3D11 device and release both
            // interfaces before calling Streamline.
            let dxgi = query_owned(self.query_dxgi_device, device)?;
            let adapter = query_owned(self.get_adapter, dxgi.as_ptr())?;
            unsafe { (self.adapter_luid)(adapter.as_ptr(), &mut luid) }
        };
        if result == OK { Ok(luid) } else { Err(result) }
    }

    pub(super) unsafe fn support(
        &self,
        native: Handle,
        device: Handle,
        shared: bool,
        out: *mut Support,
        startup: &Startup,
    ) -> i32 {
        let luid = match self.luid(device, shared) {
            Ok(luid) => luid,
            Err(result) => return result,
        };
        let mut result = 0;
        let status = unsafe { (self.query_support)(native, luid, &mut result) };
        if status != OK {
            return status;
        }
        let supported = result == 0;
        // The caller may have initialized only struct_size. Publish the
        // capability flags before the optional driver requirements query.
        let size = unsafe { addr_of!((*out).struct_size).read() };
        unsafe {
            out.write(Support {
                struct_size: size,
                supported: supported.into(),
                driver_out_of_date: (result == DRIVER_OUT_OF_DATE).into(),
                os_out_of_date: (result == OS_OUT_OF_DATE).into(),
                no_supported_adapter: (result == NO_SUPPORTED_ADAPTER).into(),
                required_driver_major: 0,
                required_driver_minor: 0,
                detected_driver_major: 0,
                detected_driver_minor: 0,
            });
        }
        let mut versions = DriverVersions::default();
        let status = unsafe { (self.requirements)(native, &mut versions) };
        if status != OK {
            return status;
        }
        unsafe {
            addr_of_mut!((*out).required_driver_major).write(versions.required_major);
            addr_of_mut!((*out).required_driver_minor).write(versions.required_minor);
            addr_of_mut!((*out).detected_driver_major).write(versions.detected_major);
            addr_of_mut!((*out).detected_driver_minor).write(versions.detected_minor);
        }
        startup.report_text(format!(
            "DLSS support on this adapter: {} (streamline result {result}, driver {}.{}, requires {}.{})",
            if supported { "yes" } else { "no" },
            versions.detected_major,
            versions.detected_minor,
            versions.required_major,
            versions.required_minor,
        ));
        if supported { OK } else { NOT_SUPPORTED }
    }

    pub(super) unsafe fn plan(&self, native: Handle, out: *mut Plan, startup: &Startup) -> i32 {
        // The caller initializes the inputs only.
        let (width, height, quality) = unsafe {
            (
                addr_of!((*out).output_width).read(),
                addr_of!((*out).output_height).read(),
                addr_of!((*out).quality).read(),
            )
        };
        let mut sizes = RenderSizes::default();
        let status =
            unsafe { (self.optimal)(native, dlss_mode(quality), width, height, &mut sizes) };
        if status != OK {
            return status;
        }
        let [render_width, render_height] = sizes.optimal;
        unsafe {
            addr_of_mut!((*out).render_width).write(render_width);
            addr_of_mut!((*out).render_height).write(render_height);
            addr_of_mut!((*out).render_width_min).write(sizes.minimum[0]);
            addr_of_mut!((*out).render_height_min).write(sizes.minimum[1]);
            addr_of_mut!((*out).render_width_max).write(sizes.maximum[0]);
            addr_of_mut!((*out).render_height_max).write(sizes.maximum[1]);
        }
        startup.report_text(format!(
            "DLSS wants {render_width}x{render_height} to produce {width}x{height}"
        ));
        OK
    }
}
