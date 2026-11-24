// SPDX-License-Identifier: MIT
//! Signature discovery of the gameoverlayrenderer64.dll code the swapchain forwarding patches.
//!
//! The overlay is found by name and its build by its code, not by its file hash: each of the ten
//! loads of Steam's saved original swapchain methods (a `mov rax/r10,[rip+slot]` with the call or
//! tail jump through it) and the Present and Present1 handler entries are matched by a pattern
//! unique in the overlay's executable sections, with rel32 branches and RIP displacements as
//! wildcards. Each method's saved slot is the RIP operand of its loads, and the two loads of one
//! method must name the same slot. A missing, ambiguous or disagreeing site refuses and names
//! itself; the patterns are matched again in live memory just before the hooks are written.
//! Recorded RVAs are those of Steam overlay 11.23.65.88 (`21c0c1dd...d5a6e52f`); the patterns
//! were derived with tools/derive-site-patterns.py from
//! docs/research/evidence/steam-overlay-sites-11.23.65.88.spec.

use rsf_game_sys::signature::{self, EXECUTE, Find, Pattern, Section, Signature};
use rsf_game_sys::{
    RSF_D3D11_DISPATCH_PRESENT, RSF_D3D11_DISPATCH_PRESENT1, RSF_D3D11_DISPATCH_RELEASE,
    RSF_D3D11_DISPATCH_RESIZE_BUFFERS, RSF_D3D11_DISPATCH_RESIZE_BUFFERS1,
    RSF_D3D11_DISPATCH_SET_COLOR_SPACE1, RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE,
};

/// The complete `mov rax/r10,[rip+disp32]` each relay replaces.
pub(crate) const LOAD_BYTES: usize = 7;
pub(crate) const LOADS: usize = 10;
pub(crate) const METHODS: usize = 7;

const fn at(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(0),
        check: "",
    }
}

/// The dispatch method each load saves, in [`SIGNATURES`] order.
pub(crate) const LOAD_METHODS: [u32; LOADS] = [
    RSF_D3D11_DISPATCH_RELEASE,
    RSF_D3D11_DISPATCH_RELEASE,
    RSF_D3D11_DISPATCH_PRESENT,
    RSF_D3D11_DISPATCH_PRESENT,
    RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE,
    RSF_D3D11_DISPATCH_RESIZE_BUFFERS,
    RSF_D3D11_DISPATCH_PRESENT1,
    RSF_D3D11_DISPATCH_PRESENT1,
    RSF_D3D11_DISPATCH_SET_COLOR_SPACE1,
    RSF_D3D11_DISPATCH_RESIZE_BUFFERS1,
];

