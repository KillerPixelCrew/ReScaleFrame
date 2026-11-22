// SPDX-License-Identifier: MIT
use super::{Image, NativeSite, UniqueGuard};
use crate::ffi::borrow_span;
use std::ffi::c_char;
use std::{ptr, slice};

#[repr(C)]
struct TessCreateSite {
    name: *const c_char,
    rva: u32,
    prologue: *const c_char,
    wrapper_rva: u32,
    wrapper: *const c_char,
}

#[repr(C)]
struct Profile<'a> {
    sites: &'a [NativeSite; 3],
    unique: *const UniqueGuard,
    unique_count: usize,
    create_shader_calls: &'a [NativeSite; 2],
    tess_create_sites: &'a [TessCreateSite; 2],
    shader_compile_prologue: *const c_char,
    d3dcompile_setup: *const c_char,
    tess_dispatch: *const c_char,
    shader_compile_rva: u32,
    d3dcompile_setup_rva: u32,
    tess_dispatch_rva: u32,
    source_name_rva: u32,
}

const _: () = {
    assert!(size_of::<TessCreateSite>() == 40);
    assert!(std::mem::offset_of!(TessCreateSite, prologue) == 16);
    assert!(std::mem::offset_of!(TessCreateSite, wrapper_rva) == 24);
    assert!(std::mem::offset_of!(TessCreateSite, wrapper) == 32);
    assert!(size_of::<Profile>() == 80);
    assert!(std::mem::offset_of!(Profile, create_shader_calls) == 24);
    assert!(std::mem::offset_of!(Profile, shader_compile_prologue) == 40);
    assert!(std::mem::offset_of!(Profile, shader_compile_rva) == 64);
    assert!(std::mem::offset_of!(Profile, source_name_rva) == 76);
};

/// The facade lends the selected build's immutable records and terminated guard strings.
/// The returned site name is borrowed from the profile or is one of the static names below.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_guard_failure(
    image: &Image,
    profile: &Profile<'_>,
) -> *const c_char {
    let unique = unsafe { borrow_span(profile.unique, profile.unique_count) };
    let hex = |rva, code| unsafe { image.matches_hex_guard(rva, code) };
    // Detour entries first: the 16 expected bytes, then any unique prefix at the same RVA.
    for site in profile.sites {
        if !unsafe { image.matches_sites(slice::from_ref(site), unique) } {
            return site.name;
        }
    }
    // The two create_shader calls whose return addresses admit the caller frame.
    for site in profile.create_shader_calls {
        if !image.matches_site(site.rva, &site.expected) {
            return site.name;
        }
    }
    // The caller frame offsets are read from this prologue and D3DCompile setup.
    if !hex(profile.shader_compile_rva, profile.shader_compile_prologue) {
        return c"CRender::shader_compile prologue".as_ptr();
    }
    if !hex(profile.d3dcompile_setup_rva, profile.d3dcompile_setup) {
        return c"CRender::shader_compile D3DCompile setup".as_ptr();
    }
    // Hull and domain shaders reach the same detour through this dispatch and object store.
    if !hex(profile.tess_dispatch_rva, profile.tess_dispatch) {
        return c"create_shader hull/domain dispatch".as_ptr();
    }
    for site in profile.tess_create_sites {
        if !hex(site.rva, site.prologue) {
            return site.name;
        }
        if !hex(site.wrapper_rva, site.wrapper) {
            return c"CreateHWShader (hull/domain)".as_ptr();
        }
    }
    // RSF recompiles with the engine's empty pSourceName.
    if !image.matches_hex(profile.source_name_rva, b"00") {
        return c"D3DCompile pSourceName".as_ptr();
    }
    ptr::null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString, c_void};

    unsafe extern "C" fn copy(out: *mut c_void, input: *const c_void, size: usize) -> bool {
        unsafe { std::ptr::copy_nonoverlapping(input.cast::<u8>(), out.cast(), size) };
        true
    }

    #[test]
    fn refusals_keep_the_native_order_and_names() {
        let mut bytes: [u8; 0x100] = std::array::from_fn(|i| i as u8);
        bytes[0xd0] = 0;
        let hex = |rva: usize, length: usize| {
            let text: String = bytes[rva..rva + length]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            CString::new(text).unwrap()
        };
        let site = |name: &CStr, rva: u32| NativeSite {
            name: name.as_ptr(),
            rva,
            expected: bytes[rva as usize..rva as usize + 16].try_into().unwrap(),
        };
        let unique_code = hex(0x20, 0x14);
        let (prologue, setup, dispatch) = (hex(0x60, 8), hex(0x70, 8), hex(0x80, 8));
        let tess_code = [hex(0x90, 8), hex(0xa0, 8), hex(0xb0, 8), hex(0xc0, 8)];
        let sites = [site(c"s0", 0), site(c"s1", 0x10), site(c"s2", 0x20)];
        let unique = [UniqueGuard {
            name: c"s2".as_ptr(),
            rva: 0x20,
            expected_hex: unique_code.as_ptr(),
        }];
        let calls = [site(c"c0", 0x40), site(c"c1", 0x50)];
        let tess =
            [(c"hull", 0x90, 0), (c"domain", 0xb0, 2)].map(|(name, rva, code)| TessCreateSite {
                name: name.as_ptr(),
                rva,
                prologue: tess_code[code].as_ptr(),
                wrapper_rva: rva + 0x10,
                wrapper: tess_code[code + 1].as_ptr(),
            });
        let profile = Profile {
            sites: &sites,
            unique: unique.as_ptr(),
            unique_count: unique.len(),
            create_shader_calls: &calls,
            tess_create_sites: &tess,
            shader_compile_prologue: prologue.as_ptr(),
            d3dcompile_setup: setup.as_ptr(),
            tess_dispatch: dispatch.as_ptr(),
            shader_compile_rva: 0x60,
            d3dcompile_setup_rva: 0x70,
            tess_dispatch_rva: 0x80,
            source_name_rva: 0xd0,
        };
        let failure = |bytes: &[u8; 0x100]| {
            let image = Image::new(bytes.as_ptr(), bytes.len() as u32, copy);
            let name = unsafe { rsf_anomaly_motion_guard_failure(&image, &profile) };
            (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) }.to_str().unwrap())
        };
        assert_eq!(failure(&bytes), None);
        // Break checks from last to first: each refusal must name the earliest broken check.
        let order = [
            (0x00, "s0"),
            (0x10, "s1"),
            (0x20, "s2"),
            (0x32, "s2"),
            (0x40, "c0"),
            (0x50, "c1"),
            (0x60, "CRender::shader_compile prologue"),
            (0x70, "CRender::shader_compile D3DCompile setup"),
            (0x80, "create_shader hull/domain dispatch"),
            (0x90, "hull"),
            (0xa0, "CreateHWShader (hull/domain)"),
            (0xb0, "domain"),
            (0xc0, "CreateHWShader (hull/domain)"),
            (0xd0, "D3DCompile pSourceName"),
        ];
        for (offset, name) in order.into_iter().rev() {
            bytes[offset] ^= 0xff;
            assert_eq!(failure(&bytes), Some(name), "offset {offset:#x}");
        }
    }
}
