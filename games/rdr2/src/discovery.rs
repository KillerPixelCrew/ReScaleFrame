// SPDX-License-Identifier: MIT
//! Signature discovery of every RDR2.exe site this adapter hooks, patches, calls or reads, and of
//! the Social Club renderer's backbuffer gate.
//!
//! The ASI resolves the sites at attach, before it hooks anything. It copies the decrypted
//! executable's code once and resolves each site from a pattern that is unique in its executable
//! sections (rel32 branches and RIP displacements are wildcards). The game plugin, a separate module
//! with its own table, takes the ASI's table through the ASI's `rsf_rdr2_asi_sites` export instead of
//! scanning: by prepare the ASI has replaced the first bytes of the entries it hooks, so a scan of
//! live code would no longer match them. Only without an ASI of the same site list (the research
//! attach DLL, the fixtures) does the plugin scan itself. Globals resolve
//! through the RIP-relative operand of an instruction that uses them; the API parameter must be the
//! target of all four of its discovered reads. The FSR2 import slot comes from the import directory and the
//! native-texture accessor from the renderer table. A missing or ambiguous site refuses by name,
//! and every site is matched again in live memory before it is hooked, patched or called.
//! Recorded RVAs are those of Steam 1.0.1491.50 (build 13773296). RDR2.exe's code is encrypted on
//! disk, so the patterns were derived from and are tested against a decrypted runtime dump;
//! docs/research/rdr2-renderer.md records how.

use rsf_game_sys::signature::{self, EXECUTE, Find, Miss, Pattern, Refusal, Section, Signature};
use std::sync::OnceLock;

/// Every discovered site. The pattern sites come first, in [`SIGNATURES`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Site {
    Evaluate,
    Pop,
    Blit,
    Marker,
    Output,
    Sleep,
    Mode,
    OutputReturn1,
    OutputReturn2,
    OutputReturn3,
    BlitReturn,
    SubmitReturn,
    FsrWrapperReturn,
    SocialClubDispatcher,
    Copy,
    Load,
    Apply,
    Optimal,
    FsrDimensions,
    RefreshDimensions,
    Reconfigure,
    Manager,
    Owner,
    Find,
    ApiRead1,
    ApiRead2,
    ApiRead3,
    ApiRead4,
    RendererTable,
    Swapchain,
    TlsIndex,
    FrameCounter,
    FrameCounterPointer,
    FrameTime,
    RenderInput,
    RenderOutput,
    #[cfg(feature = "research")]
    CameraFsr,
    #[cfg(feature = "research")]
    CameraDlss,
    #[cfg(feature = "research")]
    SettingsBind,
    #[cfg(feature = "research")]
    SettingsRead,
    #[cfg(feature = "research")]
    ResultCheck,
    // Derived from the pattern sites and the image.
    /// The engine's API string parameter, read by all four `ApiRead` sites.
    ApiOverride,
    /// The `ffx_fsr2_api_x64.dll!ffxFsr2ContextDispatch` import address table slot.
    FsrImport,
    /// The DX12 renderer's native-texture accessor, stored in its table.
    NativeTexture,
}

const PATTERN_SITES: usize = Site::ApiOverride as usize;
const SITES: usize = Site::NativeTexture as usize + 1;

const fn at(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(0),
        check: "",
    }
}

/// A five-byte call; the site is its return address.
const fn returns(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(5),
        check: "",
    }
}

