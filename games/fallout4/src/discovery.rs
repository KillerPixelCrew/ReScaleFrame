// SPDX-License-Identifier: MIT
//! Signature discovery of every Fallout4.exe site this adapter hooks, patches, calls or reads.
//!
//! Preparation scans the loaded executable's code once and resolves each site from a pattern that
//! is unique in the code sections (rel32 branches and RIP displacements are wildcards), so a game
//! update that moves code keeps working while its patterns stay unique. Globals resolve through
//! the RIP-relative operand of an instruction that uses them. The rest follows from names and
//! discovered sites: the vtables from their RTTI class names and their slots, the worker job's
//! "JobListEnd" descriptor from the only slot that holds the job, the imports from the import
//! directory, the menu queries from the literal each one names, and three calls from their
//! callee inside an already discovered function.
//!
//! Every relation the old exact-build guards fixed through a call or RIP displacement is a
//! [`LINKS`] entry, checked at discovery and again in live memory. A site that is missing,
//! ambiguous or unlinked refuses preparation and names itself; before a site is hooked, patched
//! or trusted its pattern is matched again in live memory ([`verify`]).
//! Recorded RVAs are those of Steam 1.11.240.0 (`fdcef37a...646072f8`); the patterns were derived
//! with tools/derive-site-patterns.py from docs/research/evidence/fallout4-sites-1.11.240.spec,
//! and docs/research/fallout4-site-discovery.md records how each site is found.

use rsf_game_sys::signature::{
    self, EXECUTE, Find, Miss, Pattern, Refusal, Section, Signature, find_rip_reference,
    find_unique, import_slot,
};
use std::ffi::{CStr, CString};
use std::sync::OnceLock;

/// Every discovered site. The pattern sites come first, in [`SIGNATURES`] order. A call site is
/// its return address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Site {
    // CPU producer hooks.
    Swap,
    Keyboard,
    Mouse,
    Idle,
    InputPoll,
    Consume,
    PlayerControls,
    Wrapper,
    JobEnd,
    WorkerJoin,
    // Calls whose return address selects a CPU callback.
    SwapCall,
    JoinCall,
    KeyboardCall,
    MouseCall,
    IdleCall,
    MainPollCall,
    ConsumeCall,
    WrapperPollCall,
    WorkerPollCall,
    WorkerConsumeCall,
    JobCallback,
    PlayerControlsCall,
    Peek1,
    Peek2,
    Peek3,
    Peek4,
    // Input notification evidence.
    ControlsEvents,
    ControlsKind,
    EnqueueSequence,
    DeviceEnqueue1,
    DeviceEnqueue2,
    QueueDevice,
    DeviceEnqueue3,
    DeviceEnqueue4,
    QueueKind,
    QueueAppend,
    GamepadState,
    // Render hooks.
    DrWriter,
    Prepass,
    OuterPresent,
    WorldBegin,
    ViewportStore,
    Imagespace,
    Effects,
    // Render calls.
    PresentCall,
    PrepassCall,
    DrCall,
    WorldCall,
    ImagespaceCall1,
    ImagespaceCall2,
    ImagespaceCall3,
    BuilderCall,
    SetterCall,
    // Called functions.
    CameraUpdate,
    CameraRead,
    Builder,
    TemporalUpdate,
    ViewportBuilder,
    // Render evidence.
    CameraReads,
    DepthCamera,
    DepthBuild,
    WorldRanges,
    BuilderViewport,
    BuilderScale,
    BuilderOwner,
    TemporalSize,
    DrSlots,
    DrSelect,
    DrStore,
    DrIndex,
    DrEntry,
    DrRatio,
    DrCount,
    DrViewport,
    ShadowWriter,
    TargetSelect,
    TargetView,
    TargetBind,
    EffectsSize,
    RendererInit,
    FrameRead,
    DeltaRead,
    JitterStore,
    // Screen queries.
    MenuOpen,
    FreezeBackground,
    FreezePause,
    CameraEquals,
    // Creation.
    CreateCall,
    // Globals.
    Main,
    InputManager,
    EventQueue,
    Controls,
    Worker,
    WorkerObject,
    RendererPointer,
    RendererObject,
    RenderContext,
    RendererWindow,
    PresentRenderer,
    TlsIndex,
    State,
    Targets,
    WorldGraph,
    Delta,
    ImagespaceManager,
    ViewportUnit,
    JitterScale,
    DepthMode,
    NearInterval,
    WorldInterval,
    NearCamera,
    WorldCamera,
    Ui,
    PlayerCamera,
    // Derived from names, the import directory and discovered sites, not from a code pattern.
    JobName,
    JobDescriptor,
    WorkerVtable,
    WorkerDestructor,
    WorkerEntry,
    ControlsVtable,
    TaaVtable,
    TaaQuery,
    CameraVtable,
    PeekImport,
    WaitImport,
    GamepadImport,
    CreateImport,
    CreateThunk,
    WrapperCall,
    EffectsCall,
    HudlessCall,
    LoadingMenu,
    MainMenu,
    PauseMenu,
    PipboyMenu,
    MapMenu,
    VatsMenu,
    LoadingMenuName,
    MainMenuName,
    PauseMenuName,
    PipboyMenuName,
    MapMenuName,
    VatsMenuName,
}

const PATTERN_SITES: usize = Site::JobName as usize;
const SITES: usize = Site::VatsMenuName as usize + 1;

/// A five-byte `call rel32`.
pub(crate) const CALL: usize = 5;

const fn at(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(0),
        check: "",
    }
}

/// A call site found from its own start: the site is its return address, `length` bytes on.
const fn returns(name: &'static str, length: usize, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(length),
        check: "",
    }
}

/// A function found through a distinctive call of it, with its own entry checked.
const fn called(name: &'static str, pattern: &'static str, check: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::Rip { disp: 1, next: 5 },
        check,
    }
}

/// A global addressed by the `disp32` at `disp` of the instruction ending at `next`, and the
/// value expected there when it is a constant.
const fn global(name: &'static str, disp: usize, next: usize, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::Rip { disp, next },
        check: "",
    }
}

const fn constant(
    name: &'static str,
    disp: usize,
    next: usize,
    pattern: &'static str,
    value: &'static str,
) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::Rip { disp, next },
        check: value,
    }
}

/// The first imagespace call; it also finds imagespace itself.
const IMAGESPACE_CALL_1: &str = "E8 ?? ?? ?? ?? 8B 46 20 85 C0 74 28 83 E8 01 89 46 20";

