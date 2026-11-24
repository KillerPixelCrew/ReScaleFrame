// SPDX-License-Identifier: MIT
//! Signature discovery of every ProjectWingman-Win64-Shipping.exe site the temporal observer
//! hooks or reads, and of the view fields it reads.
//!
//! Prepare copies the loaded executable's code once and resolves each site from a pattern that is
//! unique in its executable sections (rel32 branches and RIP displacements are wildcards). The TAA
//! compute helper is a template instance with identical siblings, so it is found through its call
//! in `AddTemporalAAPass`, and the two vtables it installs through their `lea` inside it. The pass
//! execute entry (an identical twin follows it) and the tracked-allocation destructor are the
//! vtables' slots. The view fields are the displacements of instructions that read them. A missing
//! or ambiguous site refuses by name, and every hooked entry is matched again in live memory
//! before its hook is created. Recorded RVAs are those of Steam build 16671873;
//! docs/research/project-wingman-renderer.md records how the patterns were derived and proven.

use rsf_game_sys::signature::{self, EXECUTE, Find, Miss, Pattern, Refusal, Section, Signature};
use std::ffi::{CStr, CString};
use std::sync::OnceLock;

/// Every discovered site. The pattern sites come first, in [`SIGNATURES`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Site {
    AddPasses,
    FreeChunks,
    AddTaa,
    AddCompute,
    SetupPass,
    // The displacement of an instruction reading a view field.
    CameraCut,
    Jitter,
    ViewState,
    ViewRect,
    // Derived from the pattern sites and the image.
    /// Installed on the graph's tracked allocation by the compute helper.
    TrackedVtable,
    /// Installed on the compute pass embedded in that allocation.
    ComputeVtable,
    /// The tracked vtable's first slot.
    Destructor,
    /// The compute vtable's second slot.
    Execute,
}

const PATTERN_SITES: usize = Site::TrackedVtable as usize;
const SITES: usize = Site::Execute as usize + 1;
pub(crate) const HOOKS: [Site; 5] = [
    Site::FreeChunks,
    Site::AddTaa,
    Site::AddCompute,
    Site::SetupPass,
    Site::Execute,
];

const fn at(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(0),
        check: "",
    }
}

/// A view field whose 32-bit displacement is at `disp` in the pattern.
const fn field(name: &'static str, disp: usize, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(disp),
        check: "",
    }
}

/// A global addressed by the `disp32` at `disp` of the instruction ending at `next`.
const fn global(name: &'static str, disp: usize, next: usize, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::Rip { disp, next },
        check: "",
    }
}

/// Indexed by [`Site`]. Derived from build 16671873 by tools/derive-site-patterns.py.
const SIGNATURES: [Signature; PATTERN_SITES] = [
    at(
        "default temporal upscaler AddPasses",
        "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 40 48 8B 05 ?? ?? ?? ?? 49 8B F9",
    ),
    at(
        "FMemStackBase::FreeChunks",
        "48 8B C4 48 89 50 10 53 41 57 48 83 EC 48 48 8B 59 10",
    ),
    at(
        "AddTemporalAAPass",
        "48 8B C4 48 89 58 08 55 56 57 41 54 41 55 41 56 41 57 48 8D A8 78 FE FF FF",
    ),
    // Its call in AddTemporalAAPass; the entry is shared by the helper's sibling instances.
    Signature {
        name: "TAA compute pass helper",
        pattern: "E8 ?? ?? ?? ?? F6 83 AC 4F 00 00 01 0F 85 ?? ?? ?? ??",
        find: Find::Rip { disp: 1, next: 5 },
        check: "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54 41 56 41 57 48 83 EC 70 0F 29 74 24 60 49 8B D9",
    },
    at(
        "RDG pass setup",
        "48 89 54 24 10 55 53 56 57 41 54 41 55 41 56 41 57 48 8D 6C 24 E1 48 81 EC 98 00 00 00 0F B6 42 21",
    ),
    // `cmp byte [rbx+cut],0` beside the history-validity test in AddTemporalAAPass.
    field(
        "camera cut test",
        8,
        "48 83 3F 00 74 ?? 80 BB ?? ?? ?? ?? 00 C6 44 24 33 00",
    ),
    // `mov r8,[r8+jitter]`, the packed jitter passed to the sample-weight helper.
    field(
        "temporal jitter read",
        13,
        "4C 8B 45 F8 49 8B D5 49 8B CF 4D 8B 80 ?? ?? ?? ?? E8",
    ),
    // `mov rcx,[rsi+state]` before `add rcx,990h` (the output history) in the Gen4 helper.
    field(
        "view state read",
        8,
        "4C 8D 4C 24 50 48 8B 8E ?? ?? ?? ?? 4C 8B C6 48 8B 55 50 48 81 C1",
    ),
    // `movups xmm0,[rdx+rect]` at the Gen4 helper's entry.
    field(
        "view rectangle read",
        10,
        "48 81 EC 08 01 00 00 0F 10 82 ?? ?? ?? ?? 44 8B B2 ?? ?? ?? ?? 4D 8B E8",
    ),
];

