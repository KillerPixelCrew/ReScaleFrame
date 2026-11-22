// SPDX-License-Identifier: MIT
//! Verified CInput profiles for the two admitted Anomaly DX11 executables.
use super::InputProfile;
use crate::engine_guards::NativeSite;
use std::ffi::CStr;

// OnFrame tests pKeyboard +0x28 before KeyUpdate, then pMouse +0x20 before MouseUpdate.
const fn site(name: &'static CStr, rva: u32, hex: &[u8; 32]) -> NativeSite {
    const fn digit(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("invalid CInput guard digit"),
        }
    }
    let mut expected = [0; 16];
    let mut i = 0;
    while i < expected.len() {
        expected[i] = digit(hex[2 * i]) << 4 | digit(hex[2 * i + 1]);
        i += 1;
    }
    NativeSite {
        name: name.as_ptr(),
        rva,
        expected,
    }
}

// Bootstrap passes the index from the exact executable name and hash match.
pub(super) const fn for_build(build: usize) -> Option<InputProfile> {
    match build {
        // ---- Standard 1.5.3 DX11 ----
        0 => Some(InputProfile {
            sites: [
                site(c"CInput::OnFrame", 0x67810, b"48895c2408574883ec20833d0f81cd00"),
                site(c"CInput::KeyUpdate", 0x66db0, b"4c8bdc534881ec40060000833d7a10cd"),
                site(c"CInput::MouseUpdate", 0x67020, b"4c8bdc534881ec80060000488bd941c7"),
            ],
            slot_rva: 0xa55a20,
            frame_code: c"48895c2408574883ec20833d0f81cd0000488bf9488b1dbd02cd007432ff833c0a000080bb100a0000007523e87f780300482b83080a0000488983f809000033c0488983200a0000488983280a0000488d0dd201cd00e84558fcff4869c0e803000033d248f735857fcd000305dbffcc0048837f28008987ec0b00007408488bcfe81af5ffff48837f20007408488bcfe87bf7ffff833d8480cd0000488b1d3502cd007413488d8bf8090000e8ef57fcff480183300a0000488b5c24304883c4205fc3".as_ptr(),
        }),
        // ---- AVX 1.5.3 DX11 ----
        1 => Some(InputProfile {
            sites: [
                site(c"CInput::OnFrame", 0x66540, b"48895c2408574883ec20833ddf73cc00"),
                site(c"CInput::KeyUpdate", 0x65ae0, b"4c8bdc534881ec40060000833d4a03cc"),
                site(c"CInput::MouseUpdate", 0x65d50, b"4c8bdc534881ec80060000488bd941c7"),
            ],
            slot_rva: 0xa42a60,
            frame_code: c"48895c2408574883ec20833ddf73cc0000488bf9488b1d8df5cb007432ff833c0a000080bb100a0000007523e81f770300482b83080a0000488983f809000033c0488983200a0000488983280a0000488d0da2f4cb00e8356afcff4869c0e803000033d248f7355572cc000305abf2cb0048837f28008987ec0b00007408488bcfe81af5ffff48837f20007408488bcfe87bf7ffff833d5473cc0000488b1d05f5cb007413488d8bf8090000e8df69fcff480183300a0000488b5c24304883c4205fc3".as_ptr(),
        }),
        _ => None,
    }
}
