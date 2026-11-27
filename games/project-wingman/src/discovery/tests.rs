// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::pe32_plus::Pe32Plus;

/// PE timestamp of build 16671873 (2024-12-08 09:44:06 UTC), whose RVAs are recorded.
const RECORDED_TIMESTAMP: u32 = 0x6755_6a66;

/// The build 16671873 RVAs, indexed by [`Site`]: documentation and the executable test.
const RECORDED: [usize; SITES] = [
    0x5c4_42b0, 0x538_21d0, 0x5c4_4910, 0x5c2_d4a0, 0x656_9660, 0x5c4_49d8, 0x5c4_4ecc, 0x5c4_120b,
    0x5c4_1065, 0x839_0f78, 0x83f_6838, 0x591_d000, 0x5c4_c9c0,
];

/// The view fields of build 16671873 (docs/research/project-wingman-renderer.md).
const RECORDED_VIEW: View = View {
    camera_cut: 0xd90,
    jitter: 0x4fa4,
    state: 0x1680,
    rect: 0x1670,
};

#[test]
fn every_pattern_parses() {
    for signature in SIGNATURES.iter().chain(&HELPER) {
        assert!(
            Pattern::parse(signature.pattern).is_some(),
            "{}",
            signature.name
        );
        assert!(
            signature.check.is_empty() || Pattern::parse(signature.check).is_some(),
            "{}",
            signature.name
        );
    }
    assert!(Pattern::parse(DESTRUCTOR).is_some() && Pattern::parse(EXECUTE_ENTRY).is_some());
}

#[test]
fn unresolved_sites_are_zero_and_never_verify() {
    // Unit tests never admit, so nothing resolves.
    assert_eq!(rva(Site::AddPasses), 0);
    assert_eq!(view(), View::default());
    assert!(!verify(0x1_4000_0000, Site::AddPasses));
}

/// Every pattern resolves uniquely in the installed executable, and to the recorded RVA when it is
/// the recorded build. Skipped when the game is not installed; `RSF_PROJECT_WINGMAN_EXE` names
/// another copy, such as one recovered from a Ghidra project by tools/ghidra/file-bytes.py.
#[test]
fn every_site_resolves_in_the_installed_executable() {
    let path = std::env::var_os("RSF_PROJECT_WINGMAN_EXE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            signature::steam_file(
                "Project Wingman\\ProjectWingman\\Binaries\\Win64\\ProjectWingman-Win64-Shipping.exe",
            )
        });
    let Some(path) = path else {
        eprintln!("skipped: ProjectWingman-Win64-Shipping.exe is not installed");
        return;
    };
    let bytes = std::fs::read(&path).expect("read the executable");
    let image = Pe32Plus::parse(&bytes).expect("PE32+ executable");
    let code = image
        .sections(false)
        .into_iter()
        .filter(|(_, _, flags, _)| flags & EXECUTE != 0)
        .map(|(_, rva, _, bytes)| Section { rva, bytes })
        .collect::<Vec<_>>();
    let read = |rva: usize, out: &mut [u8]| {
        image
            .span(rva, out.len())
            .map(|(bytes, _)| out.copy_from_slice(bytes))
            .is_some()
    };
    let found = resolve_image(&code, image.preferred_base(), read)
        .unwrap_or_else(|refusal| panic!("{} in {}", refusal, path.display()));
    let sites = found.sites;
    for (index, signature) in SIGNATURES.iter().enumerate() {
        if let Some((text, start)) = signature.guard(sites[index]) {
            let pattern = Pattern::parse(text).unwrap();
            let (bytes, _) = image.span(start, pattern.len()).unwrap();
            assert!(pattern.matches(bytes), "{} guard", signature.name);
        }
    }
    if image.timestamp() != RECORDED_TIMESTAMP {
        eprintln!(
            "{} is not the recorded build (timestamp {:#x}); every site resolved uniquely",
            path.display(),
            image.timestamp()
        );
        return;
    }
    // The recorded helper registers its pass by tail-calling the discovered setup at +199.
    let helper = sites[Site::AddCompute as usize];
    let (jump, _) = image.span(helper + 0x199, 5).unwrap();
    let displacement = i32::from_le_bytes(jump[1..].try_into().unwrap()) as isize;
    assert_eq!(jump[0], 0xe9, "helper tail jump");
    assert_eq!(
        (helper + 0x19e).checked_add_signed(displacement),
        Some(sites[Site::SetupPass as usize])
    );
    assert_eq!(found.view, RECORDED_VIEW);
    for (index, (&found, &recorded)) in sites.iter().zip(&RECORDED).enumerate() {
        assert_eq!(
            found, recorded,
            "site {index} at {found:#x}, recorded {recorded:#x}"
        );
    }
}
