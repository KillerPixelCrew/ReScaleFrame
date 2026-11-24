// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::pe32_plus::Pe32Plus;

/// PE timestamp of the 1.7.104.0 build whose RVAs are recorded.
const RECORDED_TIMESTAMP: u32 = 0x6a8c_7046;

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
    assert!(Pattern::parse(NAMED_POLL).is_some() && Pattern::parse(CREATE_THUNK).is_some());
    let mut recorded = RECORDED.to_vec();
    recorded.sort_unstable();
    recorded.dedup();
    assert_eq!(recorded.len(), SITES);
}

#[test]
fn unresolved_sites_are_zero_and_never_verify() {
    // Unit tests never admit, so nothing resolves.
    assert_eq!(rva(Site::Jitter), 0);
    assert!(!verify(&Platform, 0x1_4000_0000, Site::Jitter));
    assert!(refusal().is_null());
}

/// Every pattern resolves uniquely in the installed SkyrimSE.exe, and to the recorded RVA when
/// it is the recorded build. Skipped when the game is not installed; `RSF_SKYRIM_SE_EXE` names
/// another copy.
#[test]
fn every_site_resolves_in_the_installed_executable() {
    let path = std::env::var_os("RSF_SKYRIM_SE_EXE")
        .map(std::path::PathBuf::from)
        .or_else(|| signature::steam_file("Skyrim Special Edition\\SkyrimSE.exe"));
    let Some(path) = path else {
        eprintln!("skipped: SkyrimSE.exe is not installed");
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
    // The unpatched creation call reaches the thunk found from the import table.
    let create = sites[Site::CreateCall as usize];
    let (call, _) = image.span(create, 5).unwrap();
    assert_eq!(
        rip_target(create, call, 1, 5),
        Some(sites[Site::CreateThunk as usize])
    );
    if image.timestamp() != RECORDED_TIMESTAMP {
        eprintln!(
            "{} is not the recorded build (timestamp {:#x}); every site resolved uniquely",
            path.display(),
            image.timestamp()
        );
        return;
    }
    for (index, (&found, &recorded)) in sites.iter().zip(&RECORDED).enumerate() {
        assert_eq!(
            found, recorded,
            "site {index} at {found:#x}, recorded {recorded:#x}"
        );
    }
}

/// The Display Tweaks sites resolve uniquely in the installed SSEDisplayTweaks.dll, and to the
/// recorded 0.5.25 RVAs when it is that build. Skipped when it is not installed;
/// `RSF_SKYRIM_DISPLAY_TWEAKS` names another copy.
#[test]
fn display_tweaks_sites_resolve_in_the_installed_plugin() {
    let path = std::env::var_os("RSF_SKYRIM_DISPLAY_TWEAKS")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            signature::steam_file(
                "Skyrim Special Edition\\Data\\SKSE\\Plugins\\SSEDisplayTweaks.dll",
            )
        });
    let Some(path) = path else {
        eprintln!("skipped: SSEDisplayTweaks.dll is not installed");
        return;
    };
    let bytes = std::fs::read(&path).expect("read the installed plugin");
    let image = Pe32Plus::parse(&bytes).expect("PE32+ module");
    let code = image
        .sections(false)
        .into_iter()
        .filter(|section| section.2 & EXECUTE != 0)
        .map(|(_, rva, _, bytes)| Section { rva, bytes })
        .collect::<Vec<_>>();
    let read = |rva: usize, out: &mut [u8]| {
        image
            .span(rva, out.len())
            .map(|(bytes, _)| out.copy_from_slice(bytes))
            .is_some()
    };
    let sites = resolve_display_tweaks(&code, read)
        .unwrap_or_else(|refusal| panic!("{} in {}", refusal, path.display()));
    // The PE timestamp of the 0.5.25 build whose RVAs are recorded.
    if image.timestamp() != 0x6a93_3ece {
        eprintln!(
            "{} is not 0.5.25; its sites resolved uniquely",
            path.display()
        );
        return;
    }
    assert_eq!(
        sites,
        DisplayTweaks {
            wrapper: 0x2_7250,
            call: 0x2_72e9,
            call_return: 0x2_7334,
            retained: 0x5_e168,
        }
    );
}
