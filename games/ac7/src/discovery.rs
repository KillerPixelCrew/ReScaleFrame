// SPDX-License-Identifier: MIT
//! Signature discovery of every Ace7Game.exe site the carrier and the native renderer hook, patch,
//! call or read, and of the TrueSky plugin sites.
//!
//! Each owner (the carrier proxy after the code decrypted, the adapter DLL at prepare) copies the
//! executable sections of the running image once and resolves every site in one pass, each from a
//! pattern unique in those sections (rel32 branches and RIP displacements are wildcards). Globals
//! resolve through the RIP-relative operand of an instruction that uses them. Functions the code
//! never calls directly resolve through their vtable, itself found by its constructor, and are
//! checked at their entry. The six shader-depth gates are one group: they sit in byte-identical
//! template instantiations no pattern can tell apart, so every match of the group's pattern is a
//! member and none refuses the group. A missing or ambiguous site refuses only the features that
//! need it and names the site; every site is matched again in live memory before it is hooked,
//! patched or called. The recorded RVAs are those of Steam build 9855922 (SHA-256 c7da97f5...,
//! PE timestamp 6362748e). Ace7Game.exe's code is encrypted on disk, so the patterns were derived
//! from and are tested against a decrypted image; docs/research/ue418-hook-map.md records how.
// The WARP fixtures stand in for the recorded build, so they leave the scan unused.
#![cfg_attr(feature = "warp-fixture", allow(dead_code))]

use rsf_game_sys::signature::{self, EXECUTE, Find, Miss, Pattern, Refusal, Section, Signature};
use std::sync::{Mutex, OnceLock};

/// The hooked entry sites come first, in the native renderer's hook order.
pub const HOOKS: usize = crate::sys::RSF_AC7_HOOK_SITE_COUNT as usize;

/// Every other site, after the hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Site {
    // Engine functions the detours call or whose layout they rely on; never hooked.
    DownsampleDescriptor = HOOKS,
    EffectiveViewState,
    WaitRecorders,
    TranslucencyProducer,
    TranslucencyAllocate,
    TranslucencyDepth,
    WidgetStaticClass,
    HudParameters,
    TargetUpdate,
    MoveUniform,
    FamilySize,
    RegisterNode,
    PooledOutput,
    SceneTargets,
    InitRhiResources,
    RemoveJitter,
    PostprocessParameters,
    // Globals.
    Engine,
    DynamicRhi,
    UniformLayout,
    FrameEndSync,
    CurrentCommand,
    HangarGameModeClass,
    HangarMeshManagerClass,
    HangarPawnClass,
    HangarPlaneSelectorClass,
    PlayerPlaneClass,
    AssetViewerManagerClass,
    CameraActorClass,
    CampaignBriefingWidgetClass,
    // Anchors of the sites found through a vtable or a call.
    AaPassVtable,
    CompositePassVtable,
    HudPassConstructor,
    HudPassVtable,
    RhiVtable,
    FrameTaskDispatch,
    FrameTaskVtable,
    // Patch sites: the separate-translucency depth gates, the carrier's gates and the native
    // renderer's temporal velocity and scene precision.
    GateDepthFill,
    GateDepthBind,
    GateSnapshotResolve,
    GateResolve,
    GateLayerView,
    JitterGate,
    TranslucentVelocityGate,
    SeparateTranslucency,
    VelocityRender,
    VelocitySize,
    VelocityMoved,
    ScenePrecision,
    // Motion capture observers.
    RhiWrite,
    ShouldRenderVelocity,
    HasVelocity,
    VelocityHistory,
    PrepareSlateWindow,
    AddTemporalAa,
    TemporalAaProcess,
    RecursivelyProcess,
}

const SITES: usize = Site::RecursivelyProcess as usize + 1;

impl From<Site> for usize {
    fn from(site: Site) -> usize {
        site as usize
    }
}

/// Hook indices of the sites that other features share.
pub const POSTPROCESS: usize = 12;
pub const WIDGET_QUEUE: usize = 16;
const DESCRIPTOR_AA: usize = 7;
const DESCRIPTOR_HUD: usize = 9;
const DESCRIPTOR_COMPOSITE: usize = 10;
const RHI_PIXEL_UNIFORM: usize = 38;
const FRAME_TASK_CONSTRUCT: usize = 49;
const FRAME_TASK_EXECUTE: usize = 50;
const CREATE_PIXEL_SHADER: usize = 56;

const fn at(name: &'static str, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(0),
        check: "",
    }
}

