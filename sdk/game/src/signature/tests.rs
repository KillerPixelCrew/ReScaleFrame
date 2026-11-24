// SPDX-License-Identifier: MIT
use super::*;

fn signature(pattern: &'static str, find: Find) -> Signature {
    Signature {
        name: "site",
        pattern,
        find,
        check: "",
    }
}

#[test]
fn parse_accepts_hex_and_wildcards_and_refuses_the_rest() {
    let pattern = Pattern::parse("48 8b ?? 0F").unwrap();
    assert_eq!(pattern.0, [Some(0x48), Some(0x8b), None, Some(0x0f)]);
    assert_eq!(pattern.len(), 4);
    for invalid in ["", "48 8", "48 GG", "?? 48 ?? 8B", "488B", "48 8B ?"] {
        assert!(Pattern::parse(invalid).is_none(), "{invalid:?}");
    }
}

#[test]
fn wildcards_match_any_byte_and_fixed_bytes_must_agree() {
    let pattern = Pattern::parse("E8 ?? ?? ?? ?? 48 8B").unwrap();
    assert!(pattern.matches(&[0xe8, 1, 2, 3, 4, 0x48, 0x8b, 0xff]));
    assert!(pattern.matches(&[0xe8, 9, 9, 9, 9, 0x48, 0x8b]));
    assert!(!pattern.matches(&[0xe8, 1, 2, 3, 4, 0x48, 0x8c]));
    assert!(!pattern.matches(&[0xe8, 1, 2, 3, 4, 0x48]));
}

#[test]
fn every_pattern_must_match_exactly_once_across_sections() {
    let first = [0xcc, 0x40, 0x53, 0x48, 0x83, 0xcc, 0x40, 0x53, 0x90];
    let second = [0x90, 0x40, 0x53, 0x48, 0x83];
    let sections = [
        Section {
            rva: 0x1000,
            bytes: &first,
        },
        Section {
            rva: 0x8000,
            bytes: &second,
        },
    ];
    let unique = signature("53 90", Find::At(0));
    assert_eq!(resolve(&sections, &[unique]), Ok(vec![0x1007]));
    let ambiguous = signature("40 53 48 83", Find::At(1));
    assert_eq!(
        resolve(&sections, &[unique, ambiguous]),
        Err(Refusal {
            name: "site",
            miss: Miss::Ambiguous
        })
    );
    let missing = signature("40 53 ?? 84", Find::At(0));
    assert_eq!(
        resolve(&sections, &[missing]).unwrap_err().miss,
        Miss::Missing
    );
    let invalid = signature("4", Find::At(0));
    assert_eq!(
        resolve(&sections, &[invalid]).unwrap_err().miss,
        Miss::Pattern
    );
    // A match may straddle nothing: section boundaries split candidates.
    let straddle = signature("53 90 90 40", Find::At(0));
    assert_eq!(
        resolve(&sections, &[straddle]).unwrap_err().miss,
        Miss::Missing
    );
    let refusal = Refusal {
        name: "jitter",
        miss: Miss::Ambiguous,
    };
    assert_eq!(refusal.to_string(), "site 'jitter' matched more than once");
}

#[test]
fn rip_relative_operands_decode_from_the_end_of_their_instruction() {
    // mov rax,[rip+0x20]; mov byte [rip-0x10],1 (imm8 after the displacement); call -0x30
    let code = [
        0x90, 0x48, 0x8b, 0x05, 0x20, 0, 0, 0, 0xc6, 0x05, 0xf0, 0xff, 0xff, 0xff, 0x01, 0xe8,
        0xd0, 0xff, 0xff, 0xff,
    ];
    let sections = [Section {
        rva: 0x4000,
        bytes: &code,
    }];
    let signatures = [
        signature("48 8B 05 ?? ?? ?? ?? C6", Find::Rip { disp: 3, next: 7 }),
        signature("C6 05 ?? ?? ?? ?? 01", Find::Rip { disp: 2, next: 7 }),
        signature("01 E8 ?? ?? ?? ??", Find::Rip { disp: 2, next: 6 }),
    ];
    assert_eq!(
        resolve(&sections, &signatures),
        Ok(vec![
            0x4001 + 7 + 0x20,
            0x4008 + 7 - 0x10,
            0x400e + 6 - 0x30
        ])
    );
    // The displacement must lie inside the pattern and before the instruction end.
    let outside = signature("48 8B 05 ??", Find::Rip { disp: 3, next: 7 });
    assert_eq!(
        resolve(&sections, &[outside]).unwrap_err().miss,
        Miss::Operand
    );
}

