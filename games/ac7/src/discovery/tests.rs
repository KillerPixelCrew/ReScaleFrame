// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::pe32_plus::Pe32Plus;
use std::path::PathBuf;

/// PE timestamp of Steam build 9855922, whose RVAs are recorded.
const RECORDED_TIMESTAMP: u32 = 0x6362_748e;

fn code<'a>(image: &[([u8; 8], usize, u32, &'a [u8])]) -> Vec<Section<'a>> {
    image
        .iter()
        .filter(|(_, _, flags, _)| flags & EXECUTE != 0)
        .map(|&(_, rva, _, bytes)| Section { rva, bytes })
        .collect()
}

#[test]
fn every_pattern_parses_and_derived_sites_have_none() {
    let derived: Vec<usize> = DERIVED.iter().map(|&(site, _, _)| site).collect();
    for (index, signature) in SIGNATURES.iter().chain(&TRUESKY).enumerate() {
        let is_derived = index < SITES && derived.contains(&index);
        assert_eq!(
            signature.pattern.is_empty(),
            is_derived,
            "{}",
            signature.name
        );
        assert!(
            is_derived || Pattern::parse(signature.pattern).is_some(),
            "{}",
            signature.name
        );
        assert!(
            signature.check.is_empty() || Pattern::parse(signature.check).is_some(),
            "{}",
            signature.name
        );
    }
    for (site, from, step) in DERIVED {
        // Each anchor resolves before what is found through it.
        assert!(
            derived.iter().position(|&known| known == from)
                < derived.iter().position(|&known| known == site),
            "{}",
            SIGNATURES[site].name
        );
        if let Step::Operand { instruction, .. } = step {
            assert!(Pattern::parse(instruction).is_some());
        }
    }
    assert!(Pattern::parse(SHADER_DEPTH).is_some());
    assert_eq!(Site::DownsampleDescriptor as usize, HOOKS);
}

#[test]
fn unresolved_sites_are_zero_and_never_verify() {
    assert_eq!(rva(0), 0);
    assert!(shader_depth_gates().is_empty());
    #[cfg(windows)]
    assert!(!verify(0x1_4000_0000, 0));
}

/// The carrier's writes, which the adapter's discovery runs after: the jitter and translucent
/// velocity gates become six-byte NOPs, the separate translucency scale a 15-byte constant load,
/// and every depth gate's jump the patched one.
fn carrier_patches(image: &mut [u8], sites: &[usize], shader_depth: &[usize]) {
    let nop6 = [0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00];
    let mut put = |rva: usize, bytes: &[u8]| image[rva..rva + bytes.len()].copy_from_slice(bytes);
    put(sites[Site::JitterGate as usize], &nop6);
    put(sites[Site::TranslucentVelocityGate as usize], &nop6);
    put(
        sites[Site::SeparateTranslucency as usize],
        b"\x66\x0f\x1f\x44\x00\x00\xb8\x00\x00\x80\x3f\x66\x0f\x6e\xc8",
    );
    for (site, offset, patched) in [
        (Site::GateDepthFill, 0, 0x74),
        (Site::GateDepthBind, 0, 0x74),
        (Site::GateSnapshotResolve, 2, 0x45),
        (Site::GateResolve, 0, 0x74),
        (Site::GateLayerView, 7, 0x74),
    ] {
        put(sites[site as usize] + offset, &[patched]);
    }
    for &gate in shader_depth {
        put(gate + 7, &[0x74]);
    }
}