/// A site found `offset` bytes into its pattern.
const fn within(name: &'static str, offset: usize, pattern: &'static str) -> Signature {
    Signature {
        name,
        pattern,
        find: Find::At(offset),
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

/// A site [`DERIVED`] resolves from another, with its own entry checked when it is code.
const fn derived(name: &'static str, check: &'static str) -> Signature {
    Signature {
        name,
        pattern: "",
        find: Find::At(0),
        check,
    }
}

/// The GetPrivateStaticClass tail that registers a class (its size and flags fixed) and then
/// returns its cache: the cache is the operand of that `mov rax,[rip+cache]`.
const fn class(name: &'static str, pattern: &'static str) -> Signature {
    global(name, 32, 36, pattern)
}

const STATIC_CLASS: &str = "4C 8B DC 48 83 EC 78 48 8B 05 ?? ?? ?? ?? 48 85 C0";
/// Every reviewed ComputeOutputDesc starts this way; slot 14 of its pass vtable.
const DESCRIPTOR: &str = "40 53 48 83 EC 20 48 8B 01 48 8B DA 33 D2 FF 50 08";
const DESCRIPTOR_SLOT: usize = 14 * 8;

/// Indexed by hook number, then by [`Site`]. Derived from build 9855922 by
/// tools/derive-site-patterns.py; the depth gates' patched opcode is a wildcard, because the
/// adapter discovers after the carrier patched them.
#[rustfmt::skip]
const SIGNATURES: [Signature; SITES] = [
    at("process tonemap", "40 55 53 56 57 41 55 41 57 48 8D 6C 24 B8 48 81 EC 48 01 00 00"),
    at("process AA", "40 55 56 41 56 48 8D 6C 24 B9 48 81 EC 00 01 00 00"),
    at("process material", "40 55 53 56 57 41 54 41 55 41 56 48 8D AC 24 E0 FE FF FF"),
    at("process HUD", "40 55 53 56 41 55 41 56 41 57 48 8D AC 24 E8 FE FF FF"),
    at("process composite", "48 89 5C 24 18 55 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 E0 FE FF FF 48 81 EC 20 02 00 00 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 F0 00 00 00"),
    at("process output", "40 55 56 57 48 8D 6C 24 F0 48 81 EC 10 01 00 00 48 8B 01"),
    at("descriptor tonemap", "48 89 5C 24 08 57 48 83 EC 40 48 8B 01 48 8B DA 33 D2 48 8B F9 FF 50 08 48 8B D0 48 8B 08 48 85 C9 75 04 33 C0 EB 09 48 8B 01 8B 52 08 FF 50 38 0F 10 00 BA 01 00 00 00"),
    derived("descriptor AA", DESCRIPTOR),
    at("descriptor material", "48 89 5C 24 08 57 48 83 EC 40 48 8B 01 48 8B FA 33 D2 48 8B D9"),
    derived("descriptor HUD", DESCRIPTOR),
    derived("descriptor composite", DESCRIPTOR),
    at("descriptor output", "48 89 5C 24 08 57 48 83 EC 20 48 8B 01 48 8B DA 33 D2 48 8B F9 FF 50 08 48 8B D0 48 8B 08 48 85 C9 75 04 33 C0 EB 09 48 8B 01 8B 52 08 FF 50 38 0F 10 00 0F 11 03"),
    at("post-process", "4C 8B DC 55 49 8D AB E8 FC FF FF 48 81 EC 10 04 00 00"),
    at("graph context", "40 56 57 41 57 48 83 EC 30 65 48 8B 04 25 58 00 00 00"),
    at("visibility", "40 55 53 41 54 41 57 48 8D AC 24 68 FF FF FF 48 81 EC 98 01 00 00"),
    at("view family construct", "48 89 5C 24 20 4C 89 44 24 18 48 89 4C 24 08 55 56 57 41 54 41 55 41 56 41 57 48 83 EC 50"),
    at("widget queue", "40 55 57 48 8D 6C 24 A8 48 81 EC 58 01 00 00 0F 29 B4 24 40 01 00 00"),
    at("widget targets", "48 89 5C 24 08 57 48 83 EC 50 80 79 78 00 48 8B D9"),
    at("widget draw", "48 8B C4 48 89 58 08 48 89 70 10 48 89 78 18 4C 89 70 20 55 48 8D 68 D8 48 81 EC 20 01 00 00 F3 0F 10 65 50"),
    at("engine tick", "48 8B C4 48 89 58 10 48 89 70 18 55 57 41 54 41 56 41 57 48 8D 68 88"),
    at("renderer retire", "48 89 5C 24 08 57 48 83 EC 20 48 8B FA 48 8B D9 33 D2 E8 ?? ?? ?? ??"),
    at("input poll", "83 B9 B0 01 00 00 00 48 8B D1 75 22 F2 0F 10 82 48 05 00 00"),
    at("game instance init", "48 89 5C 24 10 48 89 74 24 18 48 89 7C 24 20 55 41 56 41 57 48 8B EC 48 83 EC 70 48 8B F9"),
    at("render target init", "0F B6 44 24 28 83 E0 01 89 91 D0 00 00 00 44 89 81 D4 00 00 00"),
    at("simulation A", "48 89 5C 24 08 57 48 83 EC 30 0F 29 74 24 20 41 0F B6 D8"),
    at("simulation B", "48 8B C4 44 88 40 18 48 89 48 08 55 53 56 57 41 55"),
    at("scene redraw", "48 89 5C 24 08 57 48 83 EC 20 48 8B D9 0F B6 FA 48 8B 89 20 07 00 00"),
    at("viewport draw", "40 55 56 57 41 54 41 55 48 8D AC 24 70 FE FF FF"),
    at("Slate private draw", "48 8B C4 48 89 50 10 55 53 48 8D A8 88 FE FF FF"),
    called("Slate allocate", "E8 ?? ?? ?? ?? 48 8B 08 48 83 C1 10 74 17 0F 10 45 88", "40 56 57 48 83 EC 48 48 89 5C 24 40 48 8B F1 48 89 6C 24 38"),
    at("Slate task", "48 89 5C 24 10 48 89 6C 24 18 48 89 74 24 20 57 48 83 EC 40 48 8B F9 E8 ?? ?? ?? ?? 0F B6 57 31"),
    at("Slate window", "4C 8B DC 55 53 57 49 8D AB 08 FA FF FF 48 81 EC E0 06 00 00"),
    at("Slate texture", "48 89 5C 24 18 55 56 57 41 55 41 56 48 83 EC 20 48 8B 71 48 4D 8B F1 4D 8B 29 48 8B DA 4C 89 7C 24 58 48 8B F9 4D 8B F8 80 7E 28 00 75 08 48 8B CE E8 ?? ?? ?? ?? 66 83 BF C2 00 00 00 01"),
    at("translucency size", "48 89 5C 24 08 57 48 83 EC 20 65 48 8B 04 25 58 00 00 00 48 8B D9 44 8B 05 ?? ?? ?? ?? 0F B6 FA B9 18 00 00 00 4E 8B 04 C0"),
    at("unmodified begin", "48 89 5C 24 18 55 56 57 48 83 EC 30 83 B9 58 02 00 00 02"),
    at("unmodified resolve", "40 57 48 83 EC 40 83 B9 58 02 00 00 02 48 8B FA"),
    at("translucency render", "40 55 56 57 41 56 48 8D 6C 24 C1 48 81 EC B8 00 00 00"),
    called("pixel View uniform", "E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 33 DB 48 8D 8E C0 00 00 00", "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57"),
    derived("RHI pixel uniform", "48 89 5C 24 10 55 56 57 41 56 41 57 48 83 EC 20"),
    at("RHI pixel tables", "48 89 5C 24 20 55 41 56 41 57 48 83 EC 30 0F B7 81 E6 40 00 00"),
    called("pixel enqueue A", "E8 ?? ?? ?? ?? 48 8B 9C 24 90 00 00 00 48 8B B4 24 98 00 00 00", "48 89 5C 24 18 48 89 6C 24 20 56 48 83 EC 20 41 80 78 06 00"),
    called("pixel enqueue B", "E8 ?? ?? ?? ?? 4C 8B 8C 24 80 00 00 00 48 8B D6", "48 89 5C 24 20 56 48 83 EC 20 41 80 78 06 00 48 8B F2"),
    called("pixel enqueue C", "E8 ?? ?? ?? ?? 48 8D 8C 24 80 00 00 00 E8 ?? ?? ?? ?? 48 8B CE", "48 89 5C 24 20 56 48 83 EC 20 41 80 78 06 00 48 8B F2"),
    called("pixel enqueue D", "E8 ?? ?? ?? ?? 48 81 C4 C0 00 00 00 5D C3 CC CC CC CC CC CC CC CC 48 89 5C 24 18", "40 53 56 57 48 83 EC 30 41 80 78 06 00 4D 8B D1"),
    at("sky projection", "48 83 EC 08 80 3D ?? ?? ?? ?? 00 0F 28 D9 F3 0F 10 41 20"),
    at("sky render", "40 55 41 54 41 55 41 57 48 8D AC 24 38 FC FF FF"),
    at("temporal sample index", "0F B6 81 1C 0C 00 00 C3 CC CC CC CC CC CC CC CC"),
    at("scene colour format", "83 B9 58 02 00 00 02 41 BA 0A 00 00 00 45 8B C2"),
    at("render family", "4C 8B DC 49 89 5B 08 49 89 6B 18 56 57 41 56 48 83 EC 60"),
    derived("BeginFrame task construct", "40 53 48 83 EC 20 C7 41 08 FF 00 00 00 48 8D 05 ?? ?? ?? ?? 48 89 01 48 8B D9 41 8D 40 01 89 41 0C 48 8D 05 ?? ?? ?? ??"),
    derived("BeginFrame task execute", "48 89 5C 24 10 48 89 6C 24 18 48 89 74 24 20 57 48 83 EC 20 48 8B F9 41 8B D0 48 83 C1 10 4C 8D 47 18 E8 ?? ?? ?? ??"),
    called("RHI frame begin", "E8 ?? ?? ?? ?? F3 0F 10 4F 18 48 8B 4F 10", "48 89 5C 24 08 57 48 83 EC 20 48 8B 51 30 48 8B D9"),
    called("RHI frame end", "E8 ?? ?? ?? ?? E9 ?? ?? ?? ?? 33 D2 48 8D 8D 48 01 00 00", "48 89 5C 24 08 57 48 83 EC 20 48 8B 51 30 48 8B D9"),
    at("frame end sync", "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 63 41 10"),
    at("message pump", "48 83 EC 68 45 33 C9 45 33 C0 33 D2 84 C9 48 8D 4C 24 30"),
    at("engine pacing", "40 55 53 57 48 8D 6C 24 B9 48 81 EC C0 00 00 00 65 48 8B 04 25 58 00 00 00"),
    derived("create pixel shader", "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57"),
    at("base pass", "48 89 5C 24 10 55 56 57 41 54 41 57 48 83 EC 40"),
    at("translucency mesh", "4C 89 4C 24 20 4C 89 44 24 18 48 89 54 24 10 48 89 4C 24 08 55 53 56 57 41 55 41 56 48 8D 6C 24 F8"),
    at("mesh particle parameters", "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 20 57 41 54 41 55 41 56 41 57 48 83 EC 40 49 8B 70 48"),
    at("beam without noise", "4C 8B DC 55 53 56 57 41 56 41 57 49 8D AB D8 FE FF FF"),
    at("beam noise", "4C 8B DC 55 49 8D AB D8 FC FF FF 48 81 EC 20 04 00 00 48 8B 82 80 00 00 00"),
    at("beam interpolated noise", "40 55 48 8D AC 24 C0 FB FF FF 48 81 EC 40 05 00 00"),
    at("ribbon fill", "40 55 53 57 41 54 41 57 48 8D AC 24 50 FE FF FF"),
    at("anim trail fill", "48 89 4C 24 08 55 53 57 41 57 48 8D AC 24 38 FF FF FF"),
    // Helpers.
    at("downsample descriptor", "48 89 5C 24 08 57 48 83 EC 40 48 8B 01 48 8B DA 33 D2 48 8B F9 FF 50 08 48 8B C8"),
    at("effective view state", "83 B9 B0 09 00 00 02 4C 8B 89 10 14 00 00 75 58"),
    at("wait for recorders", "40 53 48 83 EC 20 8B 1D ?? ?? ?? ?? 85 DB 0F 84 ?? ?? ?? ??"),
    at("translucency producer", "4C 8B DC 55 56 57 41 56 48 81 EC 78 0D 00 00 48 8B 05 ?? ?? ?? ??"),
    at("translucency allocate", "4C 89 44 24 18 53 56 57 48 81 EC A0 00 00 00 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 84 24 90 00 00 00 48 8B F9 48 8B F2 48 8B 89 B8 01 00 00"),
    at("translucency depth", "48 89 5C 24 08 55 56 57 41 54 41 55 41 56 41 57 48 8D 6C 24 A0 48 81 EC 60 01 00 00 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 50"),
    called("widget static class", "E8 ?? ?? ?? ?? 48 8B 53 10 4C 8D 80 88 00 00 00 49 63 40 08 3B 82 90 00 00 00 7F 10 48 8B C8 48 8B 82 88 00 00 00 4C 39 04 C8 74 02 33 DB 33 C9", STATIC_CLASS),
    at("HUD parameters", "48 8B C4 55 53 57 41 55 48 8D 68 A1 48 81 EC 88 00 00 00"),
    at("render target update", "48 89 5C 24 10 57 48 83 EC 40 48 83 79 70 00 0F B6 FA"),
    at("uniform move", "40 53 48 83 EC 20 48 8B D9 48 3B CA 0F 84 ?? ?? ?? ?? 48 8B 02 48 89 74 24 38 33 F6 48 89 7C 24 40 48 8B 39 48 89 01 48 89 32 48 85 FF 0F 84 ?? ?? ?? ??"),
    at("family size", "48 83 EC 28 8B 41 08 45 32 C0 0F 57 D2 4C 8B D9"),
    at("graph node register", "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 63 71 08 48 8B DA"),
    at("pooled output", "48 89 5C 24 10 48 89 6C 24 18 48 89 74 24 20 57 48 83 EC 40 48 8B 79 50"),
    at("scene targets", "48 8D 05 ?? ?? ?? ?? C3 CC CC CC CC CC CC CC CC 48 63 81 58 02 00 00"),
    at("InitRHIResources", "48 89 5C 24 20 55 56 57 48 8D 6C 24 B9 48 81 EC 90 00 00 00 48 8B F9"),
    at("remove jitter", "40 53 48 83 EC 60 F3 0F 10 41 20 48 8B D9 F3 0F 10 49 24"),
    at("post-process parameters", "48 8B C4 55 56 57 41 56 41 57 48 8D 68 B9 48 81 EC C0 00 00 00"),
    // Globals.
    global("GEngine", 3, 7, "48 8B 0D ?? ?? ?? ?? 48 8B 01 FF 90 50 02 00 00"),
    global("GDynamicRHI", 3, 7, "48 8B 0D ?? ?? ?? ?? 66 0F 5A C9 48 8B 01"),
    global("view uniform layout", 3, 7, "4C 8D 0D ?? ?? ?? ?? 4D 8B 87 18 14 00 00"),
    global("frame end sync object", 3, 7, "48 8D 0D ?? ?? ?? ?? 44 39 20 0F 95 C2 E8 ?? ?? ?? ??"),
    global("current RHI command", 3, 7, "48 89 05 ?? ?? ?? ?? FF 50 08 48 85 DB 75 E5 48 8B CF E8 ?? ?? ?? ?? 48 8B 1E"),
    class("NimbusHangarGameMode class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 0C 00 00 10 C7 44 24 20 38 04 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 83 C4 78 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC 48 83 EC 28"),
    class("NimbusHangarMeshManager class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 01 00 00 10 C7 44 24 20 F0 07 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 83 C4 78 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC B8 EC 5A 3C 59"),
    class("NimbusHangarPawn class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 00 00 00 10 C7 44 24 20 F8 03 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 83 C4 78 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC 48 83 EC 28"),
    class("NimbusHangarPlaneSelector class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 00 00 00 10 C7 44 24 20 88 03 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 83 C4 78 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC E9 ?? ?? ?? ?? CC CC CC CC CC CC CC CC CC CC CC 48 83 EC 28 48 8B 05 ?? ?? ?? ??"),
    class("PlayerPlane class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 00 00 00 10 C7 44 24 20 60 1A 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ??"),
    class("AssetViewerManager class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 01 00 00 10 C7 44 24 20 B0 06 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ??"),
    class("CameraActor class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 00 00 00 10 C7 44 24 20 20 09 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ??"),
    class("CampaignBriefingWidget class", "49 C7 43 B8 00 00 00 00 C7 44 24 28 00 00 00 10 C7 44 24 20 60 05 00 00 E8 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 83 C4 78 C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC E9 ?? ?? ?? ?? CC CC CC CC CC CC CC CC CC CC CC 48 83 EC 28 E8 ?? ?? ?? ?? 41 B8 03 00 00 00"),
    // Anchors.
    global("AA pass vtable", 3, 7, "48 8D 05 ?? ?? ?? ?? 66 89 73 68 4C 89 73 70"),
    global("composite pass vtable", 3, 7, "48 8D 05 ?? ?? ?? ?? 48 89 03 48 8B C3 48 83 C4 20 5B C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC CC C7 41 08 00 00 00 00"),
    called("HUD pass constructor", "E8 ?? ?? ?? ?? 33 DB EB 04 33 DB 8B C3 48 8B 4D D8 48 8B D0 E8 ?? ?? ?? ?? 4C 8D 85 90 00 00 00", "C7 41 08 00 00 00 00 33 D2 48 89 51 10 B8 01 00 00 00 89 51 18 48 89 41 20 48 89 51 28 48 89 51 30 48 89 51 38 48 89 41 40 66 89 41 48 48 8D 05 ?? ?? ?? ?? 48 89 41 60 48 8D 05 ?? ?? ?? ??"),
    derived("HUD pass vtable", ""),
    global("D3D11 RHI vtable", 3, 7, "48 8D 05 ?? ?? ?? ?? 48 89 43 10 8B 47 10"),
    Signature {
        name: "BeginFrame task dispatch",
        pattern: "48 8D 4D 88 41 B8 FF 00 00 00 E8 ?? ?? ?? ??",
        find: Find::Rip { disp: 11, next: 15 },
        check: "40 56 57 48 83 EC 48 48 89 5C 24 40 48 8B F1 48 89 6C 24 38",
    },
    derived("BeginFrame task vtable", ""),
    // Patch sites.
    at("depth fill gate", "?? 0E 4C 8B C3 48 8B D7 49 8B CE E8 ?? ?? ?? ??"),
    at("depth bind gate", "?? 17 4C 8B 87 18 02 00 00 48 8B D3 48 8B CF E8 ?? ?? ?? ??"),
    at("snapshot resolve gate", "44 0F ?? F9 4C 03 FF EB 3B 4C 8B 81 18 02 00 00"),
    at("resolve gate", "?? 17 4C 8B 87 18 02 00 00 48 8B D6 48 8B CF E8 ?? ?? ?? ??"),
    at("layer view gate", "0F 2F 80 20 02 00 00 ?? 0E 80 B8 F5 01 00 00 00"),
    at("jitter gate", "0F 85 ?? ?? ?? ?? 48 85 FF 0F 84 ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ??"),
    at("translucent velocity gate", "0F 87 ?? ?? ?? ?? 48 8B 03 48 8B CB FF 50 20 83 F8 03"),
    at("separate translucency scale", "73 0D 40 84 FF 74 08 F3 0F 10 0D ?? ?? ?? ?? 66 0F 6E 83 08 02 00 00"),
    at("velocity render gate", "75 0E 80 BB 34 0C 00 00 00 75 05 40 B6 01 EB 03"),
    at("velocity size gate", "0F 2F DD 76 38 48 8B D3 49 8B CB E8 ?? ?? ?? ??"),
    at("velocity moved gate", "77 88 48 FF C1 48 83 C0 04 48 83 F9 04 7C DD 48 FF C2"),
    at("scene colour precision", "48 8B 05 ?? ?? ?? ?? 48 8B CB 8B 40 04 89 45 D7"),
    // Motion capture.
    at("current RHI command write", "48 89 05 ?? ?? ?? ?? FF 50 08 48 85 DB 75 E5 48 8B CF E8 ?? ?? ?? ?? 4C 8B BC 24 D0 00 00 00"),
    at("ShouldRenderVelocity", "40 53 48 83 EC 20 4C 8B DA 48 8B D9 45 84 C0 74 46"),
    at("HasVelocity", "40 53 48 83 EC 60 80 B9 34 0C 00 00 00 48 8B DA"),
    at("velocity history", "40 53 48 83 EC 20 49 8B D8 48 85 D2 74 3A 83 7A 10 00"),
    at("Slate window prepare", "4C 8B DC 55 48 81 EC 10 03 00 00 48 8B 05 ?? ?? ?? ??"),
    at("AddTemporalAA", "48 89 5C 24 10 48 89 7C 24 18 55 41 54 41 55 41 56 41 57 48 8B EC 48 83 EC 30"),
    at("temporal AA process", "40 55 53 56 41 54 41 55 41 56 41 57 48 8D AC 24 F0 FD FF FF 48 81 EC 10 03 00 00 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 B0 01 00 00"),
    at("graph recursive process", "4C 89 44 24 18 48 89 4C 24 08 53 48 83 EC 30 48 8B 1A"),
];

/// How a derived site follows from the site it is found through.
#[derive(Clone, Copy)]
enum Step {
    /// The RIP-relative target of the instruction matching `instruction` at `from + at`, whose
    /// `disp32` sits at `disp` and which ends at `next`.
    Operand {
        at: usize,
        instruction: &'static str,
        disp: usize,
        next: usize,
    },
    /// The function a vtable at `from` holds at this byte offset: a documented slot, which no
    /// instruction addresses itself, checked by the function's entry.
    Slot(usize),
}

/// (site, the site it is found through, step), in resolution order.
const DERIVED: [(usize, usize, Step); 9] = [
    (
        DESCRIPTOR_AA,
        Site::AaPassVtable as usize,
        Step::Slot(DESCRIPTOR_SLOT),
    ),
    (
        DESCRIPTOR_COMPOSITE,
        Site::CompositePassVtable as usize,
        Step::Slot(DESCRIPTOR_SLOT),
    ),
    (
        Site::HudPassVtable as usize,
        Site::HudPassConstructor as usize,
        Step::Operand {
            at: 0x38,
            instruction: "48 8D 05 ?? ?? ?? ??",
            disp: 3,
            next: 7,
        },
    ),
    (
        DESCRIPTOR_HUD,
        Site::HudPassVtable as usize,
        Step::Slot(DESCRIPTOR_SLOT),
    ),
    // FD3D11DynamicRHI::RHISetShaderUniformBuffer (pixel) and RHICreatePixelShader.
    (
        RHI_PIXEL_UNIFORM,
        Site::RhiVtable as usize,
        Step::Slot(424 * 8),
    ),
    (
        CREATE_PIXEL_SHADER,
        Site::RhiVtable as usize,
        Step::Slot(179 * 8),
    ),
    (
        FRAME_TASK_CONSTRUCT,
        Site::FrameTaskDispatch as usize,
        Step::Operand {
            // mov [rsp+60],rbx; call construct
            at: 0x102,
            instruction: "48 89 5C 24 60 E8 ?? ?? ?? ??",
            disp: 6,
            next: 10,
        },
    ),
    (
        Site::FrameTaskVtable as usize,
        FRAME_TASK_CONSTRUCT,
        Step::Operand {
            at: 0x21,
            instruction: "48 8D 05 ?? ?? ?? ??",
            disp: 3,
            next: 7,
        },
    ),
    (
        FRAME_TASK_EXECUTE,
        Site::FrameTaskVtable as usize,
        Step::Slot(8),
    ),
];

/// The shader-depth comparison `comiss xmm0,[rsi+220]; jbe` of the six byte-identical base pass
/// pixel shader parameter instantiations, with the patched opcode a wildcard. Every match is a
/// member of the group.
const SHADER_DEPTH: &str =
    "0F 2F 86 20 02 00 00 ?? 04 4C 8B 68 08 48 8B CE E8 ?? ?? ?? ?? 80 3D ?? ?? ?? ?? 00 48 8B 00";
const SHADER_DEPTH_NAME: &str = "shader depth gates";

/// Every site's RVA or the refusal that names it, and the shader-depth gates.
struct Found {
    sites: Sites,
    shader_depth: Result<Vec<usize>, Refusal>,
}

fn matches_at(read: &mut impl FnMut(usize, &mut [u8]) -> bool, rva: usize, text: &str) -> bool {
    let Some(pattern) = Pattern::parse(text) else {
        return false;
    };
    let mut bytes = vec![0; pattern.len()];
    read(rva, &mut bytes) && pattern.matches(&bytes)
}

fn follow(
    from: usize,
    step: Step,
    image_base: u64,
    read: &mut impl FnMut(usize, &mut [u8]) -> bool,
) -> Option<usize> {
    match step {
        Step::Operand {
            at,
            instruction,
            disp,
            next,
        } => {
            let start = from.checked_add(at)?;
            let mut operand = [0; 4];
            (matches_at(read, start, instruction) && read(start + disp, &mut operand))
                .then(|| (start + next).checked_add_signed(i32::from_le_bytes(operand) as isize))?
        }
        Step::Slot(offset) => {
            let mut entry = [0; 8];
            read(from.checked_add(offset)?, &mut entry)
                .then(|| u64::from_le_bytes(entry).checked_sub(image_base))?
                .and_then(|rva| usize::try_from(rva).ok())
        }
    }
}

/// Every match of `text` in the code sections.
fn find_all(code: &[Section], text: &str) -> Vec<usize> {
    let Some(pattern) = Pattern::parse(text) else {
        return Vec::new();
    };
    code.iter()
        .flat_map(|section| {
            (0..section.bytes.len())
                .filter(|&offset| pattern.matches(&section.bytes[offset..]))
                .map(move |offset| section.rva + offset)
        })
        .collect()
}

/// Resolves every site from the code sections, the base the image's pointers are relocated to,
/// and an RVA reader.
fn resolve_image(
    code: &[Section],
    image_base: u64,
    mut read: impl FnMut(usize, &mut [u8]) -> bool,
) -> Found {
    let mut sites = signature::resolve_each(code, &SIGNATURES);
    for (site, from, step) in DERIVED {
        let signature = &SIGNATURES[site];
        let refuse = |miss| Refusal {
            name: signature.name,
            miss,
        };
        // A refused anchor refuses what is found through it, by the anchor's name.
        sites[site] = sites[from].and_then(|from| {
            let rva = follow(from, step, image_base, &mut read).ok_or(refuse(Miss::Operand))?;
            if signature.check.is_empty() || matches_at(&mut read, rva, signature.check) {
                Ok(rva)
            } else {
                Err(refuse(Miss::Missing))
            }
        });
    }
    let gates = find_all(code, SHADER_DEPTH);
    let shader_depth = if gates.is_empty() {
        Err(Refusal {
            name: SHADER_DEPTH_NAME,
            miss: Miss::Missing,
        })
    } else {
        Ok(gates)
    };
    Found {
        sites,
        shader_depth,
    }
}

static FOUND: OnceLock<Result<Found, String>> = OnceLock::new();

#[cfg(windows)]
unsafe extern "C" fn read_memory(address: usize, out: *mut std::ffi::c_void, size: usize) -> bool {
    unsafe { crate::sys::rsf_ac7_copy_memory(out, address as *const std::ffi::c_void, size) }
}

/// Copies the bytes at `address` under the guarded read.
#[cfg(windows)]
fn read_bytes(address: usize, out: &mut [u8]) -> bool {
    unsafe { read_memory(address, out.as_mut_ptr().cast(), out.len()) }
}

/// The executable sections of the module at `base` (the decrypted image in memory).
#[cfg(windows)]
fn live_code(base: usize) -> Option<Vec<(usize, Vec<u8>)>> {
    signature::mapped_sections(base, read_memory, |_, flags| flags & EXECUTE != 0)
}

/// The process executable's base.
#[cfg(windows)]
pub fn main_module() -> usize {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    unsafe { GetModuleHandleW(None) }.map_or(0, |module| module.0 as usize)
}

#[cfg(all(windows, not(feature = "warp-fixture")))]
fn discover() -> Result<Found, String> {
    let base = main_module();
    let code = live_code(base).ok_or("Ace7Game.exe sections unreadable")?;
    Ok(resolve_image(
        &signature::views(&code),
        base as u64,
        |rva, out| read_bytes(base.wrapping_add(rva), out),
    ))
}

/// The WARP fixtures stand in for the recorded build and its images.
#[cfg(feature = "warp-fixture")]
fn discover() -> Result<Found, String> {
    Ok(Found {
        sites: RECORDED.iter().map(|&rva| Ok(rva)).collect(),
        shader_depth: Ok(SHADER_DEPTH_RECORDED.to_vec()),
    })
}

#[cfg(not(any(windows, feature = "warp-fixture")))]
fn discover() -> Result<Found, String> {
    Err("not a Windows process".into())
}

fn found() -> Option<&'static Found> {
    #[cfg(feature = "warp-fixture")]
    let found = FOUND.get_or_init(discover);
    #[cfg(not(feature = "warp-fixture"))]
    let found = FOUND.get()?;
    found.as_ref().ok()
}

/// Discovers every site once per process, on the first call, and admits `sites`: the error names
/// the first of them that did not resolve. Call it only once the code is decrypted.
pub fn admit(sites: impl IntoIterator<Item = usize>) -> Result<(), String> {
    let found = FOUND.get_or_init(discover).as_ref().map_err(Clone::clone)?;
    sites.into_iter().try_for_each(|site| {
        found.sites[site]
            .map(|_| ())
            .map_err(|refusal| refusal.to_string())
    })
}

/// The shader-depth gates, discovered as by [`admit`].
pub fn admit_shader_depth() -> Result<&'static [usize], String> {
    let found = FOUND.get_or_init(discover).as_ref().map_err(Clone::clone)?;
    match &found.shader_depth {
        Ok(gates) => Ok(gates),
        Err(refusal) => Err(refusal.to_string()),
    }
}