/// A function found through its only distinctive call, with its own entry checked.
const fn called(name: &'static str, pattern: &'static str, check: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::Rip { disp: 1, next: 5 },
        check,
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

/// Indexed by [`Site`]. Derived from the 1.0.1491.50 dump by tools/derive-site-patterns.py.
const SIGNATURES: &[Signature] = &[
    at(
        "DLSS evaluate",
        "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 49 8B D8 48 8B F2 4D 8B 01",
    ),
    at(
        "command queue pop",
        "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B FA 48 8B D9 48 8B 89 38 10 00 00",
    ),
    at(
        "scene blit",
        "4C 8B DC 49 89 5B 08 49 89 6B 10 49 89 73 18 49 89 7B 20 41 56 48 83 EC 30 48 8B 44 24 68",
    ),
    at(
        "frame marker",
        "48 89 5C 24 08 48 89 54 24 10 55 48 8D 6C 24 A9",
    ),
    at(
        "postfx output",
        "48 89 5C 24 08 48 89 74 24 18 57 48 83 EC 50 41 8B F8",
    ),
    at(
        "latency sleep",
        "40 53 48 83 EC 50 80 3D ?? ?? ?? ?? 00 0F 84 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00",
    ),
    called(
        "latency mode",
        "E8 ?? ?? ?? ?? 85 C0 75 1F E8 ?? ?? ?? ?? 80 78 09 03",
        "48 89 74 24 10 57 48 83 EC 20 48 8B FA 48 8B F1",
    ),
    returns(
        "postfx output call 1",
        "E8 ?? ?? ?? ?? 48 8D 4D 90 E8 ?? ?? ?? ?? EB 05 BE F0 14 00 00",
    ),
    returns(
        "postfx output call 2",
        "E8 ?? ?? ?? ?? 48 8B 4D B0 48 8B 55 90 45 38 A7 18 B3 00 00",
    ),
    returns(
        "postfx output call 3",
        "E8 ?? ?? ?? ?? 65 48 8B 04 25 58 00 00 00 48 8B 15 ?? ?? ?? ?? 4A 8B 0C F0 48 8B 0C 31",
    ),
    returns(
        "scene blit call",
        "E8 ?? ?? ?? ?? 48 85 DB 74 44 65 48 8B 0C 25 58 00 00 00",
    ),
    returns(
        "queue submit call",
        "E8 ?? ?? ?? ?? 8B 44 24 48 4C 8B 44 24 40 4C 8B 74 24 30",
    ),
    returns(
        "FSR2 dispatch wrapper call",
        "E8 ?? ?? ?? ?? 44 8B F8 85 C0 75 70 4C 8B 43 18",
    ),
    at(
        "Social Club dispatcher",
        "40 53 48 83 EC 30 48 83 79 08 00 48 8B D9 75 29 BA 01 00 00 00 C7 44 24 20 D2 E2 31 89 41 B9 AE 00 00 00",
    ),
    at(
        "settings copy",
        "48 89 5C 24 08 57 48 83 EC 20 0F 10 02 48 8B D9 4C 8B C1",
    ),
    at(
        "settings load",
        "48 8B C4 48 89 58 08 48 89 70 10 48 89 78 18 4C 89 60 20 55 48 8D A8 08 FE FF FF",
    ),
    at(
        "settings apply",
        "40 53 48 83 EC 20 48 8B D9 E8 ?? ?? ?? ?? 48 8B CB 48 83 C4 20 5B E9 ?? ?? ?? ?? 90 48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 30",
    ),
    at(
        "DLSS optimal settings",
        "48 89 5C 24 08 55 56 57 41 54 41 55 41 56 41 57 48 8D 6C 24 D9 48 81 EC 00 01 00 00 80 3D ?? ?? ?? ?? 00",
    ),
    at(
        "FSR2 dimensions",
        "48 8B C4 48 89 58 08 48 89 70 10 48 89 78 18 55 41 56 41 57 48 8D A8 88 FC FF FF",
    ),
    at(
        "dimension refresh",
        "48 81 EC 38 01 00 00 48 8D 4C 24 20 E8 ?? ?? ?? ??",
    ),
    // The compare also fixes the manager stage at +2c8.
    at(
        "graphics reconfiguration",
        "40 53 48 83 EC 20 BA 09 00 00 00 48 8B D9 39 91 C8 02 00 00",
    ),
    called(
        "settings manager",
        "E8 ?? ?? ?? ?? 48 8B C8 41 B1 01 45 33 C0",
        "48 8B 05 ?? ?? ?? ?? C3",
    ),
    called(
        "settings owner",
        "E8 ?? ?? ?? ?? 48 8B C8 E8 ?? ?? ?? ?? BD 02 00 00 00",
        "4C 8B DC 53 48 81 EC B0 00 00 00 49 8D 43 08",
    ),
    at(
        "settings find",
        "40 53 48 83 EC 30 48 83 7A 08 00 49 8B C0 76 3A",
    ),
    // The four reads of the API parameter, each `mov r64, [rip+parameter]`.
    at(
        "API read in settings load",
        "48 8B 1D ?? ?? ?? ?? 41 BC 02 00 00 00 48 85 DB",
    ),
    at(
        "API read in adapter selection",
        "48 8B 05 ?? ?? ?? ?? 48 8B F9 33 ED 48 85 C0 48 0F 45 F0",
    ),
    at(
        "API read in adapter resolution",
        "48 8B 0D ?? ?? ?? ?? 48 85 C9 74 10 48 8D 15 ?? ?? ?? ??",
    ),
    at(
        "API read in API level",
        "48 8B 05 ?? ?? ?? ?? 33 FF 48 85 C0 75 28 8B 8B B0 00 00 00",
    ),
    global(
        "renderer table",
        3,
        7,
        "48 8D 05 ?? ?? ?? ?? 48 89 03 48 89 53 18",
    ),
    global(
        "swapchain",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? 4C 8B C3 40 0F B6 D7",
    ),
    global(
        "TLS index",
        2,
        6,
        "8B 05 ?? ?? ?? ?? BA C8 14 00 00 44 89 05 ?? ?? ?? ??",
    ),
    global(
        "frame counter",
        3,
        7,
        "3E 8B 05 ?? ?? ?? ?? 39 47 0C 74 33 48 39 1F",
    ),
    global(
        "frame counter pointer",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 8B 08 EB 06 8B 0D ?? ?? ?? ?? 8B D1",
    ),
    global(
        "frame time",
        4,
        8,
        "F3 0F 10 0D ?? ?? ?? ?? 0F 28 DE 41 B0 01",
    ),
    global(
        "render input size",
        4,
        8,
        "F3 0F 2A 0D ?? ?? ?? ?? 0F 57 C0 0F 28 DE",
    ),
    global(
        "render output size",
        2,
        6,
        "89 15 ?? ?? ?? ?? 8B 87 98 00 00 00 89 05 ?? ?? ?? ??",
    ),
    #[cfg(feature = "research")]
    at(
        "camera FSR upscale",
        "48 83 3A 00 4C 8B CA 74 27 48 83 7A 18 00 74 20",
    ),
    #[cfg(feature = "research")]
    at(
        "camera DLSS upscale",
        "8B 81 A8 00 00 00 4C 8D 0C 80 4A 8D 0C C9 48 83 C1 38",
    ),
    #[cfg(feature = "research")]
    at(
        "settings bind",
        "48 89 5C 24 10 48 89 7C 24 18 55 48 8D 6C 24 D0 48 81 EC 30 01 00 00 41 8B 40 08",
    ),
    #[cfg(feature = "research")]
    at(
        "settings read",
        "48 89 5C 24 08 57 48 83 EC 50 48 8B 02 4C 8B D2",
    ),
    #[cfg(feature = "research")]
    at(
        "D3D12 result check",
        "40 53 48 83 EC 20 8D 81 FB FF 85 77 8B D9 83 F8 02",
    ),
];

const _: () = assert!(SIGNATURES.len() == PATTERN_SITES);

/// The 1.0.1491.50 RVAs, indexed by [`Site`]: documentation and the dump test.
#[cfg(test)]
const RECORDED: &[usize] = &[
    0x26f_3a3c,
    0x26c_97d4,
    0x5c_1254,
    0x26d_4674,
    0x5f_1e08,
    0x268_7fb4,
    0x1_e800,
    0x5c_ab01,
    0x5c_ad9b,
    0x5c_adf1,
    0x5c_11b9,
    0x26d_f79b,
    0x26f_050a,
    0x27f_09e8,
    0x20_b7f8,
    0x2c7_c9a4,
    0x2c7_e6c0,
    0x26f_5270,
    0x26e_cc3c,
    0x58_a908,
    0x2c7_e8f4,
    0x2cf_31f8,
    0x2c7_6114,
    0x2c7_36f4,
    0x2c6_5575,
    0x26b_8ebb,
    0x22_a5f8,
    0x2c6_62f6,
    0x362_b1d0,
    0x571_c7e8,
    0x5b4_03e0,
    0x542_904c,
    0x542_9058,
    0x59b_393c,
    0x3f0_db38,
    0x3f0_db30,
    #[cfg(feature = "research")]
    0x7b_a10c,
    #[cfg(feature = "research")]
    0x7b_b60c,
    #[cfg(feature = "research")]
    0x2c6_26e0,
    #[cfg(feature = "research")]
    0x2c6_27b8,
    #[cfg(feature = "research")]
    0x268_b5c0,
    0x57a_bf10,
    0x603_2a98,
    0x26b_4250,
];

/// The accessor's slot in the renderer table (slot 99), a documented offset: no instruction
/// addresses the slot itself.
const NATIVE_TEXTURE_SLOT: usize = 0x318;
/// `mov rax,[rdx+48]; ret`: the accessor fixes the texture's native resource at +48.
const NATIVE_TEXTURE: &str = "48 8B 42 48 C3";

/// Resolves every site from the code sections, the image base the image's pointers are
/// relocated to, and an RVA reader.
fn resolve_image(
    code: &[Section],
    image_base: u64,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<[usize; SITES], String> {
    let found = signature::resolve(code, SIGNATURES).map_err(|refusal| refusal.to_string())?;
    let mut sites = [0; SITES];
    sites[..PATTERN_SITES].copy_from_slice(&found);
    // Every read of the API parameter must address the same global.
    let mut targets = sites[Site::ApiRead1 as usize..=Site::ApiRead4 as usize]
        .iter()
        .map(|&site| {
            let mut bytes = [0; 7];
            read(site, &mut bytes)
                .then(|| parameter_read(site, bytes))
                .flatten()
        });
    let api = targets.next().flatten();
    if api.is_none() || targets.any(|target| target != api) {
        return Err("site 'API parameter' differs between its reads".into());
    }
    sites[Site::ApiOverride as usize] = api.unwrap_or(0);
    sites[Site::FsrImport as usize] =
        signature::import_slot(&mut read, "ffx_fsr2_api_x64.dll", "ffxFsr2ContextDispatch")
            .ok_or_else(|| refused("FSR2 import")(Miss::Missing))?;
    let mut entry = [0; 8];
    let slot = sites[Site::RendererTable as usize] + NATIVE_TEXTURE_SLOT;
    sites[Site::NativeTexture as usize] = read(slot, &mut entry)
        .then(|| u64::from_le_bytes(entry).checked_sub(image_base))
        .flatten()
        .and_then(|rva| usize::try_from(rva).ok())
        .ok_or_else(|| refused("native texture")(Miss::Operand))?;
    Ok(sites)
}

/// The global a `mov r64, [rip+disp32]` at `site` reads.
pub(crate) fn parameter_read(site: usize, bytes: [u8; 7]) -> Option<usize> {
    let displacement = i32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]);
    (bytes[0] == 0x48 && bytes[1] == 0x8b && bytes[2] & 0xc7 == 0x05).then(|| {
        site.checked_add(7)?
            .checked_add_signed(displacement as isize)
    })?
}

