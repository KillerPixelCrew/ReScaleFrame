// SPDX-License-Identifier: MIT
//! Package and adapter facts used to select a D3D11 SR provider before startup.
use crate::d3d11_device_identity::device_vendor;
use rsf_game_sys::{
    RSF_D3D11_SR_ADMISSION_ABI_VERSION, RSF_SR_AUTO as AUTO, RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_FSR4,
    RSF_SR_XESS, rsf_d3d11_sr_admission as Admission,
};
use std::ffi::CStr;
use std::path::Path;

fn package_mask(has: impl Fn(usize, &str) -> bool) -> u32 {
    let mut offered = 0;
    if has(0, "sl.interposer.dll") && has(0, "nvngx_dlss.dll") {
        offered |= 1 << RSF_SR_DLSS;
    }
    if has(1, "amd_fidelityfx_upscaler_dx12.dll") && has(1, "amd_fidelityfx_loader_dx12.dll") {
        offered |= (1 << RSF_SR_FSR3) | (1 << RSF_SR_FSR4);
    }
    if has(2, "libxess.dll") {
        offered |= 1 << RSF_SR_XESS;
    }
    offered
}

fn select(vendor: u32, requested: u32, offered: u32) -> u32 {
    if vendor != 0x1414 && (1..AUTO).contains(&requested) && offered & (1 << requested) != 0 {
        return requested;
    }
    crate::rsf_gpu_select_sr_backend(vendor, u32::from(vendor == 0x1414), offered)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_sr_admit(request: *mut Admission) -> i32 {
    if request.is_null() || unsafe { request.cast::<u32>().read() } < size_of::<Admission>() as u32
    {
        return -1;
    }
    if unsafe { request.cast::<u32>().add(1).read() } != RSF_D3D11_SR_ADMISSION_ABI_VERSION {
        return -2;
    }
    let request = unsafe { &mut *request };
    let directories = [
        request.streamline_directory_utf8,
        request.fidelityfx_directory_utf8,
        request.xess_directory_utf8,
    ]
    .map(|directory| {
        if directory.is_null() {
            return None;
        }
        let path = unsafe { CStr::from_ptr(directory) }.to_str().ok()?;
        (!path.is_empty()).then(|| Path::new(path))
    });
    let concrete = package_mask(|index, name| {
        directories[index].is_some_and(|directory| directory.join(name).is_file())
    }) & request.backend_choices;
    request.offered_backends = concrete | (u32::from(concrete != 0) << AUTO);
    request.selected_backend = 0;
    if request.device.is_null() {
        request.selected_backend = if concrete != 0 { AUTO } else { 0 };
        return 0;
    }
    let Some(vendor) = (unsafe { device_vendor(request.device) }) else {
        return -3;
    };
    request.selected_backend = select(vendor, request.requested_backend, concrete);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_and_gpu_preferences_are_independent() {
        let all = package_mask(|_, _| true);
        let xess = package_mask(|index, name| index == 2 && name == "libxess.dll");
        assert_eq!(xess, 1 << RSF_SR_XESS);
        assert_eq!(select(0x10de, AUTO, all), RSF_SR_DLSS);
        assert_eq!(select(0x1002, AUTO, all), RSF_SR_FSR3);
        assert_eq!(select(0x8086, AUTO, all), RSF_SR_XESS);
        assert_eq!(select(0x1002, RSF_SR_FSR4, all), RSF_SR_FSR4);
        assert_eq!(select(0x10de, RSF_SR_DLSS, xess), RSF_SR_XESS);
        assert_eq!(select(0x1414, RSF_SR_XESS, all), 0);
    }
}
