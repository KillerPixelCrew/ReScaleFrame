// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::pe32_plus::Pe32Plus;
use std::path::PathBuf;

/// PE timestamp of the 1.0.1491.50 build whose RVAs are recorded.
const RECORDED_TIMESTAMP: u32 = 0x65ee_c92e;

fn code<'a>(image: &[([u8; 8], usize, u32, &'a [u8])]) -> Vec<Section<'a>> {
    image
        .iter()
        .filter(|(_, _, flags, _)| flags & EXECUTE != 0)
        .map(|&(_, rva, _, bytes)| Section { rva, bytes })
        .collect()
}

#[test]
fn every_pattern_parses_and_every_site_is_recorded() {
    for signature in SIGNATURES.iter().chain(&SOCIAL_CLUB) {
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
    assert!(Pattern::parse(NATIVE_TEXTURE).is_some());
    assert_eq!(RECORDED.len(), SITES);
}

#[test]
fn unresolved_sites_are_zero_and_never_verify() {
    assert_eq!(rva(Site::Evaluate), 0);
    assert!(!verify(0x1_4000_0000, Site::Evaluate));
}

/// RDR2.exe's code is encrypted on disk, so the patterns are checked against a decrypted runtime
/// dump (RVA equal to file offset): `RSF_RDR2_DUMP`, or the repository's
/// `dumps/rdr2-20261025/RDR2.exe.dump`. Every pattern must resolve uniquely, and to the recorded
/// RVA when the dump is of the recorded build. Skipped without a dump.
#[test]
fn every_site_resolves_in_the_decrypted_executable() {
    let path = std::env::var_os("RSF_RDR2_DUMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../dumps/rdr2-20261025/RDR2.exe.dump")
        });
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipped: no decrypted RDR2.exe dump at {}", path.display());
        return;
    };
    let image = Pe32Plus::parse(&bytes).expect("PE32+ image");
    let sections = image.sections(true);
    let read = |rva: usize, out: &mut [u8]| {
        bytes
            .get(rva..rva + out.len())
            .map(|bytes| out.copy_from_slice(bytes))
            .is_some()
    };
    // The loader records the actual base in the mapped header, so table pointers resolve.
    let sites = resolve_image(&code(&sections), image.preferred_base(), read)
        .unwrap_or_else(|refusal| panic!("{} in {}", refusal, path.display()));
    for (index, signature) in SIGNATURES.iter().enumerate() {
        if let Some((text, start)) = signature.guard(sites[index]) {
            let pattern = Pattern::parse(text).unwrap();
            assert!(pattern.matches(&bytes[start..]), "{} guard", signature.name);
        }
    }
    let accessor = sites[Site::NativeTexture as usize];
    assert!(
        Pattern::parse(NATIVE_TEXTURE)
            .unwrap()
            .matches(&bytes[accessor..])
    );
    // Each postfx output call reaches the discovered output.
    for site in [
        Site::OutputReturn1,
        Site::OutputReturn2,
        Site::OutputReturn3,
    ] {
        let call = sites[site as usize] - 5;
        assert_eq!(
            crate::layout::call_target(call, bytes[call..call + 5].try_into().unwrap()),
            Some(sites[Site::Output as usize])
        );
    }
    if let Some(installed) = signature::steam_file("Red Dead Redemption 2\\RDR2.exe")
        && let Ok(file) = std::fs::read(&installed)
        && Pe32Plus::parse(&file).map(|exe| exe.timestamp()) != Some(image.timestamp())
    {
        eprintln!(
            "{} is not the build of the dump; refresh the dump to test the installed build",
            installed.display()
        );
    }
    if image.timestamp() != RECORDED_TIMESTAMP {
        eprintln!(
            "{} is not the recorded build (timestamp {:#x}); every site resolved uniquely",
            path.display(),
            image.timestamp()
        );
        return;
    }
    for (index, (&found, &recorded)) in sites.iter().zip(RECORDED).enumerate() {
        assert_eq!(
            found, recorded,
            "site {index} at {found:#x}, recorded {recorded:#x}"
        );
    }
}

/// The installed Social Club renderer resolves both of its sites (2.4.0.240 at +4b70 and +4b89).
#[test]
fn social_club_sites_resolve_in_the_installed_renderer() {
    let path = std::env::var_os("ProgramFiles").map(|root| {
        PathBuf::from(root).join("Rockstar Games\\Social Club\\SocialClubD3D12Renderer.dll")
    });
    let Some(bytes) = path.as_ref().and_then(|path| std::fs::read(path).ok()) else {
        eprintln!("skipped: SocialClubD3D12Renderer.dll is not installed");
        return;
    };
    let image = Pe32Plus::parse(&bytes).expect("PE32+ module");
    let sites = resolve_social_club(&code(&image.sections(false))).unwrap();
    if image.timestamp() == 0x6a86_d560 {
        assert_eq!(sites, [0x4b70, 0x4b89]);
    }
}