fn refused(name: &'static str) -> impl Fn(Miss) -> String {
    move |miss| Refusal { name, miss }.to_string()
}

static DISCOVERY: OnceLock<Result<[usize; SITES], String>> = OnceLock::new();

/// The ASI's file name, fixed by its CMake output name.
#[cfg(windows)]
const ASI_MODULE: windows_sys::core::PCWSTR = windows_sys::core::w!("ReScaleFrame.RDR2.asi");

#[cfg(windows)]
type AsiSites = unsafe extern "C" fn(*mut usize, usize) -> usize;

/// Copies the table this module resolved into `sites` (`capacity` entries) and returns its length;
/// zero when nothing resolved yet or it does not fit. Every ASI hook is installed after
/// [`admit`] resolved, so a table here was resolved from unhooked code. The plugin calls it through
/// the export of the ASI's module definition (`asi.def`).
#[cfg(windows)]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_asi_sites(sites: *mut usize, capacity: usize) -> usize {
    match DISCOVERY.get() {
        Some(Ok(found)) if found.len() <= capacity => {
            unsafe { std::ptr::copy_nonoverlapping(found.as_ptr(), sites, found.len()) };
            found.len()
        }
        _ => 0,
    }
}

/// The ASI's table, when it has the same site list as this module (the `research` feature adds
/// sites).
#[cfg(windows)]
fn from_asi(export: AsiSites) -> Option<[usize; SITES]> {
    let mut sites = [0; SITES];
    (unsafe { export(sites.as_mut_ptr(), SITES) } == SITES).then_some(sites)
}

