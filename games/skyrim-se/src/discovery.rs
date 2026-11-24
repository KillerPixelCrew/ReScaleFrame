// SPDX-License-Identifier: MIT
//! Signature discovery of every SkyrimSE.exe site this adapter hooks, patches, calls or reads.
//!
//! Admission scans the loaded executable's code once and resolves each site from a pattern that
//! is unique in the code sections (rel32 branches and RIP displacements are wildcards), so a
//! game update that moves code keeps working while its patterns stay unique. Globals resolve
//! through the RIP-relative operand of an instruction that uses them. The D3D11 creation thunk
//! follows from the import table and the named-poll callback from its "Poll controls" entry in
//! the engine's callback table. A site that is missing or ambiguous refuses admission and names
//! itself; before a site is hooked or patched its pattern is matched again at the site.
//! Recorded RVAs are those of Steam 1.7.104.0 (`846efccf...1402f`); the patterns and how each was
//! derived are in docs/research/skyrim-se-integration.md and tools/derive-site-patterns.py.

use crate::platform::Platform;
use rsf_game_sys::signature::{
    self, EXECUTE, Find, Miss, Pattern, Refusal, Section, Signature, find_rip_reference,
    find_unique, import_slot,
};
use std::ffi::{CStr, CString, c_char};
use std::sync::OnceLock;

/// Every discovered site. The pattern sites come first, in [`SIGNATURES`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Site {
    Jitter,
    Process,
    ProcessCall,
    Reset,
    Resolution,
    End,
    Viewport,
    Upload,
    CreateCall,
    DrawInterface,
    Flush,
    MenuOpen,
    FrozenBackground,
    Tick,
    Poll,
    Render,
    Dispatch,
    World,
    Controls,
    CameraUpdate,
    JobsSubmit,
    JobsJoin,
    Movement,
    PlayerUpdate,
    PlayerCameraUpdate,
    SubmitReturn,
    JoinReturn,
    WorldReturn,
    NamedPollReturn,
    Owner,
    NativeFrame,
    ActiveWindow,
    Renderer,
    ImmediateContext,
    MainDepth,
    ShadowView,
    RasterRatio,
    SurfaceWidth,
    RenderState,
    UploadDirty,
    TaaManager,
    ActiveCamera,
    DofSelector,
    ColourTarget,
    DepthTarget,
    AddressModes,
    SamplerTable,
    Ui,
    UiNames,
    CameraRoot,
    Main,
    Player,
    JobManager,
    InputManager,
    PlayerControls,
    PlayerCamera,
    // Derived from the image, not from a code pattern.
    CreateThunk,
    /// The D3D11CreateDeviceAndSwapChain import address table slot the thunk jumps through.
    CreateImport,
    NamedCallbackSlot,
    NamedPoll,
}

const PATTERN_SITES: usize = Site::CreateThunk as usize;
const SITES: usize = Site::NamedPoll as usize + 1;

const fn at(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(0),
        check: "",
    }
}