/// The two vtable `lea`s of the compute helper, each unique within its first [`HELPER_WINDOW`]
/// bytes (a sibling instance follows the 0x19e-byte helper). The first fixes the 0x120-byte
/// allocation from graph + 0x10; the second fixes the pass at owner + 8.
const HELPER: [Signature; 2] = [
    global(
        "tracked allocation vtable",
        33,
        37,
        "48 8D 56 10 B9 20 01 00 00 44 0F B6 E0 45 8D 41 01 E8 ?? ?? ?? ?? 48 8B F8 48 85 C0 74 ?? 48 8D 05 ?? ?? ?? ?? 4C 89 74 24 20 48 89 07",
    ),
    global(
        "compute pass vtable",
        17,
        21,
        "48 8D 4F 08 E8 ?? ?? ?? ?? 0F 10 44 24 30 48 8D 05 ?? ?? ?? ?? 0F 10 4C 24 40 48 89 47 08",
    ),
];
const HELPER_WINDOW: usize = 0x180;

/// The destructor of the 0x120-byte tracked allocation (vtable slot 0).
const DESTRUCTOR: &str = "40 53 48 83 EC 20 48 8B D9 F6 C2 01 74 0A BA 20 01 00 00 E8 ?? ?? ?? ?? 48 8B C3 48 83 C4 20 5B C3";
/// The pass execute entry (vtable slot 1); fixes the parameter block at pass + 0xf0.
const EXECUTE_ENTRY: &str = "48 83 EC 38 F2 0F 10 81 08 01 00 00 4C 8D 4C 24 20 8B 81 10 01 00 00 4C 8B D2 4C 8B 81 F0 00 00 00";

/// View field offsets decoded from the code that reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) camera_cut: usize,
    pub(crate) jitter: usize,
    pub(crate) state: usize,
    pub(crate) rect: usize,
}

#[derive(Debug)]
struct Discovered {
    sites: [usize; SITES],
    view: View,
}

fn refused(name: &'static str) -> impl Fn(Miss) -> String {
    move |miss| Refusal { name, miss }.to_string()
}