/// How many sites resolved and how many shader-depth gates matched, for the log.
pub fn summary() -> String {
    match FOUND.get() {
        Some(Ok(found)) => format!(
            "{} of {} AC7 sites resolved, {} shader depth gates",
            found.sites.iter().filter(|site| site.is_ok()).count(),
            SITES,
            found.shader_depth.as_ref().map_or(0, Vec::len)
        ),
        Some(Err(reason)) => format!("AC7 site discovery refused: {reason}"),
        None => "AC7 sites not discovered yet".into(),
    }
}

/// The discovered RVA of a hook index or [`Site`]. Zero until [`admit`] resolved it: nothing
/// hooks, patches, calls or reads a site before admission, and [`verify`] refuses zero.
pub fn rva(site: usize) -> usize {
    found()
        .and_then(|found| found.sites.get(site)?.ok())
        .unwrap_or(0)
}

/// The name a refusal of a hook index or [`Site`] gives.
pub fn name(site: usize) -> &'static str {
    SIGNATURES
        .get(site)
        .map_or("unknown", |signature| signature.name)
}

/// The discovered shader-depth gates; empty until admitted.
pub fn shader_depth_gates() -> &'static [usize] {
    found()
        .and_then(|found| found.shader_depth.as_ref().ok())
        .map_or(&[], Vec::as_slice)
}

