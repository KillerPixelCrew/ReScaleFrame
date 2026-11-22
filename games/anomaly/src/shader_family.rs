// SPDX-License-Identifier: MIT
//! Stable shader-family IDs used by admission, draw policy and counters.
#[cfg(windows)]
use std::ffi::{CStr, c_char};

macro_rules! families {
    ($(($id:ident, $number:literal, $name:literal, $stage:expr)),+ $(,)?) => {
        $(
            #[cfg_attr(not(windows), allow(dead_code))]
            pub(crate) const $id: u8 = $number;
        )+
        #[cfg(windows)]
        pub(crate) const FAMILIES: &[(u8, &CStr, u8)] = &[$(($id, $name, $stage)),+];
    };
}

families! {
    (NO_FAMILY, 0, c"none", 0),
    (CORE_STATIC, 1, c"core_static", b'v'),
    (CORE_MODEL, 2, c"core_model", b'v'),
    (CORE_PIXEL, 3, c"core_pixel", b'p'),
    (FOLIAGE_TREE, 4, c"foliage_tree", b'v'),
    (FOLIAGE_DETAIL, 5, c"foliage_detail", b'v'),
    (TESS_HULL, 6, c"tess_hull", b'h'),
    (TESS_DOMAIN, 7, c"tess_domain", b'd'),
}

#[cfg(windows)]
pub(crate) const FAMILY_COUNT: usize = FAMILIES.len();
#[cfg(windows)]
const _: () = {
    // Counters are indexed by stable IDs, so the IDs must be dense and unique.
    let mut seen = [false; FAMILY_COUNT];
    let mut index = 0;
    while index < FAMILY_COUNT {
        let id = FAMILIES[index].0 as usize;
        assert!(!seen[id]);
        seen[id] = true;
        index += 1;
    }
};

#[cfg(windows)]
fn family(id: u8) -> Option<&'static (u8, &'static CStr, u8)> {
    FAMILIES.iter().find(|row| row.0 == id)
}

#[cfg(windows)]
fn family_name(id: u8) -> &'static CStr {
    family(id).map_or(c"unknown", |row| row.1)
}

#[cfg(windows)]
pub(crate) fn diagnostic_family_name(id: u8) -> &'static str {
    if id == NO_FAMILY {
        return "other";
    }
    family_name(id).to_str().expect("family names are ASCII")
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_family_name(id: u8) -> *const c_char {
    family_name(id).as_ptr()
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_shader_family_stage(id: u8) -> u8 {
    family(id).map_or(0, |row| row.2)
}
