// SPDX-License-Identifier: MIT
//! Package and adapter facts used to select a D3D11 SR provider before startup.
use crate::d3d11_device_identity::device_vendor;
use crate::sr_choices::{directory, package_has, sr_backends};
use rsf_game_sys::{
    RSF_D3D11_SR_ADMISSION_ABI_VERSION, RSF_SR_AUTO as AUTO, rsf_d3d11_sr_admission as Admission,
};
use std::ffi::CStr;

fn select(vendor: u32, requested: u32, offered: u32) -> u32 {
    if vendor != 0x1414 && (1..AUTO).contains(&requested) && offered & (1 << requested) != 0 {
        return requested;
    }
    crate::rsf_gpu_select_sr_backend(vendor, u32::from(vendor == 0x1414), offered)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_sr_admit(request: *mut Admission) -> i32 {
    if !unsafe { rsf_game_sys::ffi_util::has_capacity(request) } {
        return -1;
    }
    let request = unsafe { &mut *request };
    if request.abi_version != RSF_D3D11_SR_ADMISSION_ABI_VERSION {
        return -2;
    }
    let directories = [
        request.streamline_directory_utf8,
        request.fidelityfx_directory_utf8,
        request.xess_directory_utf8,
    ]
    .map(|text| {
        (!text.is_null())
            .then(|| unsafe { CStr::from_ptr(text) })
            .and_then(directory)
    });
    let vendor = if request.device.is_null() {
        None
    } else {
        let Some(vendor) = (unsafe { device_vendor(request.device) }) else {
            return -3;
        };
        Some(vendor)
    };
    // The adapter's limit applies to providers; Auto follows whatever provider is left.
    let concrete = sr_backends(vendor, package_has(directories), false)
        & !(1 << AUTO)
        & request.backend_choices;
    request.offered_backends = concrete | (u32::from(concrete != 0) << AUTO);
    request.selected_backend = match vendor {
        None if concrete != 0 => AUTO,
        None => 0,
        Some(vendor) => select(vendor, request.requested_backend, concrete),
    };
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    use rsf_game_sys::{RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_XESS};

    #[test]
    fn admission_offers_the_common_set_and_never_more_than_the_adapter_allows() {
        use crate::sr_choices::{COMMON, Packages};
        use std::ffi::CString;
        let packages = Packages::all("admission");
        let leaf = |name: &str| CString::new(packages.0.join(name).to_str().unwrap()).unwrap();
        let (streamline, fidelityfx, xess) = (leaf("streamline"), leaf("fidelityfx"), leaf("xess"));
        let admit = |choices: u32| {
            let mut request = Admission {
                struct_size: size_of::<Admission>() as u32,
                abi_version: RSF_D3D11_SR_ADMISSION_ABI_VERSION,
                backend_choices: choices,
                streamline_directory_utf8: streamline.as_ptr(),
                fidelityfx_directory_utf8: fidelityfx.as_ptr(),
                xess_directory_utf8: xess.as_ptr(),
                ..Default::default()
            };
            assert_eq!(unsafe { rsf_d3d11_sr_admit(&mut request) }, 0);
            request
        };
        // An adapter that passes the contract's "no limit" gets the common set before any device.
        let open = admit(rsf_game_sys::RSF_SR_CHOICES_ALL);
        assert_eq!(open.offered_backends, COMMON);
        assert_eq!(open.selected_backend, AUTO);
        // A limit of its own narrows it, Auto with it only when a provider is left.
        assert_eq!(
            admit(1 << RSF_SR_XESS).offered_backends,
            (1 << RSF_SR_XESS) | (1 << AUTO)
        );
        assert_eq!(admit(0).offered_backends, 0);
    }

    #[test]
    fn packages_and_gpu_preferences_are_independent() {
        let all = sr_backends(None, |_, _| true, false) & !(1 << AUTO);
        let xess = sr_backends(
            None,
            |index, name| index == 2 && name == "libxess.dll",
            false,
        ) & !(1 << AUTO);
        assert_eq!(xess, 1 << RSF_SR_XESS);
        assert_eq!(select(0x10de, AUTO, all), RSF_SR_DLSS);
        assert_eq!(select(0x1002, AUTO, all), RSF_SR_FSR3);
        assert_eq!(select(0x8086, AUTO, all), RSF_SR_XESS);
        assert_eq!(select(0x1002, RSF_SR_FSR4, all), RSF_SR_FSR4);
        assert_eq!(select(0x10de, RSF_SR_DLSS, xess), RSF_SR_XESS);
        assert_eq!(select(0x1414, RSF_SR_XESS, all), 0);
    }
}