/// Indexed by [`Site`]. Derived from 1.11.240.0 by tools/derive-site-patterns.py.
const SIGNATURES: [Signature; PATTERN_SITES] = [
    at(
        "Swap",
        "48 89 5C 24 08 57 48 81 EC 40 01 00 00 48 8B 05 ?? ?? ?? ?? 48 8B D9",
    ),
    at(
        "keyboard RAWINPUT",
        "48 89 5C 24 10 48 89 6C 24 18 56 48 83 EC 20 0F B7 5A 1E",
    ),
    at(
        "mouse RAWINPUT",
        "0F B7 42 1C A8 01 74 08 80 49 71 01 0F B7 42 1C",
    ),
    at(
        "Main idle",
        "48 8B C4 48 89 58 08 48 89 70 10 48 89 78 18 55 41 54 41 55 41 56 41 57 48 81 EC C0 01 00 00",
    ),
    // `cmp byte [rcx+35],0` fixes the manager's polling flag.
    at(
        "input device poll",
        "48 83 EC 38 80 79 35 00 0F 29 74 24 20 0F 28 F1",
    ),
    // The whole consume body: the queue lock, its sequence at +1490, the delivered cursor at
    // +8 and the PlayerControls dispatch.
    at(
        "input event consume",
        "40 53 48 83 EC 20 48 89 6C 24 30 48 8D 99 78 14 00 00 48 8B E9 48 89 74 24 38 48 8B CB 48 8B F2 E8 ?? ?? ?? ?? 83 7E 08 FF 48 8B CB 75 1D E8 ?? ?? ?? ?? 8B 85 90 14 00 00 89 46 08 48 85 DB 74 58 48 8B CB E8 ?? ?? ?? ?? EB 4E 48 89 7C 24 40 E8 ?? ?? ?? ?? 48 8B BD 80 14 00 00 0F 1F 40 00 48 85 FF 74 0E 8B 46 08 39 47 20 73 06 48 8B 7F 18 EB ED 8B 85 90 14 00 00 89 46 08 48 85 DB 74 08 48 8B CB E8 ?? ?? ?? ?? 48 8B 06 48 8B D7 48 8B CE FF 10 48 8B 7C 24 40 48 8B 74 24 38 48 8B 6C 24 30 48 85 DB 74 0D 48 8B CB 48 83 C4 20 5B E9 ?? ?? ?? ??",
    ),
    at(
        "PlayerControls events",
        "48 8B C4 56 57 48 83 EC 68 F3 0F 10 05 ?? ?? ?? ??",
    ),
    at(
        "worker poll wrapper",
        "48 83 EC 28 F3 0F 10 0D ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ??",
    ),
    // Its descriptor is the only data slot that holds it.
    at(
        "JobListEnd job",
        "48 89 5C 24 08 57 48 83 EC 30 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ??",
    ),
    // The join through its wait: worker mode at +68, completion event at +60.
    at(
        "worker join",
        "48 8B 0D ?? ?? ?? ?? 48 85 C9 74 20 83 79 68 01 75 1A C7 41 68 00 00 00 00 45 33 C0 48 8B 49 60 BA FF FF FF FF 48 FF 25 ?? ?? ?? ??",
    ),
    returns(
        "Swap call",
        CALL,
        "E8 ?? ?? ?? ?? 33 C9 E8 ?? ?? ?? ?? 0F B6 D8 E8 ?? ?? ?? ?? 83 3D ?? ?? ?? ?? 02",
    ),
    returns(
        "worker join call",
        CALL,
        "E8 ?? ?? ?? ?? 83 3D ?? ?? ?? ?? 02 74 13 48 8D 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B C8 E8 ?? ?? ?? ?? 84 DB",
    ),
    returns(
        "keyboard call",
        CALL,
        "E8 ?? ?? ?? ?? 81 BD C8 01 00 00 00 01 00 00 0F 86 ?? ?? ?? ??",
    ),
    returns(
        "mouse call",
        CALL,
        "E8 ?? ?? ?? ?? EB 20 83 F8 01 75 1B 48 8B 0D ?? ?? ?? ??",
    ),
    // `mov rcx,[rip+Main]; call idle`.
    returns(
        "idle call",
        12,
        "48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 1D ?? ?? ?? ?? 80 7B 24 00",
    ),
    // `mov rcx,[rip+manager]; call poll`.
    returns(
        "main poll call",
        12,
        "48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ??",
    ),
    returns(
        "consume call",
        CALL,
        "E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00",
    ),
    returns(
        "wrapper poll call",
        CALL,
        "E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 83 C4 28",
    ),
    returns(
        "worker poll call",
        CALL,
        "E8 ?? ?? ?? ?? 41 83 7E 68 02 0F 84 ?? ?? ?? ?? 33 C9",
    ),
    returns(
        "worker consume call",
        CALL,
        "E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 83 C4 28 E9 ?? ?? ?? ?? CC",
    ),
    // The job list's `call [rax]` of each job's function.
    returns(
        "job callback",
        13,
        "48 8B 47 10 48 85 C0 74 04 33 C9 FF 10 48 83 C3 08 48 FF C6",
    ),
    // The receiver's virtual event call.
    returns(
        "PlayerControls call",
        11,
        "48 8B 06 48 8B D7 48 8B CE FF 10 48 8B 7C 24 40",
    ),
    // `PeekMessageA(..., PM_REMOVE)` and the test of its result.
    returns(
        "PeekMessageA call 1",
        0x17,
        "C7 44 24 20 01 00 00 00 45 33 C0 48 8D 4D FF 33 D2 FF 15 ?? ?? ?? ?? 85 C0 74 34",
    ),
    returns(
        "PeekMessageA call 2",
        0x17,
        "C7 44 24 20 01 00 00 00 45 33 C0 48 8D 4D FF 33 D2 FF 15 ?? ?? ?? ?? 85 C0 75 CE",
    ),
    returns(
        "PeekMessageA call 3",
        0x32,
        "C7 44 24 20 01 00 00 00 45 33 C9 49 89 43 C8 45 33 C0 49 89 43 D0 33 D2 49 89 43 D8 49 8D 4B C8 49 89 43 E0 49 89 43 E8 49 89 43 F0 FF 15 ?? ?? ?? ?? 85 C0 74 36",
    ),
    returns(
        "PeekMessageA call 4",
        0x18,
        "C7 44 24 20 01 00 00 00 45 33 C0 48 8D 4C 24 30 33 D2 FF 15 ?? ?? ?? ?? 85 C0 75 CB 48 83 C4 68",
    ),
    at(
        "PlayerControls event walk",
        "48 8B 5B 18 48 85 DB 75 EC 48 8D 0D ?? ?? ?? ??",
    ),
    at(
        "PlayerControls event kind",
        "48 63 45 10 83 F8 06 77 6C 8B 94 87 84 C5 DD 00",
    ),
    at(
        "event enqueue sequence",
        "8B 86 90 14 00 00 89 47 20 FF 86 90 14 00 00 48 C7 47 18 00 00 00 00",
    ),
    at(
        "device enqueue 1",
        "E8 ?? ?? ?? ?? 48 8D 7F 02 48 83 EE 01 75 CF 0F B6 93 E6 00 00 00",
    ),
    at(
        "device enqueue 2",
        "E8 ?? ?? ?? ?? 48 8B 5C 24 70 48 8B 6C 24 78 48 8B B4 24 80 00 00 00 0F 28 74 24 50",
    ),
    at(
        "queued event device",
        "44 89 7F 08 C7 47 24 00 00 00 00 44 89 77 0C 89 6F 30",
    ),
    at(
        "device enqueue 3",
        "E8 ?? ?? ?? ?? 0F 2E B3 D8 00 00 00 4C 8B 74 24 68",
    ),
    at(
        "device enqueue 4",
        "E8 ?? ?? ?? ?? 4C 8B 7C 24 60 48 8B 74 24 70 0F 28 74 24 50",
    ),
    at(
        "queued event kind",
        "83 78 10 04 75 0B 83 78 20 FF 74 05 48 8B C8 EB 07",
    ),
    at(
        "queued event append",
        "C7 47 08 02 00 00 00 44 89 77 0C C7 47 24 00 00 00 00 89 6F 30 E8 ?? ?? ?? ?? 48 8B D0 48 8D 4F 28 E8 ?? ?? ?? ?? 48 8B D7 C6 47 34 00 48 8B CE E8 ?? ?? ?? ??",
    ),
    // `jmp [rip+XInputGetState]`.
    at(
        "gamepad state",
        "FF 25 ?? ?? ?? ?? CC CC CC 48 8D 05 ?? ?? ?? ?? C3 CC",
    ),
    at(
        "dynamic resolution writer",
        "48 83 EC 18 8B 05 ?? ?? ?? ?? 4C 8B D1 85 C0 0F 94 C1",
    ),
    // Through both thread-local render-context reads and their immediate-context fallback.
    at(
        "depth prepass",
        "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 56 48 83 EC 50 48 8B 05 ?? ?? ?? ?? 44 8B 05 ?? ?? ?? ?? 41 B9 20 0B 00 00 80 B8 26 02 00 00 00 74 39 65 48 8B 04 25 58 00 00 00 4A 8B 04 C0 49 8B 04 01 48 85 C0 75 07 48 8B 05 ?? ?? ?? ?? 48 05 70 1B 00 00 83 B8 B8 00 00 00 01 74 0D 83 08 08 C7 80 B8 00 00 00 01 00 00 00 65 48 8B 04 25 58 00 00 00 41 8B F1 4A 8B 3C C0 49 8B 04 39 48 85 C0 75 07 48 8B 05 ?? ?? ?? ?? 48 05 70 1B 00 00",
    ),
    // Through both Presents: the window cell, its chain at +18 and IDXGISwapChain::Present (+40).
    at(
        "outer Present",
        "40 53 48 83 EC 20 48 8B D9 B9 01 00 00 00 E8 ?? ?? ?? ?? B9 02 00 00 00 E8 ?? ?? ?? ?? 80 3B 00 75 7A 48 8B 05 ?? ?? ?? ?? 44 8B 05 ?? ?? ?? ?? C7 05 ?? ?? ?? ?? 01 00 00 00 48 8B 48 18 48 8B 05 ?? ?? ?? ?? 4C 8B 09 8B 50 40 41 FF 51 40 83 3D ?? ?? ?? ?? 08 75 38 3D 0A 00 7A 88 75 31 90 FF 15 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 44 8B 05 ?? ?? ?? ?? 48 8B 48 18 48 8B 05 ?? ?? ?? ?? 4C 8B 09 8B 50 40 41 FF 51 40",
    ),
    // Through its viewport selection call.
    at(
        "WorldBegin",
        "48 8B C4 53 48 81 EC B0 00 00 00 0F 29 70 E8 48 8D 0D ?? ?? ?? ?? 0F 29 78 D8 B2 01 44 0F 29 40 C8 44 0F 29 48 B8 E8 ?? ?? ?? ??",
    ),
    // `mov [rcx+FE4],dl; ret`: the viewport selector.
    at(
        "viewport selector store",
        "88 91 E4 0F 00 00 C3 CC CC CC CC CC CC CC CC CC",
    ),
    // Its entry is shared with a sibling; the first of its three calls is unique.
    called(
        "imagespace",
        IMAGESPACE_CALL_1,
        "48 89 5C 24 10 56 48 83 EC 40 8B 0D ?? ?? ?? ?? 45 33 C9",
    ),
    at(
        "imagespace Effects range",
        "48 89 5C 24 18 55 56 57 41 55 41 56 48 83 EC 30",
    ),
    // `lea rcx,[rip+renderer]; call outer Present`.
    returns(
        "outer Present call",
        12,
        "48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? B9 07 00 00 00",
    ),
    // The camera update, an untouched call, then the prepass.
    returns(
        "prepass call",
        15,
        "E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00 74 75 33 D2",
    ),
    returns(
        "dynamic resolution call",
        CALL,
        "E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B BC 24 D8 00 00 00",
    ),
    returns(
        "ordinary WorldBegin call",
        CALL,
        "E8 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00 74 10 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ??",
    ),
    returns("imagespace call 1", CALL, IMAGESPACE_CALL_1),
    returns(
        "imagespace call 2",
        CALL,
        "E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? BB 01 00 00 00",
    ),
    returns(
        "imagespace call 3",
        CALL,
        "E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 40 0F B6 C7 48 8B 5C 24 50",
    ),
    at(
        "camera builder call",
        "E8 ?? ?? ?? ?? 48 8B 5C 24 50 48 8B 6C 24 58 48 8B 74 24 60 48 83 C4 40 5F C3 CC CC CC CC CC CC CC CC CC 8B 81 50 01 00 00",
    ),
    at(
        "camera setter call",
        "E8 ?? ?? ?? ?? 48 83 C4 38 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC CC 40 55",
    ),
    at(
        "camera update",
        "48 83 EC 38 33 D2 48 8D 0D ?? ?? ?? ?? 44 8D 4A 03",
    ),
    // Through the camera cache's slot list at +140 and its entry key at +238/+240.
    at(
        "camera cache read",
        "40 57 48 83 EC 40 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 50 48 89 6C 24 58 48 89 74 24 60 41 0F B6 D8 48 8B FA 48 8B E9 33 D2 48 8D B1 40 01 00 00 8B 46 10 85 C0 74 39 4C 8B 06 48 69 C8 50 02 00 00 49 03 C8 4C 3B C1 74 1E 49 39 B8 38 02 00 00 75 09 41 38 98 40 02 00 00 74 09",
    ),
    at(
        "camera builder",
        "48 8B C4 48 89 58 08 48 89 68 10 48 89 70 18 48 89 78 20 41 56 48 81 EC A0 00 00 00 F3 41 0F 10 80 88 00 00 00",
    ),
    at(
        "temporal update",
        "40 57 48 83 EC 40 83 B9 AC 00 00 00 00 48 8B F9",
    ),
    // The explicit-size viewport builder through its first branch.
    at(
        "viewport builder",
        "40 53 48 83 EC 60 0F 29 74 24 50 48 8B DA F3 0F 10 35 ?? ?? ?? ?? 0F 29 7C 24 40 44 0F 29 44 24 30 44 0F 28 C6 44 0F 29 4C 24 20 44 0F 28 CE 45 85 C0 74 10 41 8B C0 0F 57 FF F3 48 0F 2A F8 41 8B C1 EB 63",
    ),
    // Near, world and world camera reads into the graphics state.
    at(
        "camera cache reads",
        "48 8B 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? 41 B0 01 E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? 41 B0 01 E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? 45 33 C0 E8 ?? ?? ?? ??",
    ),
    at(
        "depth camera matrices",
        "0F 10 80 60 07 00 00 48 8D 4C 24 40 0F 10 88 70 07 00 00 0F 10 90 80 07 00 00 0F 10 98 90 07 00 00 E8 ?? ?? ?? ?? 0F 28 E0 0F C6 C1 EE 0F C6 E1 44 0F 28 CA 0F C6 CB 44 0F C6 D3 EE 0F 28 DC 0F C6 D9 88 0F C6 E1 DD 0F 28 C8 0F C6 CA 88 0F C6 C2 DD 4D 85 F6 75 07 48 8D 9D 70 1B 00 00 0F 11 9B A0 08 00 00 0F 11 A3 B0 08 00 00 0F 11 8B C0 08 00 00 0F 11 83 D0 08 00 00",
    ),
    at(
        "depth build call",
        "E8 ?? ?? ?? ?? 48 8B 5C 24 60 48 8B 6C 24 68 48 8B 74 24 70 48 8B 7C 24 78 48 83 C4 50 41 5E C3 CC CC CC CC CC CC CC CC CC CC C6 05 ?? ?? ?? ?? 01",
    ),
    // The near and world depth ranges and the temporal update call.
    at(
        "world depth ranges",
        "84 C0 75 04 44 0F 28 C8 F3 44 0F 11 0D ?? ?? ?? ?? 84 C0 75 03 0F 28 F0 48 8D 0D ?? ?? ?? ?? F3 0F 11 35 ?? ?? ?? ?? E8 ?? ?? ?? ??",
    ),
    at(
        "camera builder viewport",
        "E8 ?? ?? ?? ?? 0F 11 83 D0 00 00 00 0F 11 83 10 01 00 00 0F 11 8B E0 00 00 00 0F 11 8B 20 01 00 00 0F 11 93 F0 00 00 00 0F 11 93 30 01 00 00 0F 11 9B 00 01 00 00 0F 11 9B 40 01 00 00 41 83 BE AC 00 00 00 00 0F 84 ?? ?? ?? ?? 40 84 ED 0F 84 ?? ?? ?? ??",
    ),
    at(
        "camera builder scale",
        "F3 41 0F 59 5E 04 0F 57 C0 F3 41 0F 59 56 08 0F 28 E3",
    ),
    at(
        "camera builder owner",
        "89 83 10 02 00 00 8B 86 A4 00 00 00 89 83 14 02 00 00 8B 86 A8 00 00 00 89 83 18 02 00 00 48 89 B3 38 02 00 00 49 8B 73 20 40 88 AB 40 02 00 00",
    ),
    at(
        "temporal size",
        "F3 0F 11 7F 04 F3 0F 11 5F 08 0F 28 7C 24 20 48 83 C4 40",
    ),
    at(
        "dynamic resolution slots",
        "48 63 88 F4 1B 00 00 48 05 70 1B 00 00 85 C9 78 0C 48 8B C1 48 C1 E0 05 42 8B 04 00 C3 48 63 88 88 00 00 00 85 C9 7E 0D 48 6B C9 1C 42 8B 84 01 80 0C 00 00 C3 33 C0 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC 8B 15 ?? ?? ?? ?? 4C 8B C1 65 48 8B 04 25 58 00 00 00 B9 20 0B 00 00 48 8B 04 D0 48 8B 04 01 48 85 C0 75 07 48 8B 05 ?? ?? ?? ?? 48 63 88 F4 1B 00 00 48 05 70 1B 00 00 85 C9 78 0D 48 8B C1 48 C1 E0 05 42 8B 44 00 04",
    ),
    at(
        "dynamic resolution selection",
        "85 D2 75 07 44 89 83 84 00 00 00 41 83 F8 FF 75 05 41 8B F0 EB 0A 49 63 C0 8B B4 81 F4 0D 00 00",
    ),
    at(
        "dynamic resolution store",
        "89 74 AB 2C C7 43 54 FF FF FF FF 89 7C AB 5C 83 0B 01",
    ),
    at(
        "dynamic resolution index",
        "48 63 C2 41 89 92 88 00 00 00 8B 8C 81 84 0F 00 00",
    ),
    at(
        "dynamic resolution entry",
        "48 83 EC 38 8B 15 ?? ?? ?? ?? 65 48 8B 04 25 58 00 00 00 41 B8 20 0B 00 00 48 8B 04 D0 49 8B 14 00 48 85 D2 75 07 48 8B 15 ?? ?? ?? ?? 48 81 C2 70 1B 00 00 80 B9 E4 0F 00 00 00",
    ),
    at(
        "dynamic resolution ratio",
        "41 C6 82 E5 0F 00 00 01 48 83 C4 18 C3 CC CC CC",
    ),
    at(
        "dynamic resolution count",
        "F0 FF 81 E8 0F 00 00 C3 83 B9 E8 0F 00 00 00 76 07",
    ),
    at(
        "dynamic resolution viewport",
        "48 63 8C 81 84 0F 00 00 48 69 C1 98 00 00 00 8B 0D ?? ?? ?? ?? 4E 8B 0C 08 65 48 8B 04 25 58 00 00 00 48 8B 04 C8 49 8B 0C 00 48 85 C9 75 07 48 8B 0D ?? ?? ?? ?? 48 81 C1 70 1B 00 00 44 8B C2 4E 39 8C C1 E0 03 00 00 74 38 4E 89 8C C1 E0 03 00 00 8B 41 14 83 09 01 0F AB D0 89 41 14 C6 41 28 01",
    ),
    // The common shadow viewport writer through its return.
    at(
        "shadow viewport writer",
        "8B 0D ?? ?? ?? ?? 0F 57 D2 F3 48 0F 2A D0 65 48 8B 04 25 58 00 00 00 BA 20 0B 00 00 48 8B 04 C8 48 8B 0C 02 48 85 C9 75 07 48 8B 0D ?? ?? ?? ?? 48 81 C1 70 1B 00 00 0F 28 C7 F3 0F 59 81 D0 06 00 00 F3 0F 11 83 90 00 00 00 F3 0F 5C B1 D8 06 00 00 F3 0F 59 F2 F3 0F 11 B3 94 00 00 00 F3 0F 10 81 D4 06 00 00 F3 0F 5C 81 D0 06 00 00 0F 28 74 24 50 F3 0F 59 C7 0F 28 7C 24 40 F3 41 0F 59 C0 44 0F 28 44 24 30 F3 0F 11 83 98 00 00 00 F3 0F 10 89 D8 06 00 00 F3 0F 5C 89 DC 06 00 00 F3 0F 59 CA F3 41 0F 59 C9 44 0F 28 4C 24 20 F3 0F 11 8B 9C 00 00 00 8B 81 E0 06 00 00 89 83 A0 00 00 00 8B 81 E4 06 00 00 83 0B 02 89 83 A4 00 00 00 48 83 C4 60 5B C3",
    ),
    at(
        "render target selection",
        "48 63 47 D0 83 F8 FF 74 47 48 8D 0C 40 48 8B 05 ?? ?? ?? ?? 48 03 C9 83 3F 00 48 8B 94 C8 68 0A 00 00",
    ),
    at(
        "render target view",
        "48 8B 01 FF 90 08 01 00 00 80 3D ?? ?? ?? ?? 00",
    ),
    at(
        "render target bind",
        "4C 8D 87 90 00 00 00 BA 01 00 00 00 48 8B 01 FF 90 60 01 00 00",
    ),
    // The pattern starts 105 bytes before the stride selection it guards.
    Signature {
        name: "Effects vertex stride",
        pattern: "41 8B D0 88 44 24 20 49 89 43 B0 F3 0F 7F 44 24 30 89 44 24 40 66 89 44 24 44 F3 0F 7F 44 24 58 89 44 24 68 66 89 44 24 6C E8 ?? ?? ?? ?? 8B D7 48 8D 4C 24 20 E8 ?? ?? ?? ?? 48 85 F6 0F 84 ?? ?? ?? ?? 45 33 C9 4C 8D 44 24 20 33 D2 48 8B CE E8 ?? ?? ?? ?? 45 33 C9 4C 8D 44 24 48 48 8B CE 41 8D 51 01 E8 ?? ?? ?? ?? 80 7D 48 00 74 07 BA 40 00 00 00 EB 1C 80 BE A8 00 00 00 00 74 0E 80 3D ?? ?? ?? ?? 00 BA 38 00 00 00 75 05 BA 28 00 00 00",
        find: Find::At(105),
        check: "",
    },
    // Renderer initialisation: the renderer object and its global pointer.
    at(
        "renderer initialisation",
        "48 8D 1D ?? ?? ?? ?? 48 8B CB E8 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 45 33 C0 65 48 8B 04 25 58 00 00 00 BA 20 0B 00 00 4C 89 05 ?? ?? ?? ?? 4C 89 05 ?? ?? ?? ?? 48 8B 04 C8 4C 89 05 ?? ?? ?? ?? 44 88 05 ?? ?? ?? ?? 48 89 1D ?? ?? ?? ??",
    ),
    at(
        "native frame read",
        "8B 05 ?? ?? ?? ?? 41 39 86 BC 00 00 00 0F 84 ?? ?? ?? ??",
    ),
    at(
        "frame delta read",
        "48 8D 0D ?? ?? ?? ?? 33 D2 48 69 C0 E8 03 00 00 48 F7 35 ?? ?? ?? ?? 48 8B D0 E8 ?? ?? ?? ?? F3 0F 10 05 ?? ?? ?? ?? F3 0F 11 05 ?? ?? ?? ?? 0F B6 87 D0 01 00 00",
    ),
    at(
        "jitter store",
        "F3 41 0F 11 50 10 0F 28 C2 49 2B 40 28 49 89 40 18 F3 41 0F 11 40 14 49 89 50 20 F3 0F 59 05 ?? ?? ?? ?? 4D 89 48 28 F3 41 0F 11 40 10",
    ),
    // Through the menu entry's flags test at +58.
    at(
        "menu open query",
        "48 89 5C 24 08 57 48 83 EC 40 33 C0 48 8B D9 48 8D 0D ?? ?? ?? ?? 48 89 44 24 20 48 89 44 24 28 48 8B FA 48 89 44 24 30 E8 ?? ?? ?? ?? 48 8D 8B A8 01 00 00 48 8B D7 4C 8D 44 24 20 E8 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 4C 24 20 48 85 C9 74 0A F6 41 58 40",
    ),
    at(
        "background freeze query",
        "83 B9 EC 01 00 00 00 0F 97 C0 C3 CC CC CC CC CC",
    ),
    at(
        "pause freeze query",
        "83 B9 F0 01 00 00 00 0F 97 C0 C3 CC CC CC CC CC",
    ),
    // The camera state compare against slot +E0.
    at(
        "camera state query",
        "4C 8B 41 28 4D 85 C0 74 0F 8B C2 4C 3B 84 C1 E0 00 00 00 75 03 B0 01 C3 32 C0 C3",
    ),
    // Then the renderer's device (+48), context (+50) and window (+58) and the thread's render
    // context slot (+B18).
    returns(
        "creation call",
        CALL,
        "E8 ?? ?? ?? ?? 4C 8B 05 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? BF 18 0B 00 00 49 8D 50 58 48 89 15 ?? ?? ?? ?? 49 8B 40 48 48 89 05 ?? ?? ?? ?? 65 48 8B 04 25 58 00 00 00 48 8B 1C C8 49 8B 40 50 41 B8 03 00 00 00 48 8B 4C 24 70 48 89 05 ?? ?? ?? ?? 48 89 04 3B",
    ),
    global("Main", 3, 7, "48 8B 05 ?? ?? ?? ?? 48 85 C0 74 13 38 58 29"),
    global(
        "input device manager",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? 48 89 45 D7 48 8D 05 ?? ?? ?? ??",
    ),
    global(
        "input event queue",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? 48 8D 93 A8 00 00 00 E8 ?? ?? ?? ??",
    ),
    global(
        "PlayerControls",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 80 B8 8A 00 00 00 00 75 29",
    ),
    global(
        "job list worker",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 48 85 C0 74 12 85 C9",
    ),
    global(
        "job list worker object",
        3,
        7,
        "48 89 05 ?? ?? ?? ?? 45 33 C0 33 D2 33 C9",
    ),
    global(
        "renderer pointer",
        3,
        7,
        "48 89 1D ?? ?? ?? ?? 48 8B 04 02 48 85 C0",
    ),
    global(
        "renderer object",
        3,
        7,
        "48 89 05 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? 66 0F 7F 05 ?? ?? 89 03",
    ),
    global(
        "immediate render context",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 48 05 70 1B 00 00 33 FF",
    ),
    global(
        "renderer window",
        3,
        7,
        "48 89 05 ?? ?? ?? ?? 48 8B 40 30 48 89 85 78 0A 00 00",
    ),
    global(
        "Present renderer",
        3,
        7,
        "48 8D 0D ?? ?? ?? ?? C6 86 25 02 00 00 01",
    ),
    global(
        "render context TLS index",
        2,
        6,
        "8B 0D ?? ?? ?? ?? BA 70 15 00 00 48 8B 0C C8 8B 04 0A 39 05 ?? ?? ?? ?? 7F 1A 48 8D 05 ?? ?? ?? ?? C6 05 ?? ?? ?? ?? 00",
    ),
    global(
        "graphics state",
        3,
        7,
        "48 8D 0D ?? ?? ?? ?? 45 0F B6 C5 49 8B D6",
    ),
    global(
        "render targets",
        3,
        7,
        "48 8D 1D ?? ?? ?? ?? 83 B8 A0 02 00 00 14",
    ),
    // The load also fixes the world camera at graph + 140.
    global(
        "world scene graph",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 48 8B 88 40 01 00 00 E8 ?? ?? ?? ??",
    ),
    global(
        "frame delta",
        4,
        8,
        "F3 0F 10 0D ?? ?? ?? ?? 76 10 F3 0F 5C C1",
    ),
    global(
        "imagespace manager",
        3,
        7,
        "48 89 3D ?? ?? ?? ?? 0F 1F 40 00 F2 0F 11 08",
    ),
    // 1.0f.
    constant(
        "viewport unit",
        4,
        8,
        "F3 0F 10 0D ?? ?? ?? ?? 0F 5B C0 F3 0F 5E C8",
        "00 00 80 3F",
    ),
    // 0.001f.
    constant(
        "jitter scale",
        5,
        9,
        "F3 44 0F 59 25 ?? ?? ?? ?? 48 8B 4C 24 28",
        "6F 12 83 3A",
    ),
    global(
        "depth range mode",
        3,
        7,
        "0F B6 05 ?? ?? ?? ?? F3 0F 11 1D ?? ?? ?? ?? 0F 57 D8",
    ),
    global(
        "near depth interval",
        4,
        8,
        "F3 0F 10 1D ?? ?? ?? ?? 4C 8B 05 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ??",
    ),
    global(
        "world depth interval",
        5,
        9,
        "F3 44 0F 11 0D ?? ?? ?? ?? 84 C0 75 03 0F 28 F0",
    ),
    global(
        "near camera",
        3,
        7,
        "48 8B 05 ?? ?? ?? ?? 0F 10 83 84 01 00 00",
    ),
    global(
        "world camera",
        3,
        7,
        "48 8B 1D ?? ?? ?? ?? 0F 29 B4 24 80 00 00 00",
    ),
    global(
        "UI",
        3,
        7,
        "48 89 05 ?? ?? ?? ?? 4C 8D 0D ?? ?? ?? ?? 48 8B C8 4C 8D 05 ?? ?? ?? ??",
    ),
    global(
        "PlayerCamera",
        3,
        7,
        "48 8B 0D ?? ?? ?? ?? 48 89 78 D8 C6 05 ?? ?? ?? ?? 00",
    ),
];