/// The ten loads, then the Present and Present1 handler entries (entered before they take
/// Steam's global Present lock).
const SIGNATURES: [Signature; LOADS + 2] = [
    at(
        "Release load 1",
        "48 8B 05 ?? ?? ?? ?? 48 8B F9 FF D0 8B D8 85 C0 74 42 48 8B D7 48 8D 0D ?? ?? ?? ??",
    ),
    at(
        "Release load 2",
        "48 8B 05 ?? ?? ?? ?? 48 8B CF FF D0 8B D8 85 DB 75 2D 48 8B D7 48 8D 0D ?? ?? ?? ??",
    ),
    at(
        "Present load 1",
        "48 8B 05 ?? ?? ?? ?? FF D0 E9 ?? ?? ?? ?? 4C 8D 25 ?? ?? ?? ?? 4C 89 64 24 68",
    ),
    at(
        "Present load 2",
        "48 8B 05 ?? ?? ?? ?? 44 8B C6 8B D5 49 8B CF FF D0 8B F0 48 85 FF 74 2A",
    ),
    at(
        "SetFullscreenState load",
        "48 8B 05 ?? ?? ?? ?? 4D 8B C6 41 8B D7 48 8B CE 48 83 C4 28 41 5F 41 5E",
    ),
    at(
        "ResizeBuffers load",
        "4C 8B 15 ?? ?? ?? ?? 45 8B C7 41 8B D4 48 8B CD 48 83 C4 30 41 5F 41 5E",
    ),
    at(
        "Present1 load 1",
        "48 8B 05 ?? ?? ?? ?? FF D0 E9 ?? ?? ?? ?? 4C 8D 2D ?? ?? ?? ?? 4C 89 6C 24 20",
    ),
    at(
        "Present1 load 2",
        "48 8B 05 ?? ?? ?? ?? 4D 8B CC 45 8B C6 8B D5 49 8B CF FF D0 8B E8 48 85 FF",
    ),
    at(
        "SetColorSpace1 load",
        "48 8B 05 ?? ?? ?? ?? 8B D3 48 8B CE 48 83 C4 28 5F 5E 5D 5B 48 FF E0",
    ),
    at(
        "ResizeBuffers1 load",
        "4C 8B 15 ?? ?? ?? ?? 45 8B C4 48 89 9C 24 F8 00 00 00 8B D5 48 89 BC 24 F0 00 00 00",
    ),
    at(
        "Present entry",
        "48 89 5C 24 08 48 89 6C 24 10 56 57 41 54 41 56 41 57 48 83 EC 20",
    ),
    at(
        "Present1 entry",
        "40 53 55 56 57 41 54 41 55 41 56 41 57 48 83 EC 38 4D 8B E1",
    ),
];

/// The overlay sites as RVAs: the loads, the two handler entries and each method's saved slot,
/// indexed by dispatch method - 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sites {
    pub(crate) loads: [usize; LOADS],
    pub(crate) entries: [usize; 2],
    pub(crate) slots: [usize; METHODS],
}

/// Resolves every site from the overlay's code sections and an RVA reader.
fn resolve(
    code: &[Section],
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<Sites, String> {
    let found = signature::resolve(code, &SIGNATURES).map_err(|refusal| refusal.to_string())?;
    let mut sites = Sites {
        loads: found[..LOADS].try_into().expect("ten loads"),
        entries: found[LOADS..].try_into().expect("two entries"),
        slots: [0; METHODS],
    };
    for ((&load, &method), signature) in sites.loads.iter().zip(&LOAD_METHODS).zip(&SIGNATURES) {
        let mut operand = [0; 4];
        let slot = read(load + 3, &mut operand)
            .then(|| (load + LOAD_BYTES).checked_add_signed(i32::from_le_bytes(operand) as isize))
            .flatten()
            .ok_or_else(|| format!("site '{}' has an undecodable operand", signature.name))?;
        let saved = &mut sites.slots[method as usize - 1];
        if *saved != 0 && *saved != slot {
            return Err(format!(
                "site '{}' loads another method slot than its sibling",
                signature.name
            ));
        }
        *saved = slot;
    }
    Ok(sites)
}

/// Discovers the sites of the overlay module mapped at `base`.
pub(crate) fn discover(base: usize) -> Result<Sites, String> {
    let read = rsf_game_sys::win32::read_process;
    let code = signature::mapped_sections(base, read, |_, flags| flags & EXECUTE != 0)
        .ok_or("overlay sections unreadable")?;
    resolve(&signature::views(&code), |rva, out| unsafe {
        read(base.wrapping_add(rva), out.as_mut_ptr().cast(), out.len())
    })
}

/// The name of the first site whose live bytes no longer match its pattern, if any.
pub(crate) fn changed(base: usize, sites: &Sites) -> Option<&'static str> {
    let rvas = sites.loads.iter().chain(&sites.entries);
    SIGNATURES.iter().zip(rvas).find_map(|(signature, &rva)| {
        let Some(pattern) = Pattern::parse(signature.pattern) else {
            return Some(signature.name);
        };
        let mut bytes = vec![0; pattern.len()];
        let live = unsafe {
            rsf_game_sys::win32::read_process(
                base.wrapping_add(rva),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        (!live || !pattern.matches(&bytes)).then_some(signature.name)
    })
}

#[cfg(test)]
mod tests;
