// SPDX-License-Identifier: MIT
use super::*;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn only_the_game_executable_is_admitted() {
    let is_game = |path: &[u16]| leaf_is(path, layout::EXECUTABLE_NAME);
    assert!(is_game(&units(
        "C:\\Games\\Red Dead Redemption 2\\RDR2.exe"
    )));
    assert!(is_game(&units("D:\\rdr2.EXE")));
    assert!(is_game(&units("RDR2.exe")));
    assert!(!is_game(&units("C:\\Games\\RDR2.exe\\PlayRDR2.exe")));
    assert!(!is_game(&units("C:\\Games\\RDR2.exe.bak")));
    assert!(!is_game(&[]));
}

#[test]
fn configuration_sits_beside_the_asi() {
    let (root, configuration) = deployment(&units("C:\\Game\\ReScaleFrame.RDR2.asi")).unwrap();
    assert_eq!(root, units("C:\\Game\\"));
    assert_eq!(configuration, wide("C:\\Game\\ReScaleFrame.ini"));
    assert!(deployment(&units("ReScaleFrame.RDR2.asi")).is_none());
    // The configuration path must still fit the 32768-unit profile buffer.
    let mut long = vec![u16::from(b'a'); LONG_PATH as usize - 8];
    long[LONG_PATH as usize - 10] = u16::from(b'\\');
    assert!(deployment(&long).is_none());
}

#[test]
fn read_sites_resolve_to_the_api_parameter() {
    let (parameter, site) = (0x57a_bf10usize, 0x2c6_5575usize);
    for register in [0x05, 0x0d, 0x1d] {
        let mut bytes = [0x48, 0x8b, register, 0, 0, 0, 0];
        bytes[3..].copy_from_slice(&((parameter - site - 7) as u32).to_le_bytes());
        assert_eq!(discovery::parameter_read(site, bytes), Some(parameter));
        // A register operand or another opcode is not a RIP-relative load.
        let mut other = bytes;
        other[2] ^= 1;
        assert_eq!(discovery::parameter_read(site, other), None);
        other = bytes;
        other[1] = 0x89;
        assert_eq!(discovery::parameter_read(site, other), None);
    }
}
