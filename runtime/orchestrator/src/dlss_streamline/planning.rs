// SPDX-License-Identifier: MIT
use super::{Handle, OK, Plan, Startup, Support};
use crate::streamline_sr::sdk::{
    RESULT_DRIVER_OUT_OF_DATE, RESULT_NO_SUPPORTED_ADAPTER, RESULT_OS_OUT_OF_DATE,
};
use crate::streamline_sr::{Dimensions, Optimal, dimensions};
use std::ptr::{addr_of, addr_of_mut};

const NOT_SUPPORTED: i32 = -7;

#[repr(C)]
#[derive(Default)]
pub(super) struct DriverVersions {
    pub(super) required_major: u32,
    pub(super) required_minor: u32,
    pub(super) detected_major: u32,
    pub(super) detected_minor: u32,
}

type GetLuid = unsafe extern "C" fn(Handle, &mut u64) -> i32;

#[repr(C)]
pub(super) struct Platform {
    pub(super) d3d11_luid: GetLuid,
    pub(super) device_luid: GetLuid,
    pub(super) query_support: unsafe extern "C" fn(Handle, u64, &mut u32) -> i32,
    pub(super) requirements: unsafe extern "C" fn(Handle, &mut DriverVersions) -> i32,
    pub(super) optimal: unsafe extern "C" fn(Handle, &Dimensions, &mut Optimal) -> i32,
}

const _: () = {
    assert!(size_of::<Platform>() == 5 * size_of::<Handle>());
    assert!(size_of::<DriverVersions>() == 16);
};

impl Platform {
    fn luid(&self, device: Handle, shared: bool) -> Result<u64, i32> {
        let query = if shared {
            self.device_luid
        } else {
            self.d3d11_luid
        };
        let mut luid = 0;
        let result = unsafe { query(device, &mut luid) };
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
                driver_out_of_date: (result == RESULT_DRIVER_OUT_OF_DATE).into(),
                os_out_of_date: (result == RESULT_OS_OUT_OF_DATE).into(),
                no_supported_adapter: (result == RESULT_NO_SUPPORTED_ADAPTER).into(),
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
        let mut sizes = Optimal::default();
        let status =
            unsafe { (self.optimal)(native, &dimensions(quality, width, height), &mut sizes) };
        if status != OK {
            return status;
        }
        let (render_width, render_height) = (sizes.width, sizes.height);
        unsafe {
            addr_of_mut!((*out).render_width).write(render_width);
            addr_of_mut!((*out).render_height).write(render_height);
            addr_of_mut!((*out).render_width_min).write(sizes.width_min);
            addr_of_mut!((*out).render_height_min).write(sizes.height_min);
            addr_of_mut!((*out).render_width_max).write(sizes.width_max);
            addr_of_mut!((*out).render_height_max).write(sizes.height_max);
        }
        startup.report_text(format!(
            "DLSS wants {render_width}x{render_height} to produce {width}x{height}"
        ));
        OK
    }
}