/// Whether the live code at a resolved code site of the image at `base` still matches its
/// pattern (or its entry check), so a site changed since discovery (another hook, a wrong match)
/// refuses before it is hooked, patched or called. False for globals and unresolved sites.
#[cfg(windows)]
pub fn verify(base: usize, site: usize) -> bool {
    let resolved = rva(site);
    SIGNATURES
        .get(site)
        .and_then(|signature| signature.guard(resolved))
        .filter(|_| resolved != 0 && base != 0)
        .is_some_and(|(text, start)| {
            matches_at(
                &mut |rva, out| read_bytes(base.wrapping_add(rva), out),
                start,
                text,
            )
        })
}

/// Copies the live bytes of a code site of the process executable into `out` once it still
/// matches its pattern: the native bytes a patch keeps to restore.
#[cfg(windows)]
pub fn live(site: usize, out: &mut [u8]) -> bool {
    let base = main_module();
    verify(base, site) && read_bytes(base.wrapping_add(rva(site)), out)
}

/// The TrueSkyPluginRender_MT.dll sites of the native renderer's cloud patches and relays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum TrueSky {
    /// RenderInterfaceViewFrame's `mov eax,[rdi+0x210]` divisor load.
    Resolution,
    /// The depth bounds producer's `call [r10+0x148]`.
    DepthCall,
    /// The mixed-resolution pass selector's `jne`, four bytes into its `cmp r12d,2`.
    DepthBranch,
    /// The normalized scene-depth bounds format request `mov dword [rsp+20],16`.
    DepthFormat,
    /// composite_tile's `call [rax+0x130]`, sixteen bytes into its pattern.
    MotionCall,
}