#[test]
fn guard_is_the_check_pattern_or_the_finding_pattern_at_its_match() {
    let at = signature("E8 ?? ?? ?? ?? 48 8B", Find::At(5));
    assert_eq!(at.guard(0x105), Some(("E8 ?? ?? ?? ?? 48 8B", 0x100)));
    let global = signature("48 8B 05 ?? ?? ?? ??", Find::Rip { disp: 3, next: 7 });
    assert_eq!(global.guard(0x9000), None);
    let called = Signature {
        check: "40 53 48 83",
        ..global
    };
    assert_eq!(called.guard(0x9000), Some(("40 53 48 83", 0x9000)));
}

#[test]
fn unique_bytes_and_rip_references_are_found_once() {
    let data = *b"Post process\0Poll controls\0Poll";
    let sections = [Section {
        rva: 0x2000,
        bytes: &data,
    }];
    assert_eq!(find_unique(&sections, b"Poll controls\0"), Ok(0x200d));
    assert_eq!(find_unique(&sections, b"Poll"), Err(Miss::Ambiguous));
    assert_eq!(find_unique(&sections, b"Jobs"), Err(Miss::Missing));
    // Two import thunks; only the second jumps through the slot at 0x9000.
    let mut thunks = [0xff, 0x25, 0, 0, 0, 0, 0xff, 0x25, 0, 0, 0, 0];
    thunks[8..12].copy_from_slice(&(0x9000i32 - 0x300c).to_le_bytes());
    let code = [Section {
        rva: 0x3000,
        bytes: &thunks,
    }];
    assert_eq!(find_rip_reference(&code, &[0xff, 0x25], 0x9000), Ok(0x3006));
    assert_eq!(
        find_rip_reference(&code, &[0xff, 0x25], 0x9008),
        Err(Miss::Missing)
    );
}

#[test]
fn import_slot_follows_the_named_lookup_table() {
    let mut image = vec![0u8; 0x400];
    let put32 = |image: &mut Vec<u8>, at: usize, value: u32| {
        image[at..at + 4].copy_from_slice(&value.to_le_bytes())
    };
    put32(&mut image, 0x3c, 0x80);
    put32(&mut image, 0x80 + 24 + 120, 0x200);
    put32(&mut image, 0x80 + 24 + 124, 40);
    // One descriptor: lookup 0x240, name 0x300, IAT 0x280; then the terminator.
    put32(&mut image, 0x200, 0x240);
    put32(&mut image, 0x20c, 0x300);
    put32(&mut image, 0x210, 0x280);
    image[0x300..0x30a].copy_from_slice(b"D3D11.dll\0");
    image[0x240..0x248].copy_from_slice(&(1u64 << 63 | 7).to_le_bytes());
    image[0x248..0x250].copy_from_slice(&0x320u64.to_le_bytes());
    image[0x322..0x340].copy_from_slice(b"D3D11CreateDeviceAndSwapChain\0");
    let read = |rva: usize, out: &mut [u8]| {
        image
            .get(rva..rva + out.len())
            .map(|bytes| out.copy_from_slice(bytes))
            .is_some()
    };
    assert_eq!(
        import_slot(read, "d3d11.dll", "D3D11CreateDeviceAndSwapChain"),
        Some(0x288)
    );
    assert_eq!(import_slot(read, "d3d11.dll", "D3D11CreateDevice"), None);
    assert_eq!(import_slot(read, "dxgi.dll", "CreateDXGIFactory"), None);
}