#[cfg(windows)]
fn discover() -> Result<[usize; SITES], String> {
    // The ASI asks its own export while it is still resolving, which answers nothing.
    let asi = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(ASI_MODULE) };
    if let Some(sites) =
        unsafe { rsf_game_sys::win32::typed_export(asi, c"rsf_rdr2_asi_sites") }.and_then(from_asi)
    {
        return Ok(sites);
    }
    let base = unsafe { crate::platform::main_module() };
    let code =
        signature::mapped_sections(base, crate::platform::read_process_memory, |_, flags| {
            flags & EXECUTE != 0
        })
        .ok_or("RDR2 executable sections unreadable")?;
    resolve_image(&signature::views(&code), base as u64, |rva, out| {
        crate::platform::read_bytes(base.wrapping_add(rva), out)
    })
    .map_err(|reason| format!("RDR2 native sites refused: {reason}."))
}

#[cfg(not(windows))]
fn discover() -> Result<[usize; SITES], String> {
    Err("RDR2 native sites refused: not a Windows process.".into())
}

/// Discovers every site once per process; the error names the refused site.
pub(crate) fn admit() -> Result<(), &'static str> {
    match DISCOVERY.get_or_init(discover) {
        Ok(_) => Ok(()),
        Err(reason) => Err(reason.as_str()),
    }
}