/// Names of the derived sites, from [`Site::JobName`] on.
const DERIVED: [&str; SITES - PATTERN_SITES] = [
    "JobListEnd name",
    "JobListEnd descriptor",
    "job list worker vtable",
    "job list worker destructor",
    "job list worker entry",
    "PlayerControls vtable",
    "TAA vtable",
    "TAA active query",
    "PlayerCamera vtable",
    "PeekMessageA import",
    "WaitForSingleObjectEx import",
    "XInputGetState import",
    "D3D11CreateDeviceAndSwapChain import",
    "creation thunk",
    "wrapper call",
    "Effects call",
    "HUDless viewport call",
    "LoadingMenu query",
    "MainMenu query",
    "PauseMenu query",
    "PipboyMenu query",
    "MapMenu query",
    "VATSMenu query",
    "LoadingMenu name",
    "MainMenu name",
    "PauseMenu name",
    "PipboyMenu name",
    "MapMenu name",
    "VATSMenu name",
];

fn name(site: Site) -> &'static str {
    match SIGNATURES.get(site as usize) {
        Some(signature) => signature.name,
        None => DERIVED[site as usize - PATTERN_SITES],
    }
}

/// The 1.11.240.0 RVAs, indexed by [`Site`]: documentation and the installed-image test.
#[cfg(test)]
const RECORDED: [usize; SITES] = [
    0xc3_3250, 0x167_5140, 0x167_54d0, 0xc3_39d0, 0x167_2540, 0x167_3ae0, 0xdd_be50, 0xc3_4eb0,
    0xbd_6850, 0xbd_51c0, 0xc3_46b1, 0xc3_46c0, 0xc3_a992, 0xc3_a970, 0xc3_15c0, 0xc3_42ba,
    0xc3_42de, 0xc3_4ec8, 0xbd_7653, 0xc3_4eec, 0xbd_76df, 0x167_3b74, 0xc3_38ea, 0xc3_391e,
    0xc3_398b, 0xc3_39c1, 0xdd_bffb, 0xdd_c44c, 0x167_465c, 0x167_78a2, 0x167_60df, 0x167_3d1c,
    0x167_7a5f, 0x167_7b02, 0x167_4205, 0x167_42a4, 0x167_7bb7, 0x183_9ed0, 0x21f_38b0, 0x181_8550,
    0x21f_0ac0, 0x183_9ec0, 0x21f_a200, 0x218_5420, 0xc3_35ec, 0x21f_9f78, 0x21f_9f34, 0xc3_82a5,
    0xc3_83c7, 0xc3_9d6f, 0xc3_9e13, 0x182_c02d, 0x182_c597, 0x21f_30b0, 0x182_bf20, 0x182_d750,
    0x182_d580, 0x181_7ef0, 0x21f_3294, 0x21f_3b36, 0x21f_3db6, 0x21f_0e25, 0x182_d9d9, 0x182_da63,
    0x182_db0f, 0x182_d647, 0x183_985b, 0x183_9bc9, 0x183_9c3c, 0x183_9cb1, 0x183_9e00, 0x183_a278,
    0x183_a2d4, 0x183_a4a0, 0x181_7f97, 0x182_3db7, 0x182_3f14, 0x182_430d, 0x218_56df, 0x18_a246,
    0x182_e226, 0xc3_3a60, 0x165_ba9f, 0x1a8_0320, 0x1a8_0480, 0x1a8_04a0, 0x102_b5c0, 0x182_4d45,
    0x32d_d130, 0x3e7_5cb8, 0x327_3ae0, 0x31f_0b80, 0x32a_12f0, 0x32a_1320, 0x38e_13a0, 0x3a2_5d10,
    0x38e_1398, 0x38e_13b0, 0x3a2_5d00, 0x3e7_3018, 0x3d8_71a0, 0x2f4_d750, 0x32d_d338, 0x339_f430,
    0x3e5_e4b0, 0x292_9458, 0x292_93f8, 0x3e6_2250, 0x3e6_2190, 0x2f9_7b30, 0x3e6_22d0, 0x3e6_2210,
    0x30e_8930, 0x30e_6e58, 0x255_dd10, 0x2f3_0fc0, 0x255_e3b8, 0xbd_72e0, 0xbd_7430, 0x257_1a30,
    0x291_5eb0, 0x221_4fc0, 0x25a_a6d8, 0x244_0a98, 0x244_0318, 0x244_0f10, 0x244_1758, 0x22c_4cd9,
    0xbd_6945, 0x21f_a288, 0x21f_a2ca, 0x1a8_7520, 0x1a8_79a0, 0x1a8_7e20, 0x1a8_7fd0, 0x1a8_7a30,
    0x1a8_90b0, 0x3db_d830, 0x3db_d8b0, 0x3db_d930, 0x3db_d960, 0x3db_d8d0, 0x3db_db40,
];

