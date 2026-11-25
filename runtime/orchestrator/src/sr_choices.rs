// SPDX-License-Identifier: MIT
//! The SR choices every host's overlay offers, from one place. A host reports facts (its device,
//! the packages beside the Runtime, whether its engine has its own spatial scaler); it keeps no list
//! of backends. A value this GPU or package cannot serve is still shown by the panel, greyed.
use crate::sys::{
    RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK, RSF_OVERLAY_SR_QUALITY_STANDARD_MASK, RSF_SR_DLSS,
    RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_SPATIAL, RSF_SR_XESS,
};
use rsf_game_sys::RSF_SR_AUTO;
use std::ffi::CStr;
use std::path::Path;

const NVIDIA: u32 = 0x10de;

/// A package directory a host or setup carries as text, or `None` when it names none.
pub(crate) fn directory(text: &CStr) -> Option<&Path> {
    text.to_str()
        .ok()
        .filter(|text| !text.is_empty())
        .map(Path::new)
}

/// The package directories in the order `has` is asked about them: Streamline, FidelityFX, XeSS.
pub(crate) fn package_has<'a>(
    directories: [Option<&'a Path>; 3],
) -> impl Fn(usize, &str) -> bool + 'a {
    move |index, name| directories[index].is_some_and(|directory| directory.join(name).is_file())
}

/// The SR backends this machine can serve. `vendor` is the owning adapter's, or `None` before a
/// device exists. FSR2, FSR3 and FSR4 share one FidelityFX upscaler package. DLSS needs an NVIDIA
/// adapter. `engine_spatial` is the host's own scaler. Auto is offered once any provider is.
pub(crate) fn sr_backends(
    vendor: Option<u32>,
    has: impl Fn(usize, &str) -> bool,
    engine_spatial: bool,
) -> u32 {
    let mut offered = 0;
    if has(0, "sl.interposer.dll") && has(0, "nvngx_dlss.dll") && vendor.is_none_or(|v| v == NVIDIA)
    {
        offered |= 1 << RSF_SR_DLSS;
    }
    if has(1, "amd_fidelityfx_upscaler_dx12.dll") && has(1, "amd_fidelityfx_loader_dx12.dll") {
        offered |= (1 << RSF_SR_FSR2) | (1 << RSF_SR_FSR3) | (1 << RSF_SR_FSR4);
    }
    if has(2, "libxess.dll") {
        offered |= 1 << RSF_SR_XESS;
    }
    offered
        | (u32::from(offered != 0) << RSF_SR_AUTO)
        | (u32::from(engine_spatial) << RSF_SR_SPATIAL)
}

/// The quality levels a backend serves: FSR2, FSR3, FSR4 and DLSS stop at Ultra performance, XeSS
/// and a spatial scaler add Ultra Quality. Auto offers what every provider it can pick accepts.
pub(crate) fn sr_quality(backend: u32) -> u32 {
    match backend {
        RSF_SR_XESS | RSF_SR_SPATIAL | 0 => RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK,
        RSF_SR_DLSS | RSF_SR_FSR2 | RSF_SR_FSR3 | RSF_SR_FSR4 | RSF_SR_AUTO => {
            RSF_OVERLAY_SR_QUALITY_STANDARD_MASK
        }
        _ => 0,
    }
}

/// What every host offers on a machine with every package and an NVIDIA GPU: the common set.
/// A host's own test compares its published mask with this.
#[cfg(test)]
pub(crate) const COMMON: u32 = (1 << RSF_SR_DLSS)
    | (1 << RSF_SR_FSR2)
    | (1 << RSF_SR_FSR3)
    | (1 << RSF_SR_FSR4)
    | (1 << RSF_SR_XESS)
    | (1 << RSF_SR_AUTO);

/// A directory tree holding every package file `sr_backends` looks for, removed on drop.
#[cfg(test)]
pub(crate) struct Packages(pub(crate) std::path::PathBuf);

#[cfg(test)]
impl Packages {
    pub(crate) fn all(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("rsf-{name}-{}", std::process::id()));
        for (directory, file) in [
            ("streamline", "sl.interposer.dll"),
            ("streamline", "nvngx_dlss.dll"),
            ("fidelityfx", "amd_fidelityfx_upscaler_dx12.dll"),
            ("fidelityfx", "amd_fidelityfx_loader_dx12.dll"),
            ("xess", "libxess.dll"),
        ] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
            std::fs::write(root.join(directory).join(file), b"").unwrap();
        }
        Self(root)
    }
}

#[cfg(test)]
impl Drop for Packages {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = COMMON;

    #[test]
    fn every_provider_is_offered_with_its_packages() {
        assert_eq!(sr_backends(Some(NVIDIA), |_, _| true, false), ALL);
        assert_eq!(
            sr_backends(None, |_, _| true, true),
            ALL | (1 << RSF_SR_SPATIAL)
        );
    }

    #[test]
    fn a_gpu_or_a_missing_package_removes_only_what_it_must() {
        let amd = sr_backends(Some(0x1002), |_, _| true, false);
        assert_eq!(amd, ALL & !(1 << RSF_SR_DLSS));
        let xess_only = sr_backends(
            Some(NVIDIA),
            |index, name| index == 2 && name == "libxess.dll",
            false,
        );
        assert_eq!(xess_only, (1 << RSF_SR_XESS) | (1 << RSF_SR_AUTO));
        assert_eq!(sr_backends(Some(NVIDIA), |_, _| false, false), 0);
        // A spatial scaler alone does not make Auto available: Auto picks a provider.
        assert_eq!(sr_backends(None, |_, _| false, true), 1 << RSF_SR_SPATIAL);
    }

    #[test]
    fn ultra_quality_belongs_to_xess_and_spatial() {
        for backend in [
            RSF_SR_DLSS,
            RSF_SR_FSR2,
            RSF_SR_FSR3,
            RSF_SR_FSR4,
            RSF_SR_AUTO,
        ] {
            assert_eq!(sr_quality(backend), RSF_OVERLAY_SR_QUALITY_STANDARD_MASK);
        }
        for backend in [RSF_SR_XESS, RSF_SR_SPATIAL] {
            assert_eq!(sr_quality(backend), RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK);
        }
    }
}
