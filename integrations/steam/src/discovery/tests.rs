// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::pe32_plus::Pe32Plus;

/// Linker timestamp of the 11.23.65.88 overlay whose RVAs are recorded.
const RECORDED_TIMESTAMP: u32 = 0x6af6_6f0a;

/// The 11.23.65.88 RVAs.
const RECORDED: Sites = Sites {
    loads: [
        0x9c32f, 0x9c375, 0x9c154, 0x9c23e, 0x9c6cf, 0x9c5bc, 0x9bf72, 0x9c05d, 0x9c630, 0x9c4df,
    ],
    entries: [0x9c130, 0x9bf50],
    slots: [
        0x1834b0, 0x1834b8, 0x1834c8, 0x1834c0, 0x1834d0, 0x1834d8, 0x1834e0,
    ],
};

#[test]
fn every_pattern_parses_and_every_method_has_a_load() {
    for signature in &SIGNATURES {
        assert!(
            Pattern::parse(signature.pattern).is_some(),
            "{}",
            signature.name
        );
    }
    for method in 1..=METHODS as u32 {
        assert!(LOAD_METHODS.contains(&method), "method {method}");
    }
}

/// Every site resolves uniquely in the overlay installed with Steam, and to the recorded RVAs
/// when it is the recorded build. Skipped when Steam is not installed; `RSF_STEAM_OVERLAY` names
/// another copy.
#[test]
fn every_site_resolves_in_the_installed_overlay() {
    let path = std::env::var_os("RSF_STEAM_OVERLAY")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let steam = std::path::PathBuf::from(std::env::var_os("ProgramFiles(x86)")?);
            Some(steam.join("Steam").join("GameOverlayRenderer64.dll"))
        })
        .filter(|path| path.is_file());
    let Some(path) = path else {
        eprintln!("skipped: GameOverlayRenderer64.dll is not installed");
        return;
    };
    let bytes = std::fs::read(&path).expect("read the installed overlay");
    let image = Pe32Plus::parse(&bytes).expect("PE32+ module");
    let sections = image.sections(false);
    let code = sections
        .iter()
        .filter(|section| section.2 & EXECUTE != 0)
        .map(|&(_, rva, _, bytes)| Section { rva, bytes })
        .collect::<Vec<_>>();
    let read = |rva: usize, out: &mut [u8]| {
        image
            .span(rva, out.len())
            .map(|(bytes, _)| out.copy_from_slice(bytes))
            .is_some()
    };
    let sites =
        resolve(&code, read).unwrap_or_else(|refusal| panic!("{} in {}", refusal, path.display()));
    if image.timestamp() != RECORDED_TIMESTAMP {
        eprintln!(
            "{} is not the recorded build (timestamp {:#x}); every site resolved uniquely",
            path.display(),
            image.timestamp()
        );
        return;
    }
    assert_eq!(sites, RECORDED);
}