/// A relation the code fixes through an operand: the `disp32` at `from + disp`, relative to
/// `from + next`, addresses `to + addend` (offsets from the site, which for a call site is its
/// return address).
struct Link {
    from: Site,
    disp: isize,
    next: isize,
    to: Site,
    addend: isize,
}

const fn link(from: Site, disp: isize, next: isize, to: Site, addend: isize) -> Link {
    Link {
        from,
        disp,
        next,
        to,
        addend,
    }
}

/// A call site's `call rel32` reaches `to`.
const fn calls(from: Site, to: Site) -> Link {
    link(from, -4, 0, to, 0)
}

const LINKS: &[Link] = &[
    link(Site::WorkerJoin, 3, 7, Site::Worker, 0),
    link(Site::WorkerJoin, 0x28, 0x2c, Site::WaitImport, 0),
    calls(Site::SwapCall, Site::Swap),
    calls(Site::JoinCall, Site::WorkerJoin),
    calls(Site::KeyboardCall, Site::Keyboard),
    calls(Site::MouseCall, Site::Mouse),
    link(Site::IdleCall, -9, -5, Site::Main, 0),
    calls(Site::IdleCall, Site::Idle),
    link(Site::MainPollCall, -9, -5, Site::InputManager, 0),
    calls(Site::MainPollCall, Site::InputPoll),
    calls(Site::ConsumeCall, Site::Consume),
    calls(Site::WrapperPollCall, Site::InputPoll),
    calls(Site::WorkerPollCall, Site::InputPoll),
    calls(Site::WorkerConsumeCall, Site::Consume),
    calls(Site::WrapperCall, Site::Wrapper),
    calls(Site::Peek1, Site::PeekImport),
    calls(Site::Peek2, Site::PeekImport),
    calls(Site::Peek3, Site::PeekImport),
    calls(Site::Peek4, Site::PeekImport),
    link(Site::GamepadState, 2, 6, Site::GamepadImport, 0),
    // The DR writer reads the disabled flag at targets + FE8.
    link(Site::DrWriter, 6, 10, Site::Targets, 0xfe8),
    link(Site::Prepass, 0x24, 0x28, Site::TlsIndex, 0),
    link(Site::Prepass, 0x8c, 0x90, Site::RenderContext, 0),
    link(Site::OuterPresent, 0x25, 0x29, Site::RendererWindow, 0),
    link(Site::OuterPresent, 0x69, 0x6d, Site::RendererWindow, 0),
    link(Site::WorldBegin, 0x27, 0x2b, Site::ViewportStore, 0),
    link(Site::PresentCall, -9, -5, Site::PresentRenderer, 0),
    calls(Site::PresentCall, Site::OuterPresent),
    link(Site::PrepassCall, -14, -10, Site::CameraUpdate, 0),
    calls(Site::PrepassCall, Site::Prepass),
    calls(Site::DrCall, Site::DrWriter),
    calls(Site::WorldCall, Site::WorldBegin),
    calls(Site::ImagespaceCall1, Site::Imagespace),
    calls(Site::ImagespaceCall2, Site::Imagespace),
    calls(Site::ImagespaceCall3, Site::Imagespace),
    calls(Site::EffectsCall, Site::Effects),
    calls(Site::HudlessCall, Site::ViewportStore),
    link(Site::BuilderCall, 1, 5, Site::Builder, 0),
    link(Site::SetterCall, 1, 5, Site::ViewportBuilder, 0),
    link(Site::CameraReads, 3, 7, Site::NearCamera, 0),
    link(Site::CameraReads, 10, 14, Site::State, 0),
    link(Site::CameraReads, 18, 22, Site::CameraRead, 0),
    link(Site::CameraReads, 25, 29, Site::WorldCamera, 0),
    link(Site::CameraReads, 32, 36, Site::State, 0),
    link(Site::CameraReads, 40, 44, Site::CameraRead, 0),
    link(Site::CameraReads, 47, 51, Site::WorldCamera, 0),
    link(Site::CameraReads, 54, 58, Site::State, 0),
    link(Site::CameraReads, 62, 66, Site::CameraRead, 0),
    link(Site::WorldRanges, 0xd, 0x11, Site::WorldInterval, 0),
    link(Site::WorldRanges, 0x1b, 0x1f, Site::State, 0),
    // The near interval's second word.
    link(Site::WorldRanges, 0x23, 0x27, Site::NearInterval, 4),
    link(Site::WorldRanges, 0x28, 0x2c, Site::TemporalUpdate, 0),
    link(Site::ViewportBuilder, 0x12, 0x16, Site::ViewportUnit, 0),
    link(Site::ShadowWriter, 2, 6, Site::TlsIndex, 0),
    link(Site::ShadowWriter, 0x2c, 0x30, Site::RenderContext, 0),
    link(Site::DrEntry, 6, 10, Site::TlsIndex, 0),
    link(Site::TargetSelect, 0x10, 0x14, Site::RendererPointer, 0),
    // `cmp byte [rip+targets+FE4],0`: the imm8 follows the displacement.
    link(Site::EffectsSize, 0x18, 0x1d, Site::Targets, 0xfe4),
    link(Site::RendererInit, 3, 7, Site::RendererObject, 0),
    link(Site::RendererInit, 0x49, 0x4d, Site::RendererPointer, 0),
    // The native frame counter at graphics state + 98.
    link(Site::FrameRead, 2, 6, Site::State, 0x98),
    link(Site::DeltaRead, 3, 7, Site::Delta, -0x10),
    calls(Site::CreateCall, Site::CreateThunk),
    link(Site::CreateCall, 3, 7, Site::RendererPointer, 0),
    link(Site::CreateCall, 9, 13, Site::TlsIndex, 0),
    // The window cell is the renderer's window field.
    link(Site::CreateCall, 0x19, 0x1d, Site::RendererWindow, 0),
    link(Site::CreateThunk, 2, 6, Site::CreateImport, 0),
    // Each menu query returns its name object.
    link(Site::LoadingMenu, 0x2c, 0x30, Site::LoadingMenuName, 0),
    link(Site::MainMenu, 0x2c, 0x30, Site::MainMenuName, 0),
    link(Site::PauseMenu, 0x2c, 0x30, Site::PauseMenuName, 0),
    link(Site::PipboyMenu, 0x2c, 0x30, Site::PipboyMenuName, 0),
    link(Site::MapMenu, 0x2c, 0x30, Site::MapMenuName, 0),
    link(Site::VatsMenu, 0x2c, 0x30, Site::VatsMenuName, 0),
];