/// A call or return site: the pattern starts at the call, the site is its return address.
const fn returns(name: &'static str, length: usize, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(length),
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

/// Indexed by [`Site`]. Patterns were derived from 1.7.104.0 by tools/derive-site-patterns.py.
const SIGNATURES: [Signature; PATTERN_SITES] = [
    at(
        "jitter",
        "48 8B 05 ?? ?? ?? ?? 0F 57 C0 48 8B 90 F0 01 00 00",
    ),
    at(
        "process",
        "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 20 44 89 44 24 18",
    ),
    at(
        "process call",
        "E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 8B 88 28 01 00 00 0F 11 B1 74 01 00 00",
    ),
    at(
        "reset",
        "44 39 71 30 74 09 E8 ?? ?? ?? ?? 44 89 73 30 83 7B 2C 02",
    ),
    at(
        "resolution",
        "83 B9 28 01 00 00 00 48 8B D1 0F 85 ?? ?? ?? ??",
    ),
    at(
        "end",
        "48 83 EC 48 B9 01 00 00 00 E8 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00",
    ),
    at(
        "viewport",
        "48 83 EC 68 0F 29 74 24 50 F3 0F 10 35 ?? ?? ?? ?? 0F 29 7C 24 40 0F 28 FE 44 0F 29 44 24 30",
    ),
    at(
        "upload",
        "48 89 5C 24 08 55 48 8D AC 24 E0 FD FF FF 48 81 EC 20 03 00 00",
    ),
    at(
        "create call",
        "E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 40 38 79 20 74 16",
    ),
    at(
        "draw interface",
        "4C 8B DC 56 57 41 54 41 55 41 57 48 83 EC 50 49 C7 43 B8 FE FF FF FF",
    ),
    at(
        "flush",
        "4C 8B DC 41 56 48 81 EC A0 00 00 00 8B 15 ?? ?? ?? ??",
    ),
    at(
        "menu open",
        "40 53 48 83 EC 40 48 C7 44 24 20 FE FF FF FF 33 C0 48 89 44 24 28",
    ),
    at(
        "frozen background",
        "48 89 5C 24 10 48 89 6C 24 18 48 89 74 24 20 57 41 56 41 57 48 83 EC 20 45 33 FF 48 8B E9",
    ),
    at(
        "tick",
        "48 8B C4 55 41 54 41 55 41 56 41 57 48 81 EC 30 01 00 00",
    ),
    at(
        "poll",
        "48 89 5C 24 10 48 89 74 24 18 57 48 83 EC 30 0F 29 74 24 20 48 8D 59 60",
    ),
    at(
        "render",
        "40 53 55 56 57 41 54 41 56 41 57 48 81 EC 60 01 00 00",
    ),
    // Its prologue is not unique; the only caller's call is, and the prologue is checked.
    Signature {
        name: "dispatch",
        pattern: "E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 0F B6 86 F0 00 00 00",
        find: Find::Rip { disp: 1, next: 5 },
        check: "48 8B C4 48 89 50 10 55 56 57 41 54 41 55 41 56 41 57 48",
    },
    at(
        "world",
        "48 8B C4 89 50 10 55 56 57 41 54 41 55 41 56 41 57 48 8D 68 A1",
    ),
    at(
        "controls",
        "48 8B C4 56 57 41 54 41 56 41 57 48 81 EC 80 00 00 00 48 C7 40 88 FE FF FF FF 48 89 58 10 48 89 68 18 0F 29 70 C8 4C 8B F2",
    ),
    at(
        "camera update",
        "40 57 48 83 EC 70 48 C7 44 24 20 FE FF FF FF 48 89 9C 24 88 00 00 00",
    ),
    at(
        "jobs submit",
        "48 83 EC 28 48 83 3D ?? ?? ?? ?? 00 74 69 48 89 5C 24 30",
    ),
    at(
        "jobs join",
        "40 53 48 83 EC 20 48 8B 1D ?? ?? ?? ?? 48 85 DB 74 26",
    ),
    at(
        "movement",
        "48 8B C4 41 56 48 81 EC E0 00 00 00 48 C7 44 24 30 FE FF FF FF",
    ),
    at(
        "player update",
        "48 8B C4 55 56 57 41 54 41 55 41 56 41 57 48 8D A8 98 FE FF FF",
    ),
    at(
        "player camera update",
        "40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 74 24 50 48 8B D9 48 8B 49 28",
    ),
    returns(
        "jobs submit return",
        5,
        "E8 ?? ?? ?? ?? 49 8B CC E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 8B 88 28 01 00 00",
    ),
    returns(
        "jobs join return",
        5,
        "E8 ?? ?? ?? ?? 83 3D ?? ?? ?? ?? 02 74 13 48 8D 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 33 D2 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B C8 E8 ?? ?? ?? ?? 45 84 FF",
    ),
    returns(
        "world return",
        5,
        "E8 ?? ?? ?? ?? 90 C6 05 ?? ?? ?? ?? 00 43 89 1C 3E 48 8B 5C 24 60",
    ),
    // `call [rax+rdx*8]` of the named callback table.
    returns(
        "named poll return",
        3,
        "FF 14 D0 8B 4B 0C 41 83 C6 FF 0F 94 C0 40 0A E8",
    ),
    global(
        "renderer owner",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 88 48 21 41 8B 48 14",
    ),
    global(
        "native frame",
        2,
        6,
        "8B 0D ?? ?? ?? ?? 39 8B 04 01 00 00 72 0D",
    ),
    global(
        "active window",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 45 33 C0 48 8B 48 18",
    ),
    global(
        "renderer",
        3,
        7,
        "48 8D 0D ?? ?? ?? ?? 0F 57 DB F3 0F 11 7C 24 20",
    ),
    global(
        "immediate context",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? 45 33 C9 48 89 5C 24 40",
    ),
    global(
        "main depth",
        3,
        7,
        "48 8D 05 ?? ?? ?? ?? 4D 8B 04 00 48 8B 15 ?? ?? ?? ??",
    ),
    global(
        "shadow view",
        2,
        6,
        "8B 05 ?? ?? ?? ?? 0F 28 E2 0F C6 E3 EE 0F 28 F0",
    ),
    global(
        "raster ratio",
        4,
        8,
        "F3 0F 10 15 ?? ?? ?? ?? 0F 29 9D D0 01 00 00",
    ),
    global(
        "surface width",
        2,
        6,
        "8B 05 ?? ?? ?? ?? 48 8D 4C 24 20 89 44 24 20",
    ),
    global(
        "render state",
        3,
        7,
        "48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 40 32 F6 48 8B 0D ?? ?? ?? ??",
    ),
    // `mov byte [rip+disp32], 0`: the imm8 follows the displacement.
    global(
        "upload dirty",
        2,
        7,
        "C6 05 ?? ?? ?? ?? 00 48 83 C4 48 E9 ?? ?? ?? ?? CC CC",
    ),
    global(
        "TAA manager",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? 48 8D 55 D0 F3 41 0F 59 C1",
    ),
    global(
        "active camera",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? F3 0F 10 41 08 F3 0F 10 09",
    ),
    global(
        "DOF selector",
        2,
        6,
        "89 1D ?? ?? ?? ?? 4C 8D 45 00 44 89 75 00",
    ),
    global(
        "colour target",
        3,
        7,
        "48 63 05 ?? ?? ?? ?? 85 C0 78 08 48 6B D0 1C",
    ),
    global(
        "depth target",
        3,
        7,
        "48 63 05 ?? ?? ?? ?? 83 F8 FF 75 05 49 8B DF",
    ),
    global(
        "address modes",
        3,
        7,
        "44 39 3D ?? ?? ?? ?? 8B 05 ?? ?? ?? ?? 74 11 0F BA E8 00",
    ),
    global(
        "sampler table",
        3,
        7,
        "48 8D 1D ?? ?? ?? ?? BF 05 00 00 00 48 8B 0B",
    ),
    // The compare also fixes the UI pause counter at +160.
    global("UI", 3, 7, "48 8B 05 ?? ?? ?? ?? 44 39 A8 60 01 00 00"),
    // The add also fixes the loading-menu name at +E8.
    global(
        "UI names",
        3,
        7,
        "48 8B 15 ?? ?? ?? ?? 48 81 C2 E8 00 00 00 45 33 C9",
    ),
    global(
        "camera root",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 48 8B 8F 48 01 00 00",
    ),
    global(
        "main",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 8B 78 28 FF 15 ?? ?? ?? ?? 3B F8",
    ),
    global(
        "player",
        3,
        7,
        "48 8B 15 ?? ?? ?? ?? 4C 8D 44 24 30 48 8B D9",
    ),
    global(
        "job manager",
        3,
        7,
        "48 8B 1D ?? ?? ?? ?? 45 33 C0 BA FF FF FF FF",
    ),
    global(
        "input manager",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 90 EB 03 45 33 F6",
    ),
    global(
        "player controls",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 88 5C 01 4C 39 5D 23",
    ),
    global(
        "player camera",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? F3 0F 5C 7C 24 30",
    ),
];

/// The 1.7.104.0 RVAs, indexed by [`Site`]: documentation and the installed-image test.
#[cfg(test)]
const RECORDED: [usize; SITES] = [
    0x101_e360, 0x14e_dbf0, 0x153_ade7, 0x154_4df4, 0x101_e130, 0x100_9f70, 0x100_9960, 0x100_ae80,
    0x101_2320, 0x116_ac80, 0x101_0de0, 0x116_9530, 0x116_9590, 0x65_8870, 0xcf_95f0, 0x65_65b0,
    0xcf_a650, 0x65_6e60, 0x7a_d900, 0x8f_d590, 0x60_eaf0, 0x60_eb80, 0x77_6290, 0x74_5200,
    0x55_8e70, 0x65_8b14, 0x65_9334, 0x65_6e32, 0xeb_c248, 0x333_0188, 0x333_63f4, 0x333_0198,
    0x333_1f40, 0x333_1f30, 0x333_3f58, 0x333_64c8, 0x333_64b4, 0x333_63c4, 0x333_63a0, 0x333_6400,
    0x33d_40a0, 0x33d_3d28, 0x343_6358, 0x20c_fc18, 0x20c_fc38, 0x20c_fcbc, 0x333_1990, 0x219_e5c0,
    0x21a_05c8, 0x323_0740, 0x323_0530, 0x323_0778, 0x322_0a90, 0x320_4b30, 0x31a_5698, 0x31a_5478,
    0x15b_9412, 0x17c_92e0, 0x20b_5130, 0x6e_6030,
];

/// Field of the renderer reached only through its base (`renderer + A58`): the first
/// 0x30-byte render-target slot. No instruction addresses it directly.
const TARGET_SLOTS: usize = 0xa58;
/// The named callback table holds (function, name) pairs.
const NAMED_POLL_NAME: &[u8] = b"Poll controls\0";
/// `mov rcx,[rip+main]; jmp`: the poll thunk forwards the main object.
const NAMED_POLL: &str = "48 8B 0D ?? ?? ?? ?? E9 ?? ?? ?? ??";
/// `jmp [rip+slot]`: the import thunk.
const CREATE_THUNK: &str = "FF 25 ?? ?? ?? ??";

fn refused(name: &'static str) -> impl Fn(Miss) -> String {
    move |miss| Refusal { name, miss }.to_string()
}

/// The RIP-relative target of the `disp32` at `disp` of the `bytes` at `rva` ending at `next`.
fn rip_target(rva: usize, bytes: &[u8], disp: usize, next: usize) -> Option<usize> {
    let displacement = i32::from_le_bytes(bytes.get(disp..disp + 4)?.try_into().ok()?);
    rva.checked_add(next)?
        .checked_add_signed(displacement as isize)
}

/// Resolves the derived sites from the code and data sections, the pattern sites and an RVA reader.
fn derive(
    code: &[Section],
    data: &[Section],
    image_base: u64,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
    sites: &mut [usize; SITES],
) -> Result<(), String> {
    let slot = import_slot(&mut read, "d3d11.dll", "D3D11CreateDeviceAndSwapChain")
        .ok_or_else(|| refused("D3D11 import")(Miss::Missing))?;
    sites[Site::CreateThunk as usize] =
        find_rip_reference(code, &[0xff, 0x25], slot).map_err(refused("create thunk"))?;
    sites[Site::CreateImport as usize] = slot;
    // The callback table entry naming the poll holds its function just before the name.
    let name = find_unique(data, NAMED_POLL_NAME).map_err(refused("named poll name"))?;
    let pointer = image_base + name as u64;
    let entry = find_unique(data, &pointer.to_le_bytes()).map_err(refused("named poll entry"))?;
    let callback = entry - size_of::<u64>();
    let mut value = [0; 8];
    let poll = read(callback, &mut value)
        .then(|| u64::from_le_bytes(value).checked_sub(image_base))
        .flatten()
        .and_then(|rva| usize::try_from(rva).ok())
        .ok_or_else(|| refused("named poll")(Miss::Operand))?;
    // The thunk must forward the discovered main object.
    let mut thunk = [0; 12];
    if !read(poll, &mut thunk)
        || !Pattern::parse(NAMED_POLL).is_some_and(|pattern| pattern.matches(&thunk))
        || rip_target(poll, &thunk, 3, 7) != Some(sites[Site::Main as usize])
    {
        return Err("site 'named poll' does not forward main".into());
    }
    sites[Site::NamedCallbackSlot as usize] = callback;
    sites[Site::NamedPoll as usize] = poll;
    Ok(())
}

fn resolve_image(
    code: &[Section],
    data: &[Section],
    image_base: u64,
    read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<[usize; SITES], String> {
    let found = signature::resolve(code, &SIGNATURES).map_err(|refusal| refusal.to_string())?;
    let mut sites = [0; SITES];
    sites[..PATTERN_SITES].copy_from_slice(&found);
    derive(code, data, image_base, read, &mut sites)?;
    Ok(sites)
}

struct Discovery {
    sites: Result<[usize; SITES], CString>,
    sha256: CString,
}

static DISCOVERY: OnceLock<Discovery> = OnceLock::new();

fn discover(platform: &Platform) -> Discovery {
    let base = platform.main_module();
    let read = rsf_game_sys::win32::read_process;
    let code = signature::mapped_sections(base, read, |_, flags| flags & EXECUTE != 0);
    let data = signature::mapped_sections(base, read, |name, flags| {
        flags & EXECUTE == 0 && (name == b".rdata\0\0" || name == b".data\0\0\0")
    });
    let sites = match (code, data) {
        (Some(code), Some(data)) => resolve_image(
            &signature::views(&code),
            &signature::views(&data),
            base as u64,
            |rva, out| platform.read(base.wrapping_add(rva), out),
        ),
        _ => Err("executable sections unreadable".into()),
    };
    let refusal = |reason: String| {
        CString::new(format!("Skyrim native sites refused: {reason}.")).unwrap_or_default()
    };
    // Evidence only: the file hash names the build in logs and host setup, it admits nothing.
    let sha256 = platform
        .module_sha256(base)
        .and_then(|digest| CString::new(digest.to_vec()).ok())
        .unwrap_or_default();
    Discovery {
        sites: sites.map_err(refusal),
        sha256,
    }
}

/// Discovers every site once per process; false names the refusal in [`refusal`].
pub(crate) fn admit(platform: &Platform) -> bool {
    DISCOVERY.get_or_init(|| discover(platform)).sites.is_ok()
}

/// The discovery refusal, or null before discovery and when every site resolved.
pub(crate) fn refusal() -> *const c_char {
    match DISCOVERY.get().map(|discovery| &discovery.sites) {
        Some(Err(reason)) => reason.as_ptr(),
        _ => std::ptr::null(),
    }
}

/// Lowercase hex SHA-256 of the running executable for the host setup; empty before admission.
pub(crate) fn executable_sha256() -> &'static CStr {
    DISCOVERY
        .get()
        .map_or(c"", |discovery| discovery.sha256.as_c_str())
}

/// The discovered RVA. Zero before admission resolved every site: nothing hooks, patches or
/// reads a site before admission, and [`verify`] refuses an unresolved one.
pub(crate) fn rva(site: Site) -> usize {
    match DISCOVERY.get().map(|discovery| &discovery.sites) {
        Some(Ok(sites)) => sites[site as usize],
        _ => 0,
    }
}

/// The first render-target slot inside the renderer.
pub(crate) fn target_slots() -> usize {
    match rva(Site::Renderer) {
        0 => 0,
        renderer => renderer + TARGET_SLOTS,
    }
}

/// Whether the live bytes at a resolved code site still match its pattern, so a site changed
/// since discovery (another hook, or a wrong match) refuses before it is hooked or patched. The
/// creation thunk must also still jump through the D3D11 import slot.
pub(crate) fn verify(platform: &Platform, base: usize, site: Site) -> bool {
    let resolved = rva(site);
    let guard = match site {
        Site::CreateThunk => Some((CREATE_THUNK, resolved)),
        Site::NamedPoll => Some((NAMED_POLL, resolved)),
        _ => SIGNATURES
            .get(site as usize)
            .and_then(|signature| signature.guard(resolved)),
    };
    let Some((text, start)) = guard.filter(|_| resolved != 0 && base != 0) else {
        return false;
    };
    let Some(pattern) = Pattern::parse(text) else {
        return false;
    };
    let mut bytes = [0; 128];
    let Some(bytes) = bytes.get_mut(..pattern.len()) else {
        return false;
    };
    platform.read(base.wrapping_add(start), bytes)
        && pattern.matches(bytes)
        && (site != Site::CreateThunk
            || rip_target(resolved, bytes, 2, 6) == Some(rva(Site::CreateImport)))
}

/// SSE Display Tweaks' creation wrapper (where its relay of the game's creation call lands) and
/// its call of the retained game thunk, `mov r10,[rip+slot]` ... `call r10; mov ebx,eax`, whose
/// site is the call's return address. Derived from 0.5.25 with tools/derive-site-patterns.py
/// (docs/research/evidence/skyrim-display-tweaks-sites-0.5.25.spec); its file hash is logged only.
const DISPLAY_TWEAKS: [Signature; 2] = [
    at(
        "Display Tweaks creation wrapper",
        "40 53 55 56 57 41 54 41 55 41 56 41 57 48 81 EC C8 01 00 00",
    ),
    returns(
        "Display Tweaks thunk call",
        DISPLAY_CALL_RETURN,
        "4C 8B 15 ?? ?? ?? ?? 4C 8B C5 48 89 44 24 58 8B D6 8B 84 24 40 02 00 00 48 89 5C 24 50 4C 89 64 24 48 4C 89 7C 24 40 4C 89 6C 24 38 89 44 24 30 8B 84 24 38 02 00 00 89 44 24 28 48 89 7C 24 20 48 8B 7C 24 70 48 8B CF 41 FF D2 8B D8",
    ),
];
/// The return of `call r10` within the thunk call pattern.
const DISPLAY_CALL_RETURN: usize = 75;

/// Display Tweaks sites as module RVAs: the wrapper, the thunk call's pattern start and return,
/// and the retained slot holding the game thunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisplayTweaks {
    pub(crate) wrapper: usize,
    pub(crate) call: usize,
    pub(crate) call_return: usize,
    pub(crate) retained: usize,
}