/// Ace7Game.exe's code is encrypted on disk, so the patterns are checked against a decrypted image
/// (RVA equal to file offset, pointers at the preferred base): `RSF_AC7_DUMP`, or the repository's
/// `dumps/ac7/Ace7Game.exe.dump` (the loader writes one with RSF_DUMP_MODULE=1). Every pattern
/// must resolve uniquely, again after the carrier's patches, and to the recorded RVA when the
/// image is of the recorded build. Skipped without an image.
#[test]
fn every_site_resolves_in_the_decrypted_executable() {
    let path = std::env::var_os("RSF_AC7_DUMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dumps/ac7/Ace7Game.exe.dump")
        });
    let Ok(mut bytes) = std::fs::read(&path) else {
        eprintln!(
            "skipped: no decrypted Ace7Game.exe image at {}",
            path.display()
        );
        return;
    };
    let found = {
        let image = Pe32Plus::parse(&bytes).expect("PE32+ image");
        let read = |rva: usize, out: &mut [u8]| {
            bytes
                .get(rva..rva + out.len())
                .map(|bytes| out.copy_from_slice(bytes))
                .is_some()
        };
        resolve_image(&code(&image.sections(true)), image.preferred_base(), read)
    };
    let sites: Vec<usize> = found
        .sites
        .iter()
        .map(|site| site.unwrap_or_else(|refusal| panic!("{refusal} in {}", path.display())))
        .collect();
    let shader_depth = found.shader_depth.clone().expect("shader depth gates");
    for (index, signature) in SIGNATURES.iter().enumerate() {
        // Derived globals (the vtables found through an operand) have no code to check.
        if let Some((text, start)) = signature.guard(sites[index])
            && !text.is_empty()
        {
            let pattern = Pattern::parse(text).unwrap();
            assert!(pattern.matches(&bytes[start..]), "{} guard", signature.name);
        }
    }
    let timestamp = Pe32Plus::parse(&bytes).unwrap().timestamp();
    if let Some(installed) = signature::steam_file("ACE COMBAT 7\\Ace7Game.exe")
        && let Ok(file) = std::fs::read(&installed)
        && Pe32Plus::parse(&file).map(|exe| exe.timestamp()) != Some(timestamp)
    {
        eprintln!(
            "{} is not the build of the image; refresh the image to test the installed build",
            installed.display()
        );
    }
    // The adapter discovers after the carrier patched: its sites resolve the same.
    carrier_patches(&mut bytes, &sites, &shader_depth);
    let patched = {
        let image = Pe32Plus::parse(&bytes).unwrap();
        let read = |rva: usize, out: &mut [u8]| {
            bytes
                .get(rva..rva + out.len())
                .map(|bytes| out.copy_from_slice(bytes))
                .is_some()
        };
        resolve_image(&code(&image.sections(true)), image.preferred_base(), read)
    };
    let carrier = [
        Site::JitterGate,
        Site::TranslucentVelocityGate,
        Site::SeparateTranslucency,
    ]
    .map(|site| site as usize);
    for (index, (&before, after)) in sites.iter().zip(&patched.sites).enumerate() {
        if !carrier.contains(&index) {
            assert_eq!(
                Ok(before),
                *after,
                "{} after the carrier's patches",
                SIGNATURES[index].name
            );
        }
    }
    assert_eq!(patched.shader_depth, Ok(shader_depth.clone()));
    if timestamp != RECORDED_TIMESTAMP {
        eprintln!(
            "{} is not the recorded build (timestamp {timestamp:#x}); every site resolved uniquely",
            path.display()
        );
        return;
    }
    for (index, (&found, &recorded)) in sites.iter().zip(&RECORDED).enumerate() {
        assert_eq!(
            found, recorded,
            "{} at {found:#x}, recorded {recorded:#x}",
            SIGNATURES[index].name
        );
    }
    assert_eq!(shader_depth, SHADER_DEPTH_RECORDED);
}

/// The installed TrueSky plugin (not encrypted) resolves every site, to the recorded RVAs in the
/// shipped 1,711,616-byte build. Skipped when the game is not installed.
#[test]
fn truesky_sites_resolve_in_the_installed_plugin() {
    let Some(path) = signature::steam_file(
        "ACE COMBAT 7\\Engine\\Binaries\\ThirdParty\\Simul\\Win64\\TrueSkyPluginRender_MT.dll",
    ) else {
        eprintln!("skipped: TrueSkyPluginRender_MT.dll is not installed");
        return;
    };
    let bytes = std::fs::read(&path).expect("TrueSky plugin");
    let image = Pe32Plus::parse(&bytes).expect("PE32+ module");
    let sites = signature::resolve(&code(&image.sections(false)), &TRUESKY)
        .unwrap_or_else(|refusal| panic!("{refusal} in {}", path.display()));
    if bytes.len() == 1_711_616 {
        assert_eq!(sites, TRUESKY_RECORDED);
    }
}