/// Pointer slots that hold a derived site: `holder + offset` holds the site's address. Checked
/// again in live memory, since another module may replace a vtable or job entry.
const SLOTS: [(Site, Site, isize); 5] = [
    (Site::JobName, Site::JobDescriptor, 8),
    (Site::JobEnd, Site::JobDescriptor, 0),
    (Site::WorkerDestructor, Site::WorkerVtable, 0),
    (Site::WorkerEntry, Site::WorkerVtable, 8),
    // ImageSpaceEffectTemporalAA's slot 8.
    (Site::TaaQuery, Site::TaaVtable, 64),
];

/// Expected bytes at derived code sites, as (site, pattern, offset of the site in the pattern).
const CHECKS: [(Site, &str, usize); 14] = [
    // "JobListEnd".
    (Site::JobName, "4A 6F 62 4C 69 73 74 45 6E 64 00", 0),
    (
        Site::WorkerDestructor,
        "48 89 5C 24 08 57 48 83 EC 20 48 8D 05 ?? ?? ?? ?? 48 8B F9 48 89 01 8B DA 48 8B 49 60",
        0,
    ),
    (
        Site::WorkerEntry,
        "41 56 48 83 EC 40 4C 8B F1 45 33 C9 33 C9 45 33 C0",
        0,
    ),
    (
        Site::TaaQuery,
        "83 3D ?? ?? ?? ?? 01 75 12 80 79 08 00 74 0C 83 3D ?? ?? ?? ?? 00 75 03 B0 01 C3",
        0,
    ),
    (Site::CreateThunk, CREATE_THUNK, 0),
    (Site::WrapperCall, WRAPPER_CALL, CALL),
    (Site::EffectsCall, EFFECTS_CALL, 18 + CALL),
    (Site::HudlessCall, HUDLESS_CALL, CALL),
    (Site::LoadingMenu, MENU_QUERY, 0),
    (Site::MainMenu, MENU_QUERY, 0),
    (Site::PauseMenu, MENU_QUERY, 0),
    (Site::PipboyMenu, MENU_QUERY, 0),
    (Site::MapMenu, MENU_QUERY, 0),
    (Site::VatsMenu, MENU_QUERY, 0),
];