/// The byte spans of the wrapper and thunk call patterns.
pub(crate) const DISPLAY_SPANS: [usize; 2] = [20, 77];

fn resolve_display_tweaks(
    code: &[Section],
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<DisplayTweaks, String> {
    let found = signature::resolve(code, &DISPLAY_TWEAKS).map_err(|refusal| refusal.to_string())?;
    let call = found[1] - DISPLAY_CALL_RETURN;
    let mut operand = [0; 7];
    let retained = read(call, &mut operand)
        .then(|| rip_target(call, &operand, 3, 7))
        .flatten()
        .ok_or_else(|| refused("Display Tweaks thunk call")(Miss::Operand))?;
    Ok(DisplayTweaks {
        wrapper: found[0],
        call,
        call_return: found[1],
        retained,
    })
}

/// Discovers the Display Tweaks sites in the module loaded at `module`.
pub(crate) fn display_tweaks(platform: &Platform, module: usize) -> Result<DisplayTweaks, String> {
    let code = signature::mapped_sections(module, rsf_game_sys::win32::read_process, |_, flags| {
        flags & EXECUTE != 0
    })
    .ok_or("Display Tweaks sections unreadable")?;
    resolve_display_tweaks(&signature::views(&code), |rva, out| {
        platform.read(module.wrapping_add(rva), out)
    })
}

/// Whether both Display Tweaks patterns still match in live memory at their discovered sites.
pub(crate) fn display_tweaks_current(
    platform: &Platform,
    module: usize,
    sites: &DisplayTweaks,
) -> bool {
    DISPLAY_TWEAKS
        .iter()
        .zip([sites.wrapper, sites.call_return])
        .all(|(signature, rva)| {
            signature.guard(rva).is_some_and(|(text, start)| {
                let Some(pattern) = Pattern::parse(text) else {
                    return false;
                };
                let mut bytes = vec![0; pattern.len()];
                platform.read(module.wrapping_add(start), &mut bytes) && pattern.matches(&bytes)
            })
        })
}

#[cfg(test)]
mod tests;