/// Resolves every site from the code sections, the image base the image's pointers are relocated
/// to, and an RVA reader.
fn resolve_image(
    code: &[Section],
    image_base: u64,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<Discovered, String> {
    let found = signature::resolve(code, &SIGNATURES).map_err(|refusal| refusal.to_string())?;
    let mut sites = [0; SITES];
    sites[..PATTERN_SITES].copy_from_slice(&found);
    // A field is a positive displacement inside the view.
    let mut field = |site: Site| {
        let mut bytes = [0; 4];
        read(sites[site as usize], &mut bytes)
            .then(|| u32::from_le_bytes(bytes) as usize)
            .filter(|offset| (1..0x1_0000).contains(offset))
            .ok_or_else(|| refused(SIGNATURES[site as usize].name)(Miss::Operand))
    };
    let view = View {
        camera_cut: field(Site::CameraCut)?,
        jitter: field(Site::Jitter)?,
        state: field(Site::ViewState)?,
        rect: field(Site::ViewRect)?,
    };

    let helper = sites[Site::AddCompute as usize];
    let mut window = vec![0; HELPER_WINDOW];
    if !read(helper, &mut window) {
        return Err(refused("TAA compute pass helper")(Miss::Operand));
    }
    let section = Section {
        rva: helper,
        bytes: &window,
    };
    let vtables = signature::resolve(&[section], &HELPER).map_err(|refusal| refusal.to_string())?;
    sites[Site::TrackedVtable as usize] = vtables[0];
    sites[Site::ComputeVtable as usize] = vtables[1];
    for (site, vtable, slot, entry, name) in [
        (
            Site::Destructor,
            vtables[0],
            0,
            DESTRUCTOR,
            "tracked allocation destructor",
        ),
        (
            Site::Execute,
            vtables[1],
            1,
            EXECUTE_ENTRY,
            "compute pass execute",
        ),
    ] {
        let mut pointer = [0; 8];
        let rva = read(vtable + slot * 8, &mut pointer)
            .then(|| u64::from_le_bytes(pointer).checked_sub(image_base))
            .flatten()
            .and_then(|rva| usize::try_from(rva).ok())
            .ok_or_else(|| refused(name)(Miss::Operand))?;
        if !matches_with(&mut read, rva, entry) {
            return Err(refused(name)(Miss::Missing));
        }
        sites[site as usize] = rva;
    }
    Ok(Discovered { sites, view })
}

fn matches_with(read: &mut impl FnMut(usize, &mut [u8]) -> bool, at: usize, text: &str) -> bool {
    let Some(pattern) = Pattern::parse(text) else {
        return false;
    };
    let mut bytes = vec![0; pattern.len()];
    read(at, &mut bytes) && pattern.matches(&bytes)
}

struct Discovery {
    found: Result<Discovered, CString>,
    /// Evidence for the log: the build is named by its hash, and never admitted by it.
    evidence: String,
}

static DISCOVERY: OnceLock<Discovery> = OnceLock::new();

fn discover() -> Discovery {
    let base = crate::windows::image_base();
    let read = rsf_game_sys::win32::read_process;
    let found = signature::mapped_sections(base, read, |_, flags| flags & EXECUTE != 0)
        .ok_or_else(|| "executable sections unreadable".to_owned())
        .and_then(|code| {
            resolve_image(&signature::views(&code), base as u64, |rva, out| {
                rsf_game_sys::win32::read_bytes(base.wrapping_add(rva), out)
            })
        });
    let found = found.map_err(|reason| {
        CString::new(format!("Native temporal observer refused: {reason}.")).unwrap_or_default()
    });
    Discovery {
        found,
        evidence: crate::windows::executable_evidence(),
    }
}

/// Discovers every site once per process; the error names the refused site.
pub(crate) fn admit() -> Result<(), &'static CStr> {
    match &DISCOVERY.get_or_init(discover).found {
        Ok(_) => Ok(()),
        Err(reason) => Err(reason.as_c_str()),
    }
}

/// The executable's hash, timestamp and size for the log; empty before discovery.
pub(crate) fn evidence() -> &'static str {
    DISCOVERY.get().map_or("", |discovery| &discovery.evidence)
}

fn discovered() -> Option<&'static Discovered> {
    DISCOVERY.get()?.found.as_ref().ok()
}

/// The discovered RVA. Zero before [`admit`] resolved every site: nothing hooks or reads a site
/// before admission, and [`verify`] refuses an unresolved one.
pub(crate) fn rva(site: Site) -> usize {
    discovered().map_or(0, |found| found.sites[site as usize])
}

/// The decoded view fields; zero before admission, when no hook that reads them is installed.
pub(crate) fn view() -> View {
    discovered().map_or_else(View::default, |found| found.view)
}

/// Whether the live bytes at a resolved code site of the image at `image` still match its pattern,
/// so a site changed since discovery (another hook, or a wrong match) refuses before it is hooked.
pub(crate) fn verify(image: usize, site: Site) -> bool {
    let resolved = rva(site);
    let guard = match site {
        Site::Destructor => Some((DESTRUCTOR, resolved)),
        Site::Execute => Some((EXECUTE_ENTRY, resolved)),
        _ => SIGNATURES
            .get(site as usize)
            .and_then(|signature| signature.guard(resolved)),
    };
    guard
        .filter(|_| resolved != 0 && image != 0)
        .is_some_and(|(text, start)| {
            matches_with(
                &mut |rva, out: &mut [u8]| {
                    rsf_game_sys::win32::read_bytes(image.wrapping_add(rva), out)
                },
                start,
                text,
            )
        })
}

#[cfg(test)]
mod tests;