/// `jmp [rip+slot]`: the import thunk.
const CREATE_THUNK: &str = "FF 25 ?? ?? ?? ??";
/// The wrapper call inside the JobListEnd job, then its next argument load.
const WRAPPER_CALL: &str = "E8 ?? ?? ?? ?? 48 8B 0D";
/// `Effects(manager, 0, 13, 1, 1)` inside imagespace: the call follows 18 argument bytes.
const EFFECTS_CALL: &str =
    "33 D2 C7 44 24 20 01 00 00 00 44 8D 4A 01 44 8D 42 0D E8 ?? ?? ?? ?? FF 15";
/// The HUDless viewport selection inside imagespace.
const HUDLESS_CALL: &str = "E8 ?? ?? ?? ?? 8B 0D";
/// Calls are searched for this far into their discovered caller.
const CALLER_WINDOW: usize = 0x200;
/// The lazily initialised menu-name query shared by every menu: `lea rax,[rip+name]` returns the
/// name at +29 and `lea rdx,[rip+literal]` constructs it from its literal at +55.
const MENU_QUERY: &str = "40 53 48 83 EC 20 65 48 8B 04 25 58 00 00 00 8B 0D ?? ?? ?? ?? BA 70 15 00 00 48 8B 0C C8 8B 04 0A 39 05 ?? ?? ?? ?? 7F 0D 48 8D 05 ?? ?? ?? ?? 48 83 C4 20 5B C3 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 3D ?? ?? ?? ?? FF 75 DE 48 8D 1D ?? ?? ?? ?? 48 8B CB 48 8D 15 ?? ?? ?? ??";
const MENU_LITERAL: (usize, usize) = (0x58, 0x5c);

