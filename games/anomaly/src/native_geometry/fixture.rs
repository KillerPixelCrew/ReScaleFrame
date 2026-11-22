// SPDX-License-Identifier: MIT
//! Drives the native geometry scenarios and the real MinHook guard lifecycle.

use super::fixture_abi::{
    rsf_test_anomaly_geometry_contract as Contract, rsf_test_anomaly_geometry_hooks as Hooks,
    rsf_test_anomaly_geometry_scenario as Scenario,
};
use super::*;
use crate::engine_guards::decode_hex;
use std::ffi::CStr;

fn write_at(image: &mut [u8], rva: u32, bytes: &[u8]) -> Option<()> {
    let start = rva as usize;
    let end = start.checked_add(bytes.len())?;
    image.get_mut(start..end)?.copy_from_slice(bytes);
    Some(())
}

fn seed(image: &mut [u8], profile: &GuardProfile) -> Option<()> {
    for site in profile.sites().iter().chain(profile.layout_sites()) {
        write_at(image, site.rva, &site.expected)?;
    }
    for guard in profile.unique() {
        // Each bank lends immutable, NUL-terminated guard strings.
        let hex = unsafe { CStr::from_ptr(guard.expected_hex) }.to_bytes();
        write_at(image, guard.rva, &decode_hex(hex)?)?;
    }
    Some(())
}

fn run(scenarios: &[&Scenario]) {
    for scenario in scenarios {
        // The fixture retains its stack closures through the synchronous driver.
        unsafe { scenario.run.expect("native scenario")(scenario.context) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_anomaly_geometry_run_hooks(hooks: &Hooks) {
    run(&[
        &hooks.lifecycle,
        &hooks.flush_capture,
        &hooks.phase,
        &hooks.identities,
        &hooks.foliage,
        &hooks.vertex_constants,
        &hooks.contexts,
        &hooks.coverage,
        &hooks.retirement,
        &hooks.release,
        &hooks.guards,
    ]);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_anomaly_geometry_run_contract(contract: &Contract) {
    run(&[
        &contract.capture,
        &contract.history,
        &contract.coverage,
        &contract.inheritance,
        &contract.sink,
    ]);
}

fn guard_lifecycle(
    image: *mut u8,
    platform: &Platform,
    engine: Engine,
    other: Engine,
) -> Option<()> {
    let profile = unsafe { (platform.guard_profile)(engine) };
    let other_profile = unsafe { (platform.guard_profile)(other) };
    let size = profile.image_size;
    // The native fixture lends its executable allocation for the duration of the call.
    seed(
        unsafe { std::slice::from_raw_parts_mut(image, size as usize) },
        &profile,
    )?;

    let install = || rsf_anomaly_geometry_install(platform, engine, size);
    let disable = || rsf_anomaly_geometry_disable(platform);
    let checked_image = Image::new(image, size, profile.read);
    let flip = |rva: u32, mask: u8| {
        checked_image.address(rva, 1)?;
        // No slice or Rust reference to the allocation remains live during native calls.
        let byte = image.wrapping_add(rva as usize);
        unsafe { byte.write(byte.read() ^ mask) };
        Some(())
    };
    let refuse_mutation = |rva, mask| {
        flip(rva, mask)?;
        let refused = !install();
        flip(rva, mask)?;
        refused.then_some(())
    };

    for guard in profile.unique() {
        let hex = unsafe { CStr::from_ptr(guard.expected_hex) }.to_bytes();
        let last = u32::try_from((hex.len() / 2).checked_sub(1)?).ok()?;
        refuse_mutation(guard.rva.checked_add(last)?, 1)?;
    }
    for site in profile.layout_sites() {
        refuse_mutation(site.rva, 1)?;
    }
    for site in profile.sites() {
        refuse_mutation(site.rva.checked_add(5)?, 0x80)?;
    }

    // Corrupt the final detour entry and verify every preceding entry was untouched.
    let (last, preceding) = profile.sites().split_last()?;
    flip(last.rva, 1)?;
    let refused = !install() && preceding.iter().all(|site| checked_image.matches(site));
    flip(last.rva, 1)?;
    (refused
        && install()
        && rsf_anomaly_geometry_active()
        && disable()
        && !rsf_anomaly_geometry_active()
        && profile
            .sites()
            .iter()
            .all(|site| checked_image.matches(site))
        && !rsf_anomaly_geometry_install(platform, other, size)
        && !rsf_anomaly_geometry_install(platform, other, other_profile.image_size)
        && install()
        && disable())
    .then_some(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_anomaly_geometry_guard_lifecycle(
    image: *mut u8,
    platform: &Platform,
    engine: Engine,
    other: Engine,
) -> bool {
    !image.is_null() && guard_lifecycle(image, platform, engine, other).is_some()
}