/// Indexed by [`TrueSky`]. Derived from the shipped plugin (1,711,616 bytes).
#[rustfmt::skip]
const TRUESKY: [Signature; 5] = [
    at("TrueSky cloud divisor", "8B 87 10 02 00 00 89 44 24 38 48 8B 45 D0 0F 29 45 E0"),
    at("TrueSky depth dispatch", "41 FF 92 48 01 00 00 48 8B 4B 08 4C 8D 83 B0 01 00 00"),
    within("TrueSky depth pass selector", 4, "41 83 FC 02 75 1D 48 8B 4B 08 4C 8D 43 18 48 8B D7"),
    at("TrueSky depth format", "C7 44 24 20 16 00 00 00 FF 50 58 8B 87 94 00 00 00"),
    within("TrueSky composite_tile draw", 16, "49 8B 8E 28 02 00 00 45 8B C4 49 8B D7 48 8B 01 FF 90 30 01 00 00 49 8B 9E 60 02 00 00 48 8D 0D ?? ?? ?? ?? 48 8B 03 4C 8B 40 60 48 3B C1 0F 85 ?? ?? ?? ?? FF 4B 28 48 8B 4B 38 48 85 C9 74 0B 48 8B 01 33 D2 4D 8B 07 FF 50 50 48 C7 43 38 00 00 00 00 4C 8B 8D 08 04 00 00"),
];