/// The discovered RVA. Zero before [`admit`] resolved every site: nothing hooks, patches or reads
/// a site before admission, and [`verify`] refuses an unresolved one.
pub(crate) fn rva(site: Site) -> usize {
    match DISCOVERY.get() {
        Some(Ok(sites)) => sites[site as usize],
        _ => 0,
    }
}

fn matches(address: usize, text: &str) -> bool {
    let Some(pattern) = Pattern::parse(text) else {
        return false;
    };
    let mut bytes = vec![0; pattern.len()];
    crate::platform::read_bytes(address, &mut bytes) && pattern.matches(&bytes)
}

/// Whether the live bytes at a resolved code site of the image at `image` still match its pattern,
/// so a site changed since discovery (another hook, or a wrong match) refuses before it is hooked,
/// patched or called.
pub(crate) fn verify(image: usize, site: Site) -> bool {
    let resolved = rva(site);
    let guard = match site {
        Site::NativeTexture => Some((NATIVE_TEXTURE, resolved)),
        _ => SIGNATURES
            .get(site as usize)
            .and_then(|signature| signature.guard(resolved)),
    };
    guard
        .filter(|_| resolved != 0 && image != 0)
        .is_some_and(|(text, start)| matches(image.wrapping_add(start), text))
}

/// Social Club renderer (`SocialClubD3D12Renderer.dll`) sites: the frame entry traced by the
/// research build and the external-command-list gate the backbuffer fix relays. The gate's
/// pattern fixes `cmp byte [rcx+40C],0`, which the relay repeats. Derived from 2.4.0.240.
const SOCIAL_CLUB: [Signature; 2] = [
    at(
        "Social Club frame entry",
        "48 89 5C 24 18 48 89 54 24 10 55 56 57 41 54 41 55",
    ),
    at(
        "Social Club gate",
        "80 B9 0C 04 00 00 00 4D 8B F1 48 8B F9 74 17 48 8B 49 20",
    ),
];

/// The Social Club renderer's (frame entry, gate) RVAs in its code sections.
fn resolve_social_club(code: &[Section]) -> Result<[usize; 2], String> {
    let found = signature::resolve(code, &SOCIAL_CLUB).map_err(|refusal| refusal.to_string())?;
    Ok([found[0], found[1]])
}

/// Discovers the Social Club renderer's (frame entry, gate) RVAs in the module loaded at `base`
/// and checks them in live memory.
#[cfg(windows)]
pub(crate) fn social_club(base: usize) -> Result<[usize; 2], String> {
    let code =
        signature::mapped_sections(base, crate::platform::read_process_memory, |_, flags| {
            flags & EXECUTE != 0
        })
        .ok_or("Social Club renderer sections unreadable")?;
    let sites = resolve_social_club(&signature::views(&code))?;
    if SOCIAL_CLUB
        .iter()
        .zip(sites)
        .all(|(signature, rva)| matches(base.wrapping_add(rva), signature.pattern))
    {
        Ok(sites)
    } else {
        Err("Social Club renderer bytes changed".into())
    }
}

#[cfg(test)]
mod tests;
