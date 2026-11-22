// SPDX-License-Identifier: MIT
//! Measured capability, collection and refresh vectors for the native settings policy.
use super::*;

fn original() -> [u8; collection::BYTES] {
    let mut data = [0x5a; collection::BYTES];
    data[collection::VERSION..collection::VERSION + 2].copy_from_slice(&37u16.to_le_bytes());
    data
}

#[test]
fn dlss_choice_follows_the_capability_list() {
    // The measured active list omits unsupported Ultra Quality 4, while the freshly constructed
    // pending owner initially contains it. Indices move.
    let active = [5, 6, 2, 1, 0, 3];
    let initial = [5, 6, 4, 2, 1, 0, 3];
    let active_indices = [2, 2, 3, 4, 5];
    let initial_indices = [3, 3, 4, 5, 6];
    for quality in 0..5 {
        assert_eq!(
            dlss_choice(&active, quality),
            Some(active_indices[quality as usize])
        );
        assert_eq!(
            dlss_choice(&initial, quality),
            Some(initial_indices[quality as usize])
        );
    }
    assert_eq!(dlss_choice(&[], 1), None);
    assert_eq!(dlss_choice(&[5, 6, 2, 1, 0, 3, 4, 7], 1), None);
    assert_eq!(dlss_choice(&active, 5), None);
    assert_eq!(dlss_choice(&[5, 6], 1), None);
    assert_eq!(
        unsafe { rsf_rdr2_dlss_choice_abi(ptr::null(), 6, 1) },
        u32::MAX
    );
    assert_eq!(
        unsafe { rsf_rdr2_dlss_choice_abi(active.as_ptr(), 6, 3) },
        4
    );
}

#[test]
fn pin_keeps_a_carrier_for_every_vendor_and_quality() {
    for vendor in [NVIDIA, 0x1002, 0x8086, 0] {
        let nvidia = vendor == NVIDIA;
        for quality in 0..5usize {
            let mut data = original();
            let mut wanted = original();
            put(
                &mut wanted,
                collection::DLSS,
                if nvidia { [2, 2, 3, 4, 5][quality] } else { 0 },
            );
            put(
                &mut wanted,
                collection::DLSS_QUALITY,
                if nvidia { [2, 2, 1, 0, 3][quality] } else { 5 },
            );
            put(
                &mut wanted,
                collection::FSR,
                if nvidia { 0 } else { [1, 1, 2, 3, 3][quality] },
            );
            put(&mut wanted, collection::REFLEX, 1);
            assert!(pin(&mut data, vendor, quality as u32));
            assert_eq!(data, wanted);
            // A native Off selection is replaced again.
            put(
                &mut data,
                if nvidia {
                    collection::DLSS
                } else {
                    collection::FSR
                },
                0,
            );
            assert!(pin(&mut data, vendor, quality as u32));
            assert_eq!(data, wanted);
        }
    }
}

#[test]
fn pin_refuses_other_objects_unchanged() {
    let mut defaults = original();
    defaults[collection::VERSION..collection::VERSION + 2].copy_from_slice(&0u16.to_le_bytes());
    assert!(pin(&mut defaults, 0x1002, DEFAULT_QUALITY));
    let mut invalid = original();
    invalid[collection::VERSION..collection::VERSION + 4].copy_from_slice(&1u32.to_le_bytes());
    let unchanged = invalid;
    assert!(!pin(&mut invalid, NVIDIA, DEFAULT_QUALITY));
    assert_eq!(invalid, unchanged);
    assert!(!pin(&mut invalid, NVIDIA, 5));
    let mut short = original();
    assert!(!pin(
        &mut short[..collection::BYTES - 1],
        NVIDIA,
        DEFAULT_QUALITY
    ));
    assert!(!unsafe {
        rsf_rdr2_pin_engine_settings_abi(ptr::null_mut(), collection::BYTES, NVIDIA, 2)
    });
}

#[test]
fn refresh_rationals_stay_storable() {
    // Measured Arc B370 panel: 120 Hz is 31550000/262912, which RAGE saved as 262144/262144.
    let (mut n, mut d) = (31_550_000, 262_912);
    assert!(unsafe { rsf_rdr2_storable_refresh_abi(&mut n, &mut d, 120) });
    assert_eq!((n, d), (12000, 100));
    (n, d) = (REFRESH_LIMIT, REFRESH_LIMIT);
    assert!(storable_refresh(&mut n, &mut d, 120));
    assert_eq!((n, d), (12000, 100));
    (n, d) = (REFRESH_LIMIT, REFRESH_LIMIT);
    assert!(!storable_refresh(&mut n, &mut d, 0));
    assert_eq!(n, REFRESH_LIMIT);
    for mode in [(3000, 100), (6000, 100), (120, 1), (0, 0)] {
        (n, d) = mode;
        assert!(!storable_refresh(&mut n, &mut d, 120));
        assert_eq!((n, d), mode);
    }
    (n, d) = (400_000, 0);
    assert!(!storable_refresh(&mut n, &mut d, 120));
    assert!(!unsafe { rsf_rdr2_storable_refresh_abi(ptr::null_mut(), &mut d, 120) });
}

#[test]
fn failed_dxgi_query_can_retry_the_same_index() {
    let cache = AtomicU64::new(u64::MAX);
    assert_eq!(adapter_vendor(&cache, 2, |_| 0), 0);
    assert_eq!(
        adapter_vendor(&cache, 2, |index| {
            assert_eq!(index, 2);
            NVIDIA
        }),
        NVIDIA
    );
    assert_eq!(
        adapter_vendor(&cache, 2, |_| panic!("successful lookup should be cached")),
        NVIDIA
    );
}

#[test]
fn exported_pin_writes_the_measured_collection_offsets() {
    // Keep this layout fixture independent of the Rust offset constants.
    let mut data = [0x5a; 0x180];
    data[0x178..0x17a].copy_from_slice(&37u16.to_le_bytes());
    let mut expected = data;
    for (offset, value) in [(0xe8, 3), (0xec, 1), (0xf4, 0), (0x15c, 1)] {
        put(&mut expected, offset, value);
    }
    assert!(unsafe {
        rsf_rdr2_pin_engine_settings_abi(data.as_mut_ptr().cast(), data.len(), 0x10de, 2)
    });
    assert_eq!(data, expected);
}
