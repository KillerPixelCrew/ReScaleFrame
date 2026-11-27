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

/// Whether an adapter name is an RTX GPU's. DLSS runs on tensor cores, which every NVIDIA RTX GPU
/// has (GeForce RTX, TITAN RTX, Quadro RTX, RTX A and Ada) and no GeForce GTX has, Turing GTX 16
/// included; the driver names the adapter.
fn rtx_name(description: &[u16]) -> bool {
    String::from_utf16_lossy(description)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == "RTX")
}

/// The pre-start DLSS capability query: an NVIDIA RTX adapter is present. Read once per process
/// from DXGI, before any provider starts, so a machine without one sees DLSS greyed instead of
/// offered and then refused by Streamline. Streamline still confirms support at start.
#[cfg(not(test))]
fn rtx_present() -> bool {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    static PRESENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PRESENT.get_or_init(|| {
        let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
            return false;
        };
        (0..)
            .map_while(|index| unsafe { factory.EnumAdapters1(index) }.ok())
            .filter_map(|adapter| unsafe { adapter.GetDesc1() }.ok())
            .any(|desc| desc.VendorId == NVIDIA && rtx_name(&desc.Description))
    })
}

// Unit tests describe an RTX machine; rtx_name is tested on its own.
#[cfg(test)]
fn rtx_present() -> bool {
    true
}

/// The SR backends this machine can serve. `vendor` is the owning adapter's, or `None` before a
/// device exists. FSR2, FSR3 and FSR4 share one FidelityFX upscaler package. DLSS needs an NVIDIA
/// adapter on a machine with an RTX GPU. `engine_spatial` is a spatial scaler the host serves: its
/// engine's own, or the Runtime's (sr12 on D3D12). Auto is offered once any provider is.
pub(crate) fn sr_backends(
    vendor: Option<u32>,
    has: impl Fn(usize, &str) -> bool,
    engine_spatial: bool,
) -> u32 {
    let mut offered = 0;
    if has(0, "sl.interposer.dll")
        && has(0, "nvngx_dlss.dll")
        && vendor.is_none_or(|v| v == NVIDIA)
        && rtx_present()
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
    fn only_rtx_adapters_run_dlss() {
        let name = |text: &str| rtx_name(&text.encode_utf16().collect::<Vec<_>>());
        for rtx in [
            "NVIDIA GeForce RTX 4090",
            "NVIDIA GeForce RTX 2060 SUPER",
            "NVIDIA TITAN RTX",
            "Quadro RTX 5000",
            "NVIDIA RTX A4000",
            "NVIDIA RTX 2000 Ada Generation Laptop GPU",
        ] {
            assert!(name(rtx), "{rtx}");
        }
        for gtx in [
            "NVIDIA GeForce GTX 1660 SUPER",
            "NVIDIA GeForce GTX 1080 Ti",
            "NVIDIA GeForce MX450",
            "NVIDIA RTXDI test adapter",
            "",
        ] {
            assert!(!name(gtx), "{gtx}");
        }
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
