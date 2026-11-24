// SPDX-License-Identifier: MIT
use super::Image;
use crate::motion_profile::MotionProfile;
use std::ffi::{CStr, c_char};
use std::slice;

/// The name of the first mismatching check of the selected build, or None. Names come from the static profile or
/// are the static names below.
pub(crate) fn motion_guard_failure(image: &Image, profile: &MotionProfile) -> Option<&'static CStr> {
    // The profile records hold static NUL-terminated names.
    let name = |name: *const c_char| Some(unsafe { CStr::from_ptr(name) });
    let hex = |rva, code: &std::ffi::CStr| unsafe { image.matches_hex_guard(rva, code.as_ptr()) };
    // Detour entries first: the 16 expected bytes, then any unique prefix at the same RVA.
    for site in &profile.sites {
        if !unsafe { image.matches_sites(slice::from_ref(site), &profile.unique_guards) } {
            return name(site.name);
        }
    }
    // The two create_shader calls whose return addresses admit the caller frame.
    for site in &profile.create_shader_calls {
        if !image.matches_site(site.rva, &site.expected) {
            return name(site.name);
        }
    }
    // The caller frame offsets are read from this prologue and D3DCompile setup.
    if !hex(profile.shader_compile_rva, profile.shader_compile_prologue) {
        return Some(c"CRender::shader_compile prologue");
    }
    if !hex(profile.d3dcompile_setup_rva, profile.d3dcompile_setup) {
        return Some(c"CRender::shader_compile D3DCompile setup");
    }
    // Hull and domain shaders reach the same detour through this dispatch and object store.
    if !hex(profile.tess_dispatch_rva, profile.tess_dispatch) {
        return Some(c"create_shader hull/domain dispatch");
    }
    for site in &profile.tess_create_sites {
        if !hex(site.rva, site.prologue) {
            return Some(site.name);
        }
        if !hex(site.wrapper_rva, site.wrapper) {
            return Some(c"CreateHWShader (hull/domain)");
        }
    }
    // RSF recompiles with the engine's empty pSourceName.
    if !image.matches_hex(profile.source_name_rva, b"00") {
        return Some(c"D3DCompile pSourceName");
    }
    None
}
#[cfg(test)]
mod tests {
    use super::super::{NativeSite, UniqueGuard};
    use super::*;
    use crate::motion_profile::{CallerFrame, TessCreateSite};
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
        let leak = |text: CString| -> &'static CStr { Box::leak(text.into_boxed_c_str()) };
        let unique_code = leak(hex(0x20, 0x14));
        let (prologue, setup, dispatch) =
            (leak(hex(0x60, 8)), leak(hex(0x70, 8)), leak(hex(0x80, 8)));
        let tess_code = [0x90, 0xa0, 0xb0, 0xc0].map(|rva| leak(hex(rva, 8)));
        let sites = [site(c"s0", 0), site(c"s1", 0x10), site(c"s2", 0x20)];
        let calls = [site(c"c0", 0x40), site(c"c1", 0x50)];
        let tess =
            [(c"hull", 0x90, 0), (c"domain", 0xb0, 2)].map(|(name, rva, code)| TessCreateSite {
                name,
                rva,
                prologue: tess_code[code],
                wrapper_rva: rva + 0x10,
                wrapper: tess_code[code + 1],
            });
        let profile = MotionProfile {
            sites,
            unique_guards: [UniqueGuard {
                name: c"s2".as_ptr(),
                rva: 0x20,
                expected_hex: unique_code.as_ptr(),
            }],
            shader_compile_rva: 0x60,
            shader_compile_prologue: prologue,
            create_shader_calls: calls,
            frame: CallerFrame {
                source_offset: 0,
                length_offset: 0,
                entry_offset: 0,
                flags_offset: 0,
                defines_offset: 0,
            },
            d3dcompile_setup_rva: 0x70,
            d3dcompile_setup: setup,
            source_name_rva: 0xd0,
            tess_dispatch_rva: 0x80,
            tess_dispatch: dispatch,
            tess_create_sites: tess,
        };
        let failure = |bytes: &[u8; 0x100]| {
            let image = Image::new(bytes.as_ptr(), bytes.len() as u32, copy);
            motion_guard_failure(&image, &profile).map(|name| name.to_str().unwrap())
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