/// The plugin module base and its sites, discovered once before the first patch.
static TRUESKY_FOUND: Mutex<Option<(usize, Sites)>> = Mutex::new(None);

/// Each site's RVA or the refusal that names it.
type Sites = Vec<Result<usize, Refusal>>;

/// The RVA of a TrueSky site in the plugin loaded at `base`, discovered from the module's code
/// sections on the first call for that base, before any of its sites is patched. The error names
/// the site.
pub fn truesky(base: usize, site: TrueSky) -> Result<usize, String> {
    let mut found = TRUESKY_FOUND
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if found.as_ref().is_none_or(|(known, _)| *known != base) {
        *found = Some((base, truesky_sites(base)));
    }
    let (_, sites) = found.as_ref().expect("discovered above");
    sites[site as usize].map_err(|refusal| refusal.to_string())
}

#[cfg(all(windows, not(feature = "warp-fixture")))]
fn truesky_sites(base: usize) -> Sites {
    match live_code(base) {
        Some(code) => signature::resolve_each(&signature::views(&code), &TRUESKY),
        None => truesky_refused(),
    }
}

#[cfg(feature = "warp-fixture")]
fn truesky_sites(_: usize) -> Sites {
    TRUESKY_RECORDED.iter().map(|&rva| Ok(rva)).collect()
}

