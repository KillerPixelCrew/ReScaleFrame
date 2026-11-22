// SPDX-License-Identifier: MIT
//! Exact Anomaly 1.5.3 DX11 profiles shared by startup, observation and fixtures.
//! Evidence: docs/research/anomaly-native-hooks.md and anomaly-avx-profile.md.
use crate::engine_guards::{NativeSite, UniqueGuard, site};
use std::ffi::CStr;

pub(crate) const HOOK_COUNT: usize = 8;

pub(crate) struct Profile {
    pub(crate) label: &'static CStr,
    pub(crate) executable: &'static CStr,
    pub(crate) sha256: &'static CStr,
    pub(crate) image_size: u32,
    pub(crate) device_width_rva: u32,
    pub(crate) device_height_rva: u32,
    pub(crate) device_frame_rva: u32,
    pub(crate) sites: [NativeSite; HOOK_COUNT],
    pub(crate) guards: [UniqueGuard; 2],
}

// SAFETY: the only raw pointers in the two immutable records point to static
// C string literals. The records are never written through those pointers.
unsafe impl Sync for Profile {}

pub(crate) static PROFILES: [Profile; 2] = [
    // ---- Standard 1.5.3 DX11 ----
    Profile {
        label: c"standard",
        executable: c"AnomalyDX11.exe",
        sha256: c"c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e",
        image_size: 0xfca000,
        device_width_rva: 0xd37668,
        device_height_rva: 0xd3766c,
        device_frame_rva: 0xd37680,
        sites: [
            site(
                c"IRenderDeviceRender::Begin",
                0x822cb0,
                b"40534883ec20488bd9488d0d000f5400",
            ),
            site(
                c"IRenderDeviceRender::End",
                0x822eb0,
                b"4883ec28833d498a4c00007406488b01",
            ),
            site(
                c"IRenderDeviceRender::SetCacheXform",
                0x823250,
                b"40534883ec20488d0dbb095400498bd8",
            ),
            site(
                c"CRender::Render",
                0x8977c0,
                b"488bc448894808535657415441554156",
            ),
            site(
                c"CHUDManager::RenderUI",
                0x0f1e60,
                b"40534883ec40f7051823bf0000100000",
            ),
            site(
                c"CRender::RenderToTarget",
                0x898640,
                b"40534883ec30448bc24183e801744d41",
            ),
            site(
                c"CRender::reset_begin",
                0x83a020,
                b"48894c2408565741564883ec3048c744",
            ),
            site(
                c"CRender::reset_end",
                0x83a1c0,
                b"40574883ec5048895c2468488bf94889",
            ),
        ],
        guards: [
            UniqueGuard::new(
                c"CRender::Render",
                0x8977c0,
                c"488bc44889480853565741544155415641574881ec20",
            ),
            UniqueGuard::new(
                c"CRender::reset_begin",
                0x83a020,
                c"48894c2408565741564883ec3048c7442420feffffff48895c2460488bf9",
            ),
        ],
    },
    // ---- AVX 1.5.3 DX11 ----
    Profile {
        label: c"avx",
        executable: c"AnomalyDX11AVX.exe",
        sha256: c"2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12",
        image_size: 0xfb8000,
        device_width_rva: 0xd25668,
        device_height_rva: 0xd2566c,
        device_frame_rva: 0xd25680,
        sites: [
            site(
                c"IRenderDeviceRender::Begin",
                0x81a930,
                b"40534883ec20488bd9488d0d80725300",
            ),
            site(
                c"IRenderDeviceRender::End",
                0x81ab40,
                b"4883ec28833db9ed4b00007406488b01",
            ),
            site(
                c"IRenderDeviceRender::SetCacheXform",
                0x81aee0,
                b"40534883ec20488d0d2b6d5300498bd8",
            ),
            site(
                c"CRender::Render",
                0x88d420,
                b"488bc448894808535657415441554156",
            ),
            site(
                c"CHUDManager::RenderUI",
                0x0f0810,
                b"40534883ec40f7056819be0000100000",
            ),
            site(
                c"CRender::RenderToTarget",
                0x88e290,
                b"40534883ec30448bc24183e801744d41",
            ),
            site(
                c"CRender::reset_begin",
                0x8312d0,
                b"48894c2408565741564883ec3048c744",
            ),
            site(
                c"CRender::reset_end",
                0x831470,
                b"40574883ec5048895c2468488bf94889",
            ),
        ],
        guards: [
            UniqueGuard::new(
                c"CRender::Render",
                0x88d420,
                c"488bc44889480853565741544155415641574881ec50",
            ),
            UniqueGuard::new(
                c"CRender::reset_begin",
                0x8312d0,
                c"48894c2408565741564883ec3048c7442420feffffff48895c2460488bf9",
            ),
        ],
    },
];

pub(crate) fn find(executable: &[u8], hash: &[u8]) -> Option<usize> {
    PROFILES.iter().position(|profile| {
        profile
            .executable
            .to_bytes()
            .eq_ignore_ascii_case(executable)
            && profile.sha256.to_bytes().eq_ignore_ascii_case(hash)
    })
}