/// Each menu query, its returned name object and the literal it is built from.
pub(crate) const MENUS: [(Site, Site, &CStr); 6] = [
    (Site::LoadingMenu, Site::LoadingMenuName, c"LoadingMenu"),
    (Site::MainMenu, Site::MainMenuName, c"MainMenu"),
    (Site::PauseMenu, Site::PauseMenuName, c"PauseMenu"),
    (Site::PipboyMenu, Site::PipboyMenuName, c"PipboyMenu"),
    (Site::MapMenu, Site::MapMenuName, c"MapMenu"),
    (Site::VatsMenu, Site::VatsMenuName, c"VATSMenu"),
];

/// RTTI class names of the vtables.
const VTABLES: [(Site, &[u8]); 4] = [
    (Site::WorkerVtable, b".?AVServingThread@JobListManager@@\0"),
    (Site::ControlsVtable, b".?AVPlayerControls@@\0"),
    (Site::TaaVtable, b".?AVImageSpaceEffectTemporalAA@@\0"),
    (Site::CameraVtable, b".?AVPlayerCamera@@\0"),
];

const IMPORTS: [(Site, &str, &str); 4] = [
    (Site::PeekImport, "USER32.dll", "PeekMessageA"),
    (Site::WaitImport, "KERNEL32.dll", "WaitForSingleObjectEx"),
    // XInputGetState is imported by ordinal.
    (Site::GamepadImport, "XINPUT1_3.dll", "#2"),
    (
        Site::CreateImport,
        "d3d11.dll",
        "D3D11CreateDeviceAndSwapChain",
    ),
];

fn refused(site: Site) -> impl Fn(Miss) -> String {
    move |miss| {
        Refusal {
            name: name(site),
            miss,
        }
        .to_string()
    }
}

/// The RIP-relative target of the `disp32` at `at + disp` of the instruction ending at
/// `at + next`, read through `read`.
fn decode(
    at: usize,
    disp: isize,
    next: isize,
    read: impl FnOnce(usize, &mut [u8]) -> bool,
) -> Option<usize> {
    let mut operand = [0; 4];
    read(at.checked_add_signed(disp)?, &mut operand).then_some(())?;
    at.checked_add_signed(next)?
        .checked_add_signed(i32::from_le_bytes(operand) as isize)
}

/// The site `slot` holds, given the image base the image's pointers are relocated to.
fn pointer(
    slot: usize,
    image_base: u64,
    read: &mut impl FnMut(usize, &mut [u8]) -> bool,
) -> Option<usize> {
    let mut value = [0; 8];
    read(slot, &mut value).then_some(())?;
    usize::try_from(u64::from_le_bytes(value).checked_sub(image_base)?).ok()
}

/// The primary vtable of the class whose RTTI type name is `class`: its type descriptor, the
/// complete object locator of offset 0 that names it, and the slot before the vtable that points
/// at the locator.
fn vtable(data: &[Section], image_base: u64, class: &[u8]) -> Result<usize, Miss> {
    // A type descriptor is (vtable, spare, name).
    let descriptor = find_unique(data, class)?
        .checked_sub(16)
        .ok_or(Miss::Operand)?;
    let mut locator = [0; 16];
    locator[0] = 1;
    locator[12..].copy_from_slice(
        &u32::try_from(descriptor)
            .map_err(|_| Miss::Operand)?
            .to_le_bytes(),
    );
    let locator = find_unique(data, &locator)?;
    Ok(find_unique(data, &(image_base + locator as u64).to_le_bytes())? + 8)
}

/// Every match of `pattern` in the code sections.
fn every(code: &[Section], pattern: &Pattern) -> Vec<usize> {
    code.iter()
        .flat_map(|section| {
            section
                .bytes
                .windows(pattern.len())
                .enumerate()
                .filter(|(_, window)| pattern.matches(window))
                .map(move |(offset, _)| section.rva + offset)
        })
        .collect()
}

/// The only one of `candidates` that `keep` accepts.
fn only(
    candidates: impl IntoIterator<Item = usize>,
    keep: impl FnMut(&usize) -> bool,
) -> Result<usize, Miss> {
    let mut kept = candidates.into_iter().filter(keep);
    match (kept.next(), kept.next()) {
        (Some(found), None) => Ok(found),
        (None, _) => Err(Miss::Missing),
        _ => Err(Miss::Ambiguous),
    }
}

fn parse(text: &str) -> Pattern {
    Pattern::parse(text).expect("valid derived pattern")
}

