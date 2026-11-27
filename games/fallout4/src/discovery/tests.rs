// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::pe32_plus::Pe32Plus;

/// PE timestamp of the 1.11.240.0 build whose RVAs are recorded.
const RECORDED_TIMESTAMP: u32 = 0x6a72_7fcc;

#[test]
fn every_pattern_parses_and_every_site_is_recorded_once() {
    for signature in &SIGNATURES {
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
    for (site, text, _) in CHECKS {
        assert!(Pattern::parse(text).is_some(), "{}", name(site));
    }
    let mut recorded = RECORDED.to_vec();
    recorded.sort_unstable();
    recorded.dedup();
    assert_eq!(recorded.len(), SITES);
}

#[test]
fn unresolved_sites_are_zero_and_never_verify() {
    // Unit tests never discover, so nothing resolves.
    assert_eq!(rva(Site::Swap), 0);
    assert_eq!(address(Site::Swap), 0);
    assert!(!verify(Site::Swap) && !verify_all(&[Site::Main]));
}

/// Every site resolves uniquely in the installed Fallout4.exe, every link and check holds, and
/// each site is at its recorded RVA when it is the recorded build. Skipped when the game is not
/// installed; `RSF_FALLOUT4_EXE` names another copy.
#[test]
fn every_site_resolves_in_the_installed_executable() {
    let path = std::env::var_os("RSF_FALLOUT4_EXE")
        .map(std::path::PathBuf::from)
        .or_else(|| signature::steam_file("Fallout 4\\Fallout4.exe"));
    let Some(path) = path else {
        eprintln!("skipped: Fallout4.exe is not installed");
        return;
    };
    let bytes = std::fs::read(&path).expect("read the installed executable");
    let image = Pe32Plus::parse(&bytes).expect("PE32+ executable");
    let sections = image.sections(false);
    let views = |keep: &dyn Fn(&[u8; 8], u32) -> bool| {
        sections
            .iter()
            .filter(|(name, _, flags, _)| keep(name, *flags))
            .map(|&(_, rva, _, bytes)| Section { rva, bytes })
            .collect::<Vec<_>>()
    };
    let code = views(&|_, flags| flags & EXECUTE != 0);
    let data = views(&|name, flags| {
        flags & EXECUTE == 0 && (name == b".rdata\0\0" || name == b".data\0\0\0")
    });
    // Headers precede the first section at the same offset in the file and the image.
    let headers = sections.iter().map(|section| section.1).min().unwrap_or(0);
    let read = |rva: usize, out: &mut [u8]| {
        image
            .span(rva, out.len())
            .map(|(bytes, _)| bytes)
            .or_else(|| bytes.get(rva..rva + out.len()).filter(|_| rva < headers))
            .map(|bytes| out.copy_from_slice(bytes))
            .is_some()
    };
    let sites = resolve_image(&code, &data, image.preferred_base(), read)
        .unwrap_or_else(|refusal| panic!("{} in {}", refusal, path.display()));
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
    for (index, (&found, &recorded)) in sites.iter().zip(&RECORDED).enumerate() {
        let site = match SIGNATURES.get(index) {
            Some(signature) => signature.name,
            None => DERIVED[index - PATTERN_SITES],
        };
        assert_eq!(
            found, recorded,
            "site '{site}' at {found:#x}, recorded {recorded:#x}"
        );
    }
}