#[cfg(not(any(windows, feature = "warp-fixture")))]
fn truesky_sites(_: usize) -> Sites {
    truesky_refused()
}

#[cfg(not(feature = "warp-fixture"))]
fn truesky_refused() -> Sites {
    TRUESKY
        .iter()
        .map(|signature| {
            Err(Refusal {
                name: signature.name,
                miss: Miss::Missing,
            })
        })
        .collect()
}

/// The build 9855922 RVAs, indexed like [`SIGNATURES`]: documentation, the image test and the
/// WARP fixtures' stand-in.
#[cfg(any(test, feature = "warp-fixture"))]
#[rustfmt::skip]
const RECORDED: [usize; SITES] = [
    // Hooks.
    0x10b_0220, 0xfb_fe90, 0x100_4960, 0xfc_bc90, 0xfc_86c0, 0x10b_1650, 0x109_8ff0, 0xfb_6f20,
    0xff_7b50, 0xfb_7bc0, 0xfb_7b50, 0x109_91e0, 0xff_b900, 0x10b_3970, 0x112_afa0, 0x111_3080,
    0x4d_6340, 0x4d_6660, 0x138_0d80, 0x3a_3f70, 0x113_a0c0, 0xce_a130, 0x3c_d130, 0x1ab_0200,
    0x3c_7a30, 0x178_1e80, 0x177_cdd0, 0x1ac_3600, 0x134_5310, 0x133_c6c0, 0x134_6520, 0x134_3660,
    0x135_1e10, 0x10b_e2b0, 0x109_7d20, 0x109_d320, 0x116_8d40, 0xe9_7d80, 0xe3_9520, 0xe1_f8d0,
    0xe9_b880, 0xed_b9d0, 0x18d_21c0, 0xf4_c620, 0x222_5470, 0x222_afa0, 0x10e_cdd0, 0x109_f300,
    0x112_ff00, 0x3a_5a20, 0x3a_8b60, 0x120_7df0, 0x120_c540, 0x1ad_a010, 0xa8_8dc0, 0x1ad_bcb0,
    0xe3_1e90, 0xeb_f050, 0x115_8250, 0x188_cfa0, 0x190_e740, 0x190_4d40, 0x190_7980, 0x190_d510,
    0x190_ce60,
    // Helpers.
    0xfb_7a00, 0x112_6090, 0x121_83c0, 0x116_ea00, 0x109_e0d0, 0xee_93f0, 0x92_3e60, 0xfc_f880,
    0x1ab_7dc0, 0xde_5cf0, 0x19b_6fe0, 0xe9_3670, 0x10b_4870, 0x109_dc70, 0x112_7a20, 0xff_9af0,
    0x113_46d0,
    // Globals.
    0x3cb_bc28, 0x3c7_83d0, 0x3ca_a6c8, 0x3a4_a6f8, 0x3c7_8358, 0x3a8_2008, 0x3a8_2130, 0x3a8_2140,
    0x3a8_2158, 0x3a8_4660, 0x3a7_4730, 0x3cc_0150, 0x3a7_6c90,
    // Anchors.
    0x2ac_5d48, 0x2ac_7288, 0xff_2090, 0x2ac_7310, 0x2a5_4650, 0x3a_7e20, 0x257_bc78,
    // Patch sites.
    0x116_8f6f, 0x109_7a0c, 0x109_d03a, 0x109_d061, 0x115_83a6, 0x112_b1f3, 0x118_23de, 0x10b_e329,
    0x118_a4e3, 0x10f_dd09, 0x118_38c4, 0x109_5620,
    // Motion capture.
    0x120_e67c, 0x10f_dc00, 0x118_3820, 0x10e_e670, 0x4d_69a0, 0xff_6ad0, 0x10a_eca0, 0x10b_3f10,
];

#[cfg(any(test, feature = "warp-fixture"))]
const SHADER_DEPTH_RECORDED: [usize; 6] = [
    0x102_5b12, 0x102_94d2, 0x102_ada2, 0x102_c672, 0x102_e792, 0x103_1902,
];

#[cfg(any(test, feature = "warp-fixture"))]
const TRUESKY_RECORDED: [usize; 5] = [0x8_7f6d, 0xc_003f, 0xb_ff0b, 0xb_fc05, 0xa_bc6b];

#[cfg(test)]
mod tests;