/// Resolves the derived sites from the code and data sections and the pattern sites.
fn derive(
    code: &[Section],
    data: &[Section],
    image_base: u64,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
    sites: &mut [usize; SITES],
) -> Result<(), String> {
    let set = |sites: &mut [usize; SITES], site: Site, value: Result<usize, Miss>| {
        sites[site as usize] = value.map_err(refused(site))?;
        Ok::<_, String>(())
    };
    for (site, library, function) in IMPORTS {
        let slot = import_slot(&mut read, library, function).ok_or(Miss::Missing);
        set(sites, site, slot)?;
    }
    for (site, class) in VTABLES {
        set(sites, site, vtable(data, image_base, class))?;
    }
    // The worker's job descriptor (job, name) is the only data slot that holds the job.
    let job = image_base + sites[Site::JobEnd as usize] as u64;
    set(
        sites,
        Site::JobDescriptor,
        find_unique(data, &job.to_le_bytes()),
    )?;
    for &(site, holder, offset) in &SLOTS {
        let slot = sites[holder as usize].wrapping_add_signed(offset);
        let held = pointer(slot, image_base, &mut read).ok_or(Miss::Operand);
        if (site as usize) < PATTERN_SITES {
            // A slot of a pattern site must hold that site.
            if held != Ok(sites[site as usize]) {
                return Err(format!(
                    "site '{}' does not hold '{}'",
                    name(holder),
                    name(site)
                ));
            }
        } else {
            set(sites, site, held)?;
        }
    }
    let thunk = find_rip_reference(code, &[0xff, 0x25], sites[Site::CreateImport as usize]);
    set(sites, Site::CreateThunk, thunk)?;
    // Calls found by their callee inside their discovered caller.
    for (site, caller, callee) in [
        (Site::WrapperCall, Site::JobEnd, Site::Wrapper),
        (Site::EffectsCall, Site::Imagespace, Site::Effects),
        (Site::HudlessCall, Site::Imagespace, Site::ViewportStore),
    ] {
        let &(_, text, end) = CHECKS
            .iter()
            .find(|check| check.0 == site)
            .expect("call check");
        let pattern = parse(text);
        let start = sites[caller as usize];
        let target = sites[callee as usize];
        let mut window = vec![0; CALLER_WINDOW + pattern.len()];
        let found = if read(start, &mut window) {
            // `end` is the call's return offset in the pattern.
            only(0..CALLER_WINDOW, |&offset| {
                let call = offset + end;
                let displacement =
                    i32::from_le_bytes(window[call - 4..call].try_into().expect("4 bytes"));
                pattern.matches(&window[offset..])
                    && (start + call).checked_add_signed(displacement as isize) == Some(target)
            })
            .map(|offset| start + offset + end)
        } else {
            Err(Miss::Missing)
        };
        set(sites, site, found)?;
    }
    let queries = every(code, &parse(MENU_QUERY));
    let (disp, next) = MENU_LITERAL;
    for (query, object, literal) in MENUS {
        let expected = literal.to_bytes_with_nul();
        let entry = only(queries.iter().copied(), |&entry| {
            decode(entry, disp as isize, next as isize, &mut read).is_some_and(|literal| {
                let mut name = vec![0; expected.len()];
                read(literal, &mut name) && name == expected
            })
        });
        set(sites, query, entry)?;
        let name = decode(sites[query as usize], 0x2c, 0x30, &mut read).ok_or(Miss::Operand);
        set(sites, object, name)?;
    }
    Ok(())
}

/// Checks every derived site's expected bytes and every link in the image read through `read`.
fn check(
    sites: &[usize; SITES],
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<(), String> {
    for (site, text, at) in CHECKS {
        let pattern = parse(text);
        let mut bytes = vec![0; pattern.len()];
        let start = sites[site as usize].wrapping_sub(at);
        if !read(start, &mut bytes) || !pattern.matches(&bytes) {
            return Err(format!("site '{}' has unexpected bytes", name(site)));
        }
    }
    for link in LINKS {
        let target = decode(sites[link.from as usize], link.disp, link.next, &mut read);
        if target != sites[link.to as usize].checked_add_signed(link.addend) {
            return Err(format!(
                "site '{}' does not address '{}'",
                name(link.from),
                name(link.to)
            ));
        }
    }
    Ok(())
}

fn resolve_image(
    code: &[Section],
    data: &[Section],
    image_base: u64,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Result<[usize; SITES], String> {
    let found = signature::resolve(code, &SIGNATURES).map_err(|refusal| refusal.to_string())?;
    let mut sites = [0; SITES];
    sites[..PATTERN_SITES].copy_from_slice(&found);
    derive(code, data, image_base, &mut read, &mut sites)?;
    check(&sites, read)?;
    Ok(sites)
}

struct Discovery {
    sites: Result<[usize; SITES], CString>,
    sha256: CString,
}

static DISCOVERY: OnceLock<Discovery> = OnceLock::new();

fn read_image(base: usize) -> impl FnMut(usize, &mut [u8]) -> bool {
    move |rva, out| unsafe {
        rsf_game_sys::win32::read_process(
            base.wrapping_add(rva),
            out.as_mut_ptr().cast(),
            out.len(),
        )
    }
}

fn discover() -> Discovery {
    let base = crate::image();
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
            read_image(base),
        ),
        _ => Err("executable sections unreadable".into()),
    };
    Discovery {
        sites: sites
            .map_err(|reason| crate::message(format!("Fallout4 native sites refused: {reason}."))),
        sha256: executable_digest().unwrap_or_default(),
    }
}

/// The running executable's SHA-256 in lowercase hex: evidence for the log and the host setup,
/// never a gate.
fn executable_digest() -> Option<CString> {
    unsafe extern "C" {
        fn rsf_common_sha256_file_hex(path: *const u16, output: *mut [u8; 64]) -> u32;
    }
    let path = unsafe { rsf_game_sys::win32::module_path(std::ptr::null_mut()) }?;
    let mut digest = [0; 64];
    (unsafe { rsf_common_sha256_file_hex(path.terminated().as_ptr(), &raw mut digest) } != 0)
        .then(|| CString::new(digest.to_vec()).ok())
        .flatten()
}

/// Discovers every site once per process; the error names the refused site.
pub(crate) fn admit() -> Result<(), &'static CStr> {
    match &DISCOVERY.get_or_init(discover).sites {
        Ok(_) => Ok(()),
        Err(reason) => Err(reason.as_c_str()),
    }
}

/// Lowercase hex SHA-256 of the running executable; empty before discovery or when unreadable.
pub(crate) fn executable_sha256() -> &'static CStr {
    DISCOVERY
        .get()
        .map_or(c"", |discovery| discovery.sha256.as_c_str())
}

/// The discovered RVA. Zero before discovery resolved every site: nothing hooks, patches or reads
/// a site before preparation admitted them, and [`verify`] refuses an unresolved one.
pub(crate) fn rva(site: Site) -> usize {
    match DISCOVERY.get().map(|discovery| &discovery.sites) {
        Some(Ok(sites)) => sites[site as usize],
        _ => 0,
    }
}

/// The site's address in the running image; zero while unresolved.
pub(crate) fn address(site: Site) -> usize {
    match rva(site) {
        0 => 0,
        rva => crate::image().wrapping_add(rva),
    }
}

/// Whether a resolved site still holds what discovery found in live memory: its pattern (or a
/// global constant's value), every operand link from it, and the slot that holds it. A site
/// another hook or a wrong match changed since discovery refuses before it is hooked, patched,
/// called or trusted.
pub(crate) fn verify(site: Site) -> bool {
    let Some(Ok(sites)) = DISCOVERY.get().map(|discovery| &discovery.sites) else {
        return false;
    };
    let resolved = sites[site as usize];
    if resolved == 0 {
        return false;
    }
    let base = crate::image();
    let mut read = read_image(base);
    let guard = match SIGNATURES.get(site as usize) {
        Some(signature) => signature.guard(resolved),
        None => CHECKS
            .iter()
            .find(|check| check.0 == site)
            .map(|&(_, text, at)| (text, resolved.wrapping_sub(at))),
    };
    if let Some((text, start)) = guard {
        let pattern = parse(text);
        let mut bytes = vec![0; pattern.len()];
        if !read(start, &mut bytes) || !pattern.matches(&bytes) {
            return false;
        }
    }
    let linked = LINKS.iter().filter(|link| link.from == site).all(|link| {
        decode(resolved, link.disp, link.next, &mut read)
            == sites[link.to as usize].checked_add_signed(link.addend)
    });
    let held = SLOTS
        .iter()
        .filter(|slot| slot.0 == site)
        .all(|&(_, holder, offset)| {
            let slot = base
                .wrapping_add(sites[holder as usize])
                .wrapping_add_signed(offset);
            crate::read::<usize>(slot) == Some(base.wrapping_add(resolved))
        });
    linked && held
}

/// Every site of `sites` verifies.
pub(crate) fn verify_all(sites: &[Site]) -> bool {
    sites.iter().all(|&site| verify(site))
}

#[cfg(test)]
mod tests;
