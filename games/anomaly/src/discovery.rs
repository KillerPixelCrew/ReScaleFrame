// SPDX-License-Identifier: MIT
//! Signature discovery of every Anomaly DX11 site this adapter hooks, patches, calls or reads.
//!
//! At start-up the running executable's code is copied once from live memory and every site is
//! resolved from a pattern that is unique in its executable sections (rel32 branches, RIP
//! displacements and the few bytes the SSE and AVX builds encode differently are wildcards), so
//! one description resolves in AnomalyDX11.exe and AnomalyDX11AVX.exe alike. Globals and vtables
//! resolve through the RIP-relative operand of an instruction that uses them. Functions reached
//! only through a vtable (rmNear, rmFar, rmNormal, CSkeletonX_PM::Render, the position binder's
//! setup) are read from their discovered vtable slot, and the first-person `set_P`/`rmNear` call
//! pairs are decoded from the functions that hold them. Before a site is used its expected code
//! is matched at the found address: the finding pattern, or for a site found through a caller or
//! a slot its own check (one encoding per codegen where SSE and AVX differ in length). A site that
//! is missing, ambiguous or does not match refuses only the feature it serves and names itself.
//! The executable's SHA-256 is logged as evidence and gates nothing.
//!
//! The profile records the hooks and guards consume are built from the resolved sites
//! (`records`): the 16 expected bytes of a detour entry are copied from the found site, so a site
//! patched between discovery and installation refuses. Recorded RVAs are those of the 1.5.3
//! DX11 builds; docs/research/anomaly-native-hooks.md records how each site is found, and
//! tools/derive-site-patterns.py derived the patterns from both executables.
// Fixture builds run on the recorded records and only the tests resolve sites.
#![cfg_attr(all(feature = "recorded-profiles", not(test)), allow(dead_code))]

use crate::engine_guards::{NativeSite, UniqueGuard, matches_text};
use crate::geometry_profile::Profile as GeometryProfile;
use crate::input::InputProfile;
use crate::motion_profile::MotionProfile;
use crate::native_profile::Profile as NativeProfile;
use crate::sr_profile::Profile as SrProfile;
use rsf_game_sys::signature::{self, Find, Section, Signature};
use std::ffi::{CStr, CString};

/// Every discovered site. The pattern sites come first, in [`SIGNATURES`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Site {
    Begin,
    End,
    SetCacheXform,
    Render,
    RenderUi,
    RenderToTarget,
    ResetBegin,
    ResetEnd,
    DeviceWidth,
    DeviceHeight,
    DeviceFrame,
    RendererVtable,
    RenderHud,
    RenderSorted,
    RenderEmissive,
    ModelCreate,
    ModelCreateChild,
    ModelDuplicate,
    ModelDelete,
    VisualRender,
    ProgressiveRender,
    SkeletonStRender,
    SkeletonPmVtable,
    TreeStRender,
    TreePmRender,
    ConstantsFlush,
    DetailRender,
    SetW,
    BoneInstance,
    TransformR,
    PhaseCompare,
    SvpFrame,
    BufferFlush,
    TreePmLastLod,
    RenderPhase,
    SecondViewport,
    WorldMatrix,
    VertexConstantBuffers,
    VisualVtable,
    KinematicsVtable,
    AnimatedVtable,
    LodVtable,
    PhaseScenePrepare,
    PhaseCombine,
    PhaseSunshafts,
    PhaseBlur,
    SetP,
    SetV,
    SurfaceSet,
    SetMipLodBias,
    SetMaxAnisotropy,
    Invalidate,
    USetrt,
    ParticleRender,
    AccumPoint,
    SunCascades,
    PrepareZCall,
    PrepareCall,
    CombineCall,
    SunshaftsCall,
    BlurCall,
    CascadesCall,
    RenderHudUi,
    BinderRegistration,
    GetCBuffer,
    ReflectedSize,
    Backend,
    DeviceView,
    DeviceProject,
    DeviceFullTransform,
    DevicePrecacheFrame,
    LoadScreenRegistered,
    HwDevice,
    HwContext,
    HwSwapChain,
    HwBaseRt,
    HwBaseZb,
    RenderImplementation,
    OptionsWord0,
    OptionsWord3,
    SunshaftsMode,
    SamplerCache,
    UserMipBias,
    UserAnisotropy,
    SsaoSetViewport,
    SsaoViewportGuard,
    SsaoViewport,
    CreateShader,
    PhaseSceneBegin,
    PhaseSceneEnd,
    CacheCall,
    CompileCall,
    ShaderCompile,
    CompileSetup,
    SourceName,
    TessDispatch,
    HullCreate,
    DomainCreate,
    HullWrapper,
    DomainWrapper,
    OnFrame,
    KeyUpdate,
    MouseUpdate,
    InputVtable,
    // Derived from the pattern sites and the image.
    RmNear,
    RmFar,
    RmNormal,
    SkeletonPmRender,
    PositionSetupVtable,
    PositionSetup,
    HudSetP,
    HudNear,
    HudUiSetP,
    HudUiNear,
    SortedSetP,
    SortedNear,
    EmissiveSetP,
    EmissiveNear,
    ParticleSetP,
    ParticleNear,
    AccumSetP,
    AccumNear,
}

const PATTERN_SITES: usize = Site::RmNear as usize;
pub(crate) const SITES: usize = Site::AccumNear as usize + 1;

/// The feature a site serves. A refused site refuses the records that need its feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum Feature {
    /// The observer's detours and the device globals every stage reads.
    Core,
    /// Sites geometry and SR share: the renderer vtable and the three r_dsgraph renders.
    Engine,
    Geometry,
    Sr,
    Motion,
    Input,
}
use Feature::*;

const FEATURES: usize = Input as usize + 1;
const FEATURE_NAMES: [&str; FEATURES] = ["native", "engine", "geometry", "SR", "motion", "input"];

struct Entry {
    feature: Feature,
    name: &'static CStr,
    pattern: &'static CStr,
    find: Find,
    /// Expected code at the site when the pattern does not cover it: patterns separated by `|`,
    /// one per codegen. Empty when there is none.
    check: &'static CStr,
}

const fn at(feature: Feature, name: &'static CStr, pattern: &'static CStr) -> Entry {
    after(feature, name, 0, pattern)
}

/// The site is the match plus `offset`.
const fn after(
    feature: Feature,
    name: &'static CStr,
    offset: usize,
    pattern: &'static CStr,
) -> Entry {
    Entry {
        feature,
        name,
        pattern,
        find: Find::At(offset),
        check: c"",
    }
}

/// A function found through its only distinctive call, with its own entry checked.
const fn called(
    feature: Feature,
    name: &'static CStr,
    pattern: &'static CStr,
    check: &'static CStr,
) -> Entry {
    Entry {
        feature,
        name,
        pattern,
        find: Find::Rip { disp: 1, next: 5 },
        check,
    }
}

/// A global addressed by the `disp32` at `disp` of the instruction ending at `next`.
const fn global(
    feature: Feature,
    name: &'static CStr,
    disp: usize,
    next: usize,
    pattern: &'static CStr,
) -> Entry {
    Entry {
        feature,
        name,
        pattern,
        find: Find::Rip { disp, next },
        check: c"",
    }
}

/// Indexed by [`Site`]. Derived from both 1.5.3 DX11 executables by tools/derive-site-patterns.py.
const SIGNATURES: [Entry; PATTERN_SITES] = [
    // Site::Begin
    at(
        Core,
        c"IRenderDeviceRender::Begin",
        c"40 53 48 83 EC 20 48 8B D9 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? BA 02 00 00 00",
    ),
    // Site::End
    at(
        Core,
        c"IRenderDeviceRender::End",
        c"48 83 EC 28 83 3D ?? ?? ?? ?? 00 74 06 48 8B 01",
    ),
    // Site::SetCacheXform
    at(
        Core,
        c"IRenderDeviceRender::SetCacheXform",
        c"40 53 48 83 EC 20 48 8D 0D ?? ?? ?? ?? 49 8B D8",
    ),
    // Site::Render
    at(
        Core,
        c"CRender::Render",
        c"48 8B C4 48 89 48 08 53 56 57 41 54 41 55 41 56 41 57 48 81 EC ?? 01 00 00",
    ),
    // Site::RenderUi
    at(
        Core,
        c"CHUDManager::RenderUI",
        c"40 53 48 83 EC 40 F7 05 ?? ?? ?? ?? 00 10 00 00",
    ),
    // Site::RenderToTarget
    at(
        Core,
        c"CRender::RenderToTarget",
        c"40 53 48 83 EC 30 44 8B C2 41 83 E8 01 74 4D 41 83 F8 01",
    ),
    // Site::ResetBegin
    at(
        Core,
        c"CRender::reset_begin",
        c"48 89 4C 24 08 56 57 41 56 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 60 48 8B \
        F9",
    ),
    // Site::ResetEnd
    at(
        Core,
        c"CRender::reset_end",
        c"40 57 48 83 EC 50 48 89 5C 24 68 48 8B F9 48 89 74 24 48",
    ),
    // Site::DeviceWidth
    global(
        Core,
        c"Device.dwWidth",
        3,
        7,
        c"44 8B 05 ?? ?? ?? ?? 49 8D 8F 88 05 00 00",
    ),
    // Site::DeviceHeight
    global(
        Core,
        c"Device.dwHeight",
        2,
        6,
        c"8B 05 ?? ?? ?? ?? 48 8B F1 3D 58 02 00 00",
    ),
    // Site::DeviceFrame
    global(
        Core,
        c"Device.dwFrame",
        2,
        6,
        c"8B 05 ?? ?? ?? ?? F7 F1 85 D2 75 03 B0 01",
    ),
    // Site::RendererVtable
    global(
        Engine,
        c"CRender vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 89 07 48 8D 8F F0 04 00 00",
    ),
    // Site::RenderHud
    called(
        Engine,
        c"R_dsgraph_structure::r_dsgraph_render_hud",
        c"E8 ?? ?? ?? ?? 41 B0 01 33 D2 48 8B CE E8 ?? ?? ?? ?? 41 B0 01",
        c"48 8B C4 48 89 70 10 57 48 81 EC ?? 01 00 00",
    ),
    // Site::RenderSorted
    at(
        Engine,
        c"R_dsgraph_structure::r_dsgraph_render_sorted",
        c"48 8B C4 48 89 58 08 48 89 70 10 57 48 81 EC ?? 01 00 00 83 B9 E8 01 00 00 00",
    ),
    // Site::RenderEmissive
    at(
        Engine,
        c"R_dsgraph_structure::r_dsgraph_render_emissive",
        c"48 8B C4 48 89 58 08 48 89 70 10 57 48 81 EC ?? 01 00 00 83 B9 58 02 00 00 00",
    ),
    // Site::ModelCreate
    at(
        Geometry,
        c"CRender::model_Create",
        c"48 8B 89 58 07 00 00 E9 ?? ?? ?? ?? CC CC CC CC 48 8B 89 58 07 00 00 E9 ?? ?? ?? ?? CC \
        CC CC CC 48 89 5C 24 08",
    ),
    // Site::ModelCreateChild
    at(
        Geometry,
        c"CRender::model_CreateChild",
        c"48 8B 89 58 07 00 00 E9 ?? ?? ?? ?? CC CC CC CC 48 89 5C 24 08",
    ),
    // Site::ModelDuplicate
    at(
        Geometry,
        c"CRender::model_Duplicate",
        c"48 8B 89 58 07 00 00 E9 ?? ?? ?? ?? CC CC CC CC 48 8B 81 58 07 00 00",
    ),
    // Site::ModelDelete
    at(
        Geometry,
        c"CRender::model_Delete",
        c"40 53 48 83 EC 20 48 8B 02 48 8B DA 48 8B 89 58 07 00 00",
    ),
    // Site::VisualRender
    at(
        Geometry,
        c"Fvisual::Render",
        c"48 89 5C 24 10 55 56 57 41 56 41 57 48 83 EC 30 48 8B B9 98 00 00 00",
    ),
    // Site::ProgressiveRender
    at(
        Geometry,
        c"FProgressive::Render",
        c"48 89 5C 24 10 48 89 6C 24 20 56 57 41 54 41 56 41 57 48 83 EC 30",
    ),
    // Site::SkeletonStRender
    at(
        Geometry,
        c"CSkeletonX_ST::Render",
        c"48 83 EC 38 4C 8B C1 48 8D 51 68 48 81 C1 A0 00 00 00",
    ),
    // Site::SkeletonPmVtable
    global(
        Geometry,
        c"CSkeletonX_PM vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 8B F9 48 89 01 8B DA 48 8D 05 ?? ?? ?? ?? 48 89 41 60 48 81 C1 \
        D0 00 00 00",
    ),
    // Site::TreeStRender
    at(
        Geometry,
        c"FTreeVisual_ST::Render",
        c"48 89 5C 24 10 55 56 57 41 56 41 57 48 83 EC 30 48 8B D9",
    ),
    // Site::TreePmRender
    at(
        Geometry,
        c"FTreeVisual_PM::Render",
        c"48 89 5C 24 18 48 89 6C 24 20 56 57 41 54 41 56 41 57 48 83 EC 40",
    ),
    // Site::ConstantsFlush
    at(
        Geometry,
        c"CBackend::constants.flush",
        c"48 89 5C 24 08 57 48 83 EC 20 48 8D 1D ?? ?? ?? ?? BF 0E 00 00 00",
    ),
    // Site::DetailRender
    at(
        Geometry,
        c"CDetailManager::Render",
        c"40 53 48 83 EC 20 48 83 B9 58 04 00 00 00 48 8B D9",
    ),
    // Site::SetW
    at(
        Geometry,
        c"R_xforms::set_W",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 8B 02 48 8D B1 00 01 00 00",
    ),
    // Site::BoneInstance
    at(
        Geometry,
        c"CKinematics::LL_GetBoneInstance",
        c"0F B7 C2 48 69 C0 A8 00 00 00 48 03 41 50 C3 CC",
    ),
    // Site::TransformR
    at(
        Geometry,
        c"CKinematics::LL_GetTransform_R",
        c"48 83 EC 28 48 8B 01 FF 50 48 48 83 C0 40 48 83 C4 28",
    ),
    // Site::PhaseCompare
    at(
        Geometry,
        c"Fvisual::Render phase compare",
        c"83 3D ?? ?? ?? ?? 01 0F 85 ?? ?? ?? ?? 81 3D ?? ?? ?? ?? 00 B0 00 00",
    ),
    // Site::SvpFrame
    at(
        Geometry,
        c"CSecondVPParams::IsSVPFrame",
        c"80 39 00 74 15 0F B6 49 01 33 D2 8B 05 ?? ?? ?? ??",
    ),
    // Site::BufferFlush
    at(
        Geometry,
        c"dx10ConstantBuffer::Flush",
        c"40 53 48 83 EC 40 80 79 68 00 48 8B D9 74 32 48 8B 0D ?? ?? ?? ??",
    ),
    // Site::TreePmLastLod
    after(
        Geometry,
        c"FTreeVisual_PM last_lod load",
        5,
        c"E8 ?? ?? ?? ?? 44 8B 8B 08 01 00 00",
    ),
    // Site::RenderPhase
    global(
        Geometry,
        c"RImplementation.phase",
        2,
        7,
        c"83 3D ?? ?? ?? ?? 00 48 8B DA B8 02 00 00 00",
    ),
    // Site::SecondViewport
    global(
        Geometry,
        c"Device.m_SecondViewport",
        3,
        7,
        c"48 8D 0D ?? ?? ?? ?? 48 8B 90 B8 04 00 00",
    ),
    // Site::WorldMatrix
    global(
        Geometry,
        c"RCache.xforms",
        3,
        7,
        c"48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 49 8D 95 C4 02 00 00",
    ),
    // Site::VertexConstantBuffers
    global(
        Geometry,
        c"CBackend::m_aVertexConstants",
        13,
        17,
        c"48 89 5C 24 08 57 48 83 EC 20 48 8D 1D ?? ?? ?? ?? BF 0E 00 00 00",
    ),
    // Site::VisualVtable
    global(
        Geometry,
        c"FHierrarhyVisual vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 89 01 48 8B D9 75 4A",
    ),
    // Site::KinematicsVtable
    global(
        Geometry,
        c"CKinematics vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 89 03 48 8D 05 ?? ?? ?? ?? 48 89 83 80 00 00 00",
    ),
    // Site::AnimatedVtable
    global(
        Geometry,
        c"CKinematicsAnimated vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 89 03 48 8D 8B 84 01 00 00",
    ),
    // Site::LodVtable
    global(
        Geometry,
        c"FLOD vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 89 03 48 8B C3 48 C7 83 80 00 00 00 00 00 00 00",
    ),
    // Site::PhaseScenePrepare
    at(
        Sr,
        c"CRenderTarget::phase_scene_prepare",
        c"40 53 48 83 EC 50 F7 05 ?? ?? ?? ?? 00 40 00 00",
    ),
    // Site::PhaseCombine
    at(
        Sr,
        c"CRenderTarget::phase_combine",
        c"48 8B C4 55 56 41 54 41 55 41 56 48 8D A8 88 FE FF FF",
    ),
    // Site::PhaseSunshafts
    at(
        Sr,
        c"CRenderTarget::phase_sunshafts",
        c"4C 8B DC 55 41 56 49 8D 6B A1 48 81 EC ?? 00 00 00 48 8B 05 ?? ?? ?? ?? 4C 8B F1 ?? ?? \
        10 05 ?? ?? ?? ??",
    ),
    // Site::PhaseBlur
    called(
        Sr,
        c"CRenderTarget::phase_blur",
        c"E8 ?? ?? ?? ?? 49 8B CE E8 ?? ?? ?? ?? F7 05 ?? ?? ?? ?? 00 00 40 00",
        c"48 89 5C 24 20 55 56 57 41 56 41 57 48 8B EC 48 81 EC | 48 8B C4 48 89 58 20 55 56 57 \
        41 56 41 57 48 8D 68 A1 48 81 EC",
    ),
    // Site::SetP
    at(
        Sr,
        c"R_xforms::set_P",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 56 48 83 EC 20 8B 02",
    ),
    // Site::SetV
    at(
        Sr,
        c"R_xforms::set_V",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 41 56 41 57 48 83 EC 20 8B 02",
    ),
    // Site::SurfaceSet
    at(
        Sr,
        c"CTexture::surface_set",
        c"48 89 5C 24 18 55 56 57 48 83 EC 40 48 8B FA 48 8B D9",
    ),
    // Site::SetMipLodBias
    at(
        Sr,
        c"dx10SamplerStateCache::SetMipLODBias",
        c"40 57 48 81 EC A0 00 00 00 ?? ?? 10 81 9C 01 00 00",
    ),
    // Site::SetMaxAnisotropy
    at(
        Sr,
        c"dx10SamplerStateCache::SetMaxAnisotropy",
        c"40 57 48 81 EC A0 00 00 00 48 8B F9 83 FA 01 73 07",
    ),
    // Site::Invalidate
    at(
        Sr,
        c"CBackend::Invalidate",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 33 ED 48 8B D9 48 89 A9 78 \
        05 00 00",
    ),
    // Site::USetrt
    at(
        Sr,
        c"CRenderTarget::u_setrt(W,H)",
        c"48 83 EC 28 89 51 08 44 89 41 0C 4C 3B 0D ?? ?? ?? ??",
    ),
    // Site::ParticleRender
    at(
        Sr,
        c"PS::CParticleEffect::Render",
        c"40 53 48 81 EC 80 01 00 00 48 8B D9 E8 ?? ?? ?? ??",
    ),
    // Site::AccumPoint
    at(
        Sr,
        c"CRenderTarget::accum_point",
        c"48 8B C4 55 53 48 8D A8 ?? FE FF FF 48 81 EC ?? 02 00 00 48 89 70 18",
    ),
    // Site::SunCascades
    at(
        Sr,
        c"CRender::render_sun_cascades",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 56 48 83 EC 20 48 8B D9 \
        48 8B 0D ?? ?? ?? ??",
    ),
    // Site::PrepareZCall
    at(
        Sr,
        c"CRender::Render -> phase_scene_prepare (z-prepass, r4_R_render.cpp:278)",
        c"E8 ?? ?? ?? ?? 33 D2 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 41 B0 01",
    ),
    // Site::PrepareCall
    at(
        Sr,
        c"CRender::Render -> phase_scene_prepare (r4_R_render.cpp:285)",
        c"E8 ?? ?? ?? ?? 48 8B 1D ?? ?? ?? ?? 83 3D ?? ?? ?? ?? 00 74 30 FF 83 34 06 00 00",
    ),
    // Site::CombineCall
    at(
        Sr,
        c"CRender::Render -> phase_combine (r4_R_render.cpp:538)",
        c"E8 ?? ?? ?? ?? EB 39 48 8B 05 ?? ?? ?? ?? 48 89 44 24 30",
    ),
    // Site::SunshaftsCall
    at(
        Sr,
        c"phase_combine -> phase_sunshafts (phase_combine.cpp:374)",
        c"E8 ?? ?? ?? ?? 49 8B CE E8 ?? ?? ?? ?? 49 8B CE E8 ?? ?? ?? ?? F7 05 ?? ?? ?? ?? 00 00 \
        40 00",
    ),
    // Site::BlurCall
    at(
        Sr,
        c"phase_combine -> phase_blur (phase_combine.cpp:378)",
        c"E8 ?? ?? ?? ?? 49 8B CE E8 ?? ?? ?? ?? F7 05 ?? ?? ?? ?? 00 00 40 00",
    ),
    // Site::CascadesCall
    at(
        Sr,
        c"CRender::Render -> render_sun_cascades (r4_R_render.cpp:492)",
        c"E8 ?? ?? ?? ?? EB 15 E8 ?? ?? ?? ?? 48 8B CE E8 ?? ?? ?? ?? 48 8B CE",
    ),
    // Site::RenderHudUi
    called(
        Sr,
        c"R_dsgraph_structure::r_dsgraph_render_hud_ui",
        c"E8 ?? ?? ?? ?? 48 83 BE 60 07 00 00 00 74 18",
        c"48 8B C4 53 48 81 EC ?? 01 00 00",
    ),
    // Site::BinderRegistration
    at(
        Sr,
        c"position binder registration",
        c"4C 8D 05 ?? ?? ?? ?? 48 8B CB 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 4C 8D 05 ?? ?? ?? ?? \
        48 8B CB 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 4C 8D 05 ?? ?? ?? ?? 48 8B CB 48 8D 15 ?? ?? \
        ?? ?? E8 ?? ?? ?? ?? 4C 8D 05 ?? ?? ?? ?? 48 8B CB 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 \
        8B 0D ?? ?? ?? ??",
    ),
    // Site::GetCBuffer
    at(
        Sr,
        c"R_constants::GetCBuffer",
        c"48 83 EC 38 45 85 C0 75 1B 0F B7 42 12 48 8D 0D ?? ?? ?? ?? 83 E0 0F 48 8B 84 C1 28 03 \
        00 00 48 83 C4 38 C3 41 83 F8 01 75 1E 8B 42 10 48 8D 0D ?? ?? ?? ?? 48 C1 E8 0C 83 E0 0F \
        48 8B 84 C1 B8 02 00 00 48 83 C4 38 C3 41 83 F8 02 75 1E 8B 42 10 48 8D 0D ?? ?? ?? ?? 48 \
        C1 E8 08 83 E0 0F 48 8B 84 C1 98 03 00 00 48 83 C4 38 C3 41 83 F8 03 75 1E 8B 42 10 48 8D \
        0D ?? ?? ?? ?? 48 C1 E8 14 83 E0 0F 48 8B 84 C1 08 04 00 00 48 83 C4 38 C3 41 83 F8 04 75 \
        1B 0F B6 42 13 48 8D 0D ?? ?? ?? ?? 83 E0 0F 48 8B 84 C1 78 04 00 00 48 83 C4 38 C3 41 83 \
        F8 05 75 1B 8B 42 10 48 8D 0D ?? ?? ?? ?? 48 C1 E8 1C 48 8B 84 C1 E8 04 00 00 48 83 C4 38 \
        C3",
    ),
    // Site::ReflectedSize
    at(
        Sr,
        c"constant buffer reflected size",
        c"48 8D 54 24 40 4C 89 6B 10 49 8B CC 4C 89 6B 20 4C 89 6B 28 4C 89 6B 30 4C 89 6B 38 4C \
        89 6B 40 4C 89 6B 48 C6 43 68 01 49 8B 04 24 FF 10 48 8B 54 24 40 48 8B 0D ?? ?? ?? ?? E8 \
        ?? ?? ?? ?? 48 85 C0 74 02 FF 00 48 8B 4B 10 48 85 C9 74 0F FF 09 48 8B 4B 10 44 39 29 75 \
        04 4C 89 6B 10 8B 54 24 4C 48 8D 4B 20 48 89 43 10 8B 44 24 48 89 43 18 8B 44 24 50 89 43 \
        58",
    ),
    // Site::Backend
    global(
        Sr,
        c"RCache",
        3,
        7,
        c"48 8D 0D ?? ?? ?? ?? 48 8B 52 08 45 33 C0",
    ),
    // Site::DeviceView
    global(
        Sr,
        c"Device.mView",
        3,
        7,
        c"4C 8D 2D ?? ?? ?? ?? 49 63 C9 BE 04 00 00 00",
    ),
    // Site::DeviceProject
    global(
        Sr,
        c"Device.mProject",
        3,
        7,
        c"48 8D 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 95 ?? ?? 00 00",
    ),
    // Site::DeviceFullTransform
    global(
        Sr,
        c"Device.mFullTransform",
        3,
        7,
        c"48 8D 15 ?? ?? ?? ?? 48 8D 4C 24 20 41 B8 2F 00 00 00",
    ),
    // Site::DevicePrecacheFrame
    global(
        Sr,
        c"Device.dwPrecacheFrame",
        2,
        7,
        c"83 3D ?? ?? ?? ?? 00 48 8B F2 48 8B ?? 74 10",
    ),
    // Site::LoadScreenRegistered
    global(
        Sr,
        c"load_screen_renderer.b_registered",
        2,
        7,
        c"80 3D ?? ?? ?? ?? 00 75 27 45 33 C9 48 8D 15 ?? ?? ?? ??",
    ),
    // Site::HwDevice
    global(
        Sr,
        c"HW.pDevice",
        3,
        7,
        c"48 8B 0D ?? ?? ?? ?? 4C 8D 4B 40 4C 8D 44 24 58",
    ),
    // Site::HwContext
    global(
        Sr,
        c"HW.pContext",
        3,
        7,
        c"48 8B 0D ?? ?? ?? ?? 48 8B 01 48 83 C4 50",
    ),
    // Site::HwSwapChain
    global(
        Sr,
        c"HW.m_pSwapChain",
        3,
        7,
        c"48 8B 0D ?? ?? ?? ?? 48 8D 94 24 88 40 00 00",
    ),
    // Site::HwBaseRt
    global(
        Sr,
        c"HW.pBaseRT",
        3,
        7,
        c"48 8B 05 ?? ?? ?? ?? 48 3B 83 78 05 00 00",
    ),
    // Site::HwBaseZb
    global(
        Sr,
        c"HW.pBaseZB",
        3,
        7,
        c"48 8B 05 ?? ?? ?? ?? 48 3B 83 98 05 00 00",
    ),
    // Site::RenderImplementation
    global(
        Sr,
        c"RImplementation",
        3,
        7,
        c"48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 45 33 C9 49 89 46 18",
    ),
    // Site::OptionsWord0
    global(
        Sr,
        c"RImplementation.o word 0",
        2,
        6,
        c"8B 05 ?? ?? ?? ?? C1 E8 08 0F B7 C8 FF C9",
    ),
    // Site::OptionsWord3
    global(
        Sr,
        c"RImplementation.o word 3",
        3,
        7,
        c"44 8B 05 ?? ?? ?? ?? 41 8B D0 C1 EA 0C 83 E2 01",
    ),
    // Site::SunshaftsMode
    global(
        Sr,
        c"ps_sunshafts_mode",
        2,
        6,
        c"8B 05 ?? ?? ?? ?? 4D 8B E1 FF C8 44 8B EA",
    ),
    // Site::SamplerCache
    global(
        Sr,
        c"SSManager",
        3,
        7,
        c"48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 33 DB C7 05 ?? ?? ?? ?? 01 00 00 00",
    ),
    // Site::UserMipBias
    global(
        Sr,
        c"ps_r__tf_Mipbias",
        4,
        8,
        c"?? ?? 10 0D ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? 48 83 C4 28 E9 ?? ?? ?? ??",
    ),
    // Site::UserAnisotropy
    global(
        Sr,
        c"ps_r__tf_Anisotropic",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? C7 05 ?? ?? ?? ?? 01 00 00 00 48 89 05 ?? ?? ?? ?? 48 8D 0D ?? ?? \
        ?? ??",
    ),
    // Site::SsaoSetViewport
    called(
        Sr,
        c"set_viewport (r4_rendertarget_phase_ssao.cpp:3)",
        c"E8 ?? ?? ?? ?? 48 8B 83 30 09 00 00 4C 8D 4D 30",
        c"40 53 48 83 EC 40 8B 05 ?? ?? ?? ?? 48 8B D9 65 48 8B 14 25 58 00 00 00 0F 29 74 24 30 \
        0F 28 F2 B9 10 00 02 00 0F 29 7C 24 20 0F 28 F9 48 8B 14 C2 8B 04 11 39 05 ?? ?? ?? ?? 7F \
        28 48 8B 03 4C 8D 05 ?? ?? ?? ?? | 40 53 48 83 EC 40 8B 05 ?? ?? ?? ?? 48 8B D9 65 48 8B \
        14 25 58 00 00 00 C5 F8 29 74 24 30 B9 10 00 02 00 C5 F8 29 7C 24 20 48 8B 14 C2 C5 F8 28 \
        F2 C5 F8 28 F9 8B 04 11 39 05 ?? ?? ?? ?? 7F 2A 48 8B 03 4C 8D 05 ?? ?? ?? ??",
    ),
    // Site::SsaoViewportGuard
    global(
        Sr,
        c"set_viewport static guard",
        2,
        6,
        c"39 05 ?? ?? ?? ?? 7F ?? 48 8B 03 4C 8D 05 ?? ?? ?? ?? BA 01 00 00 00",
    ),
    // Site::SsaoViewport
    global(
        Sr,
        c"set_viewport static viewport",
        14,
        18,
        c"39 05 ?? ?? ?? ?? 7F ?? 48 8B 03 4C 8D 05 ?? ?? ?? ?? BA 01 00 00 00",
    ),
    // Site::CreateShader
    at(
        Motion,
        c"create_shader",
        c"48 89 5C 24 10 48 89 74 24 18 48 89 7C 24 20 55 41 54 41 55 41 56 41 57 48 8D AC 24 B0 \
        FE FF FF",
    ),
    // Site::PhaseSceneBegin
    at(
        Motion,
        c"CRenderTarget::phase_scene_begin",
        c"40 53 48 83 EC 50 8B 15 ?? ?? ?? ?? 48 8B D9 48 8D 0D ?? ?? ?? ??",
    ),
    // Site::PhaseSceneEnd
    at(
        Motion,
        c"CRenderTarget::phase_scene_end",
        c"40 55 57 48 8D 6C 24 B1 48 81 EC B8 00 00 00 48 8B F9",
    ),
    // Site::CacheCall
    at(
        Motion,
        c"shader_compile -> create_shader (cache file, r4.cpp:1867)",
        c"E8 ?? ?? ?? ?? 89 44 24 68 48 8B CE E8 ?? ?? ?? ?? 8B 44 24 68",
    ),
    // Site::CompileCall
    at(
        Motion,
        c"shader_compile -> create_shader (D3DCompile output, r4.cpp:1903)",
        c"E8 ?? ?? ?? ?? E9 ?? ?? ?? ?? 48 83 7B 50 10 4C 8D 43 38",
    ),
    // Site::ShaderCompile
    at(
        Motion,
        c"CRender::shader_compile prologue",
        c"48 89 5C 24 10 44 89 4C 24 20 4C 89 44 24 18 48 89 4C 24 08 55 56 57 41 54 41 55 41 56 \
        41 57 48 8D AC 24 60 EC FF FF B8 A0 14 00 00",
    ),
    // Site::CompileSetup
    at(
        Motion,
        c"CRender::shader_compile D3DCompile setup",
        c"8B 95 F8 13 00 00 48 8D 05 ?? ?? ?? ?? 48 8B 8D F0 13 00 00 4C 8D 8D 50 01 00 00 48 89 \
        85 A0 00 00 00 4C 8D 05 ?? ?? ?? ?? 33 F6 48 8D 45 80 48 89 44 24 50 48 8D 44 24 68 48 89 \
        44 24 48 8B 85 10 14 00 00 89 74 24 40 89 44 24 38 48 8B 85 00 14 00 00 4C 89 6C 24 30 48 \
        89 44 24 28 48 8D 85 A0 00 00 00 48 89 44 24 20 48 89 74 24 68 48 89 75 80 E8 ?? ?? ?? ??",
    ),
    // Site::SourceName
    global(
        Motion,
        c"D3DCompile pSourceName",
        37,
        41,
        c"8B 95 F8 13 00 00 48 8D 05 ?? ?? ?? ?? 48 8B 8D F0 13 00 00 4C 8D 8D 50 01 00 00 48 89 \
        85 A0 00 00 00 4C 8D 05 ?? ?? ?? ?? 33 F6 48 8D 45 80 48 89 44 24 50 48 8D 44 24 68 48 89 \
        44 24 48 8B 85 10 14 00 00 89 74 24 40 89 44 24 38 48 8B 85 00 14 00 00 4C 89 6C 24 30 48 \
        89 44 24 28 48 8D 85 A0 00 00 00 48 89 44 24 20 48 89 74 24 68 48 89 75 80 E8 ?? ?? ?? ??",
    ),
    // Site::TessDispatch
    at(
        Motion,
        c"create_shader hull/domain dispatch",
        c"3C 68 0F B6 85 A8 01 00 00 88 44 24 28 48 8B 85 A0 01 00 00 48 89 44 24 20 75 07 E8 ?? \
        ?? ?? ?? EB 05 E8 ?? ?? ?? ??",
    ),
    // Site::HullCreate
    called(
        Motion,
        c"create_shader<SHS> (r4.cpp:934, hull)",
        c"E8 ?? ?? ?? ?? EB 05 E8 ?? ?? ?? ?? 8B F0 80 BD A8 01 00 00 00",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 30 48 8B FA 41 8B D8 48 8B CF \
        41 8B D0 49 8B E9 E8 ?? ?? ?? ?? 48 8B 74 24 60 4C 8D 4C 24 20 4C 8D 05 ?? ?? ?? ?? 8B D3 \
        48 8B 0E 48 89 41 10",
    ),
    // Site::DomainCreate
    called(
        Motion,
        c"create_shader<SDS> (r4.cpp:934, domain)",
        c"E8 ?? ?? ?? ?? 8B F0 80 BD A8 01 00 00 00",
        c"48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 30 48 8B FA 41 8B D8 48 8B CF \
        41 8B D0 49 8B E9 E8 ?? ?? ?? ?? 48 8B 74 24 60 4C 8D 4C 24 20 4C 8D 05 ?? ?? ?? ?? 8B D3 \
        48 8B 0E 48 89 41 10",
    ),
    // Site::HullWrapper
    at(
        Motion,
        c"CreateHWShader (hull)",
        c"48 83 EC 48 4C 8B D1 48 C7 44 24 60 00 00 00 00 48 8B 0D ?? ?? ?? ?? 4C 8D 44 24 60 4C \
        89 44 24 20 45 33 C9 4C 8B C2 49 8B D2 48 8B 01 FF 90 80 00 00 00",
    ),
    // Site::DomainWrapper
    at(
        Motion,
        c"CreateHWShader (domain)",
        c"48 83 EC 48 4C 8B D1 48 C7 44 24 60 00 00 00 00 48 8B 0D ?? ?? ?? ?? 4C 8D 44 24 60 4C \
        89 44 24 20 45 33 C9 4C 8B C2 49 8B D2 48 8B 01 FF 90 88 00 00 00",
    ),
    // Site::OnFrame
    at(
        Input,
        c"CInput::OnFrame",
        c"48 89 5C 24 08 57 48 83 EC 20 83 3D ?? ?? ?? ?? 00 48 8B F9 48 8B 1D ?? ?? ?? ?? 74 32 \
        FF 83 3C 0A 00 00 80 BB 10 0A 00 00 00 75 23 E8 ?? ?? ?? ?? 48 2B 83 08 0A 00 00 48 89 83 \
        F8 09 00 00 33 C0 48 89 83 20 0A 00 00 48 89 83 28 0A 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? \
        ?? ?? 48 69 C0 E8 03 00 00 33 D2 48 F7 35 ?? ?? ?? ?? 03 05 ?? ?? ?? ?? 48 83 7F 28 00 89 \
        87 EC 0B 00 00 74 08 48 8B CF E8 ?? ?? ?? ?? 48 83 7F 20 00 74 08 48 8B CF E8 ?? ?? ?? ?? \
        83 3D ?? ?? ?? ?? 00 48 8B 1D ?? ?? ?? ?? 74 13 48 8D 8B F8 09 00 00 E8 ?? ?? ?? ?? 48 01 \
        83 30 0A 00 00 48 8B 5C 24 30 48 83 C4 20 5F C3",
    ),
    // Site::KeyUpdate
    at(
        Input,
        c"CInput::KeyUpdate",
        c"4C 8B DC 53 48 81 EC 40 06 00 00 83 3D ?? ?? ?? ?? 00",
    ),
    // Site::MouseUpdate
    at(
        Input,
        c"CInput::MouseUpdate",
        c"4C 8B DC 53 48 81 EC 80 06 00 00 48 8B D9 41 C7 43 08 40 00 00 00",
    ),
    // Site::InputVtable
    global(
        Input,
        c"CInput vtable",
        3,
        7,
        c"48 8D 05 ?? ?? ?? ?? 48 8B F1 48 89 01 48 8B D1",
    ),
];

/// `push rbx; sub rsp,imm8; mov rax,[rcx]`: the entry rmNear, rmFar and rmNormal share.
const VIEWPORT_DEPTH: &CStr = c"40 53 48 83 EC ?? 48 8B 01";

/// The derived sites' feature, name and expected code, indexed from [`Site::RmNear`].
const DERIVED: [(Feature, &CStr, &CStr); SITES - PATTERN_SITES] = [
    (Engine, c"CRender::rmNear", VIEWPORT_DEPTH),
    (Engine, c"CRender::rmFar", VIEWPORT_DEPTH),
    (Engine, c"CRender::rmNormal", VIEWPORT_DEPTH),
    (
        Geometry,
        c"CSkeletonX_PM::Render",
        c"48 83 EC 38 44 8B 89 C8 00 00 00",
    ),
    (Sr, c"position binder vtable", c""),
    (Sr, c"cl_pos_decompress_params::setup", POSITION_BINDER),
    (Sr, c"r_dsgraph_render_hud set_P call", c""),
    (Sr, c"r_dsgraph_render_hud rmNear call", c""),
    (Sr, c"r_dsgraph_render_hud_ui set_P call", c""),
    (Sr, c"r_dsgraph_render_hud_ui rmNear call", c""),
    (Sr, c"r_dsgraph_render_sorted set_P call", c""),
    (Sr, c"r_dsgraph_render_sorted rmNear call", c""),
    (Sr, c"r_dsgraph_render_emissive set_P call", c""),
    (Sr, c"r_dsgraph_render_emissive rmNear call", c""),
    (Sr, c"CParticleEffect::Render set_P call", c""),
    (Sr, c"CParticleEffect::Render rmNear call", c""),
    (Sr, c"CRenderTarget::accum_point set_P call", c""),
    (Sr, c"CRenderTarget::accum_point rmNear call", c""),
];

/// CRender vtable slots 55, 56 and 57 (r4.cpp:834-852; CRender has no other reference to them).
const RM_SLOTS: [(Site, usize); 3] = [(Site::RmNear, 55), (Site::RmFar, 56), (Site::RmNormal, 57)];
/// CSkeletonX_PM's Render is slot 7 of the vtable its constructor stores.
const SKELETON_PM_RENDER_SLOT: usize = 7;
/// m_aVertexConstants is the first of the per-stage tables the flush loop walks; its `lea rbx`
/// addresses the second, 0x70 bytes on.
const VERTEX_CONSTANTS_STRIDE: usize = 0x70;
/// Every first-person site calls set_P and then, within this many bytes, rmNear.
const FIRST_PERSON_SPAN: usize = 0x120;
/// Where in a function its first-person pair is searched.
const FIRST_PERSON_WINDOW: usize = 0x800;
/// `call [rax+1B8h]`: rmNear through CRender vtable slot 55.
const RM_NEAR_VIRTUAL: [u8; 6] = [0xff, 0x90, 0xb8, 0x01, 0x00, 0x00];

/// The name of site `index` (a [`Site`] discriminant).
fn name(index: usize) -> &'static CStr {
    match SIGNATURES.get(index) {
        Some(entry) => entry.name,
        None => DERIVED[index - PATTERN_SITES].1,
    }
}

fn feature(index: usize) -> Feature {
    match SIGNATURES.get(index) {
        Some(entry) => entry.feature,
        None => DERIVED[index - PATTERN_SITES].0,
    }
}

/// The code expected at site `index` resolved to `rva` and the RVA it starts at: its check, or
/// else the finding pattern at its match. `None` for a global or a site without code to match.
fn guard(index: usize, rva: usize) -> Option<(&'static CStr, usize)> {
    let Some(entry) = SIGNATURES.get(index) else {
        let check = DERIVED[index - PATTERN_SITES].2;
        return (!check.is_empty()).then_some((check, rva));
    };
    if !entry.check.is_empty() {
        return Some((entry.check, rva));
    }
    match entry.find {
        Find::At(offset) => Some((entry.pattern, rva.checked_sub(offset)?)),
        Find::Rip { .. } => None,
    }
}

/// Copies image bytes at an RVA.
type Read<'a> = &'a dyn Fn(usize, &mut [u8]) -> bool;

/// Every site's RVA, and per feature the first refusal.
pub(crate) struct Resolved {
    sites: [usize; SITES],
    refused: [Option<String>; FEATURES],
}

impl Resolved {
    fn refuse(&mut self, site: Site, reason: &str) {
        self.refuse_index(site as usize, reason);
    }

    fn refuse_index(&mut self, index: usize, reason: &str) {
        let slot = &mut self.refused[feature(index) as usize];
        if slot.is_none() {
            *slot = Some(format!("site '{}' {reason}", name(index).to_string_lossy()));
        }
    }

    fn ready(&self, site: Site) -> bool {
        self.refused[feature(site as usize) as usize].is_none()
    }

    fn rva(&self, site: Site) -> usize {
        self.sites[site as usize]
    }

    /// The RVA a 64-bit pointer at `rva` holds, relative to `base`.
    fn pointer(read: Read, base: u64, rva: usize) -> Option<usize> {
        let mut value = [0; 8];
        read(rva, &mut value)
            .then(|| u64::from_le_bytes(value).checked_sub(base))
            .flatten()
            .and_then(|rva| usize::try_from(rva).ok())
    }

    /// Resolves `site` from slot `slot` of the vtable at `vtable`.
    fn slot(&mut self, read: Read, base: u64, site: Site, vtable: Site, slot: usize) {
        match Self::pointer(read, base, self.rva(vtable) + slot * 8) {
            Some(rva) => self.sites[site as usize] = rva,
            None => self.refuse(site, "has an unreadable vtable slot"),
        }
    }
}

fn rel32_target(at: usize, bytes: &[u8]) -> Option<usize> {
    let displacement = i32::from_le_bytes(bytes.get(1..5)?.try_into().ok()?);
    (bytes[0] == 0xe8).then(|| (at + 5).checked_add_signed(displacement as isize))?
}

/// The first `call set_P` in the function at `function` followed within [`FIRST_PERSON_SPAN`] by
/// an rmNear call (slot 55 or direct): the first-person projection upload and its depth range.
fn first_person(read: Read, function: usize, set_p: usize, rm_near: usize) -> Option<[usize; 2]> {
    let mut code = vec![0; FIRST_PERSON_WINDOW + FIRST_PERSON_SPAN];
    if !read(function, &mut code) {
        return None;
    }
    let call = |offset: usize| rel32_target(function + offset, &code[offset..]);
    let set_p_call = (0..FIRST_PERSON_WINDOW).find(|&offset| call(offset) == Some(set_p))?;
    let near_call = (set_p_call + 5..set_p_call + FIRST_PERSON_SPAN).find(|&offset| {
        code[offset..].starts_with(&RM_NEAR_VIRTUAL) || call(offset) == Some(rm_near)
    })?;
    Some([function + set_p_call, function + near_call])
}

/// Resolves every site from the code sections, the image base the image's pointers are relocated
/// to, and an RVA reader. A refusal is recorded per feature; the other features still resolve.
pub(crate) fn resolve_image(code: &[Section], base: u64, read: Read) -> Resolved {
    let mut resolved = Resolved {
        sites: [0; SITES],
        refused: Default::default(),
    };
    for feature in [Core, Engine, Geometry, Sr, Motion, Input] {
        let sites = (0..PATTERN_SITES)
            .filter(|&index| SIGNATURES[index].feature == feature)
            .collect::<Vec<_>>();
        let signatures = sites
            .iter()
            .map(|&index| {
                let entry = &SIGNATURES[index];
                Signature {
                    name: entry.name.to_str().unwrap_or("?"),
                    pattern: entry.pattern.to_str().unwrap_or(""),
                    find: entry.find,
                    check: "",
                }
            })
            .collect::<Vec<_>>();
        match signature::resolve(code, &signatures) {
            Ok(found) => {
                for (index, rva) in sites.into_iter().zip(found) {
                    resolved.sites[index] = rva;
                }
            }
            Err(refusal) => resolved.refused[feature as usize] = Some(refusal.to_string()),
        }
    }
    let vertex = Site::VertexConstantBuffers as usize;
    resolved.sites[vertex] = resolved.sites[vertex].wrapping_sub(VERTEX_CONSTANTS_STRIDE);
    if resolved.ready(Site::RendererVtable) {
        for (site, slot) in RM_SLOTS {
            resolved.slot(read, base, site, Site::RendererVtable, slot);
        }
    }
    if resolved.ready(Site::SkeletonPmVtable) {
        resolved.slot(
            read,
            base,
            Site::SkeletonPmRender,
            Site::SkeletonPmVtable,
            SKELETON_PM_RENDER_SLOT,
        );
    }
    if resolved.ready(Site::BinderRegistration) {
        // `lea r8,[binder]` registers the statically constructed binder; its vptr is the vtable.
        let registration = resolved.rva(Site::BinderRegistration);
        let mut lea = [0; 7];
        let binder = read(registration, &mut lea)
            .then(|| i32::from_le_bytes([lea[3], lea[4], lea[5], lea[6]]))
            .and_then(|displacement| (registration + 7).checked_add_signed(displacement as isize));
        match binder.and_then(|binder| Resolved::pointer(read, base, binder)) {
            Some(vtable) => {
                resolved.sites[Site::PositionSetupVtable as usize] = vtable;
                resolved.slot(
                    read,
                    base,
                    Site::PositionSetup,
                    Site::PositionSetupVtable,
                    0,
                );
            }
            None => resolved.refuse(Site::PositionSetupVtable, "has an unreadable binder"),
        }
    }
    if resolved.ready(Site::SetP) && resolved.ready(Site::RmNear) {
        let (set_p, rm_near) = (resolved.rva(Site::SetP), resolved.rva(Site::RmNear));
        for (function, [set_p_call, near_call]) in [
            (Site::RenderHud, [Site::HudSetP, Site::HudNear]),
            (Site::RenderHudUi, [Site::HudUiSetP, Site::HudUiNear]),
            (Site::RenderSorted, [Site::SortedSetP, Site::SortedNear]),
            (
                Site::RenderEmissive,
                [Site::EmissiveSetP, Site::EmissiveNear],
            ),
            (
                Site::ParticleRender,
                [Site::ParticleSetP, Site::ParticleNear],
            ),
            (Site::AccumPoint, [Site::AccumSetP, Site::AccumNear]),
        ] {
            match first_person(read, resolved.rva(function), set_p, rm_near) {
                Some([first, second]) => {
                    resolved.sites[set_p_call as usize] = first;
                    resolved.sites[near_call as usize] = second;
                }
                None => resolved.refuse(set_p_call, "was not found"),
            }
        }
    }
    // Every resolved code site holds its expected code.
    for index in 0..SITES {
        if resolved.refused[feature(index) as usize].is_some() {
            continue;
        }
        if let Some((text, start)) = guard(index, resolved.sites[index])
            && !matches_text(text.to_bytes(), |out| read(start, out))
        {
            resolved.refuse_index(index, "does not hold its expected code");
        }
    }
    // Operands the layout sites fix must address the discovered globals.
    for (site, disp, next, global) in [
        (Site::PhaseCompare, 2, 7, Site::RenderPhase),
        (Site::SvpFrame, 13, 17, Site::DeviceFrame),
    ] {
        if resolved.ready(site) && resolved.ready(global) {
            let mut bytes = [0; 4];
            let rva = resolved.rva(site);
            let target = read(rva + disp, &mut bytes)
                .then(|| (rva + next).checked_add_signed(i32::from_le_bytes(bytes) as isize))
                .flatten();
            if target != Some(resolved.rva(global)) {
                resolved.refuse(site, "does not read its global");
            }
        }
    }
    resolved
}

/// Builder access to the resolved sites: their RVA, name, expected head bytes and guard code.
pub(crate) struct Found<'a> {
    sites: &'a [usize; SITES],
    read: Read<'a>,
}

impl Found<'_> {
    pub(crate) fn rva(&self, site: Site) -> u32 {
        // Anomaly's image is far below 4 GiB.
        self.sites[site as usize] as u32
    }

    pub(crate) fn name(&self, site: Site) -> &'static CStr {
        name(site as usize)
    }

    /// A detour or layout site with its current first 16 bytes.
    pub(crate) fn site(&self, site: Site) -> NativeSite {
        let rva = self.rva(site);
        let mut expected = [0; 16];
        if !(self.read)(rva as usize, &mut expected) {
            // Zero matches no code; the hook preflight refuses the site.
            expected = [0; 16];
        }
        NativeSite {
            name: self.name(site).as_ptr(),
            rva,
            expected,
        }
    }

    /// The site's expected code and where it starts, as a guard the installers match again.
    pub(crate) fn guard(&self, site: Site) -> UniqueGuard {
        let rva = self.sites[site as usize];
        let (text, start) = guard(site as usize, rva).unwrap_or((c"", rva));
        UniqueGuard {
            name: self.name(site).as_ptr(),
            rva: start as u32,
            expected_hex: text.as_ptr(),
        }
    }

    /// The site's expected code.
    pub(crate) fn code(&self, site: Site) -> &'static CStr {
        guard(site as usize, self.sites[site as usize]).map_or(c"", |(text, _)| text)
    }
}

/// The running executable's identity, logged as evidence; none of it gates.
pub(crate) struct Identity {
    pub(crate) executable: &'static CStr,
    pub(crate) sha256: &'static CStr,
    pub(crate) image_size: u32,
}

/// One executable's profile records; a refused record names its feature and site.
pub(crate) struct Records {
    pub(crate) native: Result<&'static NativeProfile, &'static CStr>,
    pub(crate) geometry: Result<&'static GeometryProfile, &'static CStr>,
    pub(crate) sr: Result<&'static SrProfile, &'static CStr>,
    pub(crate) motion: Result<&'static MotionProfile, &'static CStr>,
    pub(crate) input: Result<&'static InputProfile, &'static CStr>,
}

fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

fn leak_text(text: String) -> &'static CStr {
    Box::leak(CString::new(text).unwrap_or_default().into_boxed_c_str())
}

/// Builds the records of the resolved sites; a record is refused when a feature it needs is.
pub(crate) fn records_of(resolved: &Resolved, read: Read, identity: Identity) -> Records {
    let found = Found {
        sites: &resolved.sites,
        read,
    };
    let refusal = |features: &[Feature]| {
        features.iter().find_map(|&feature| {
            let reason = resolved.refused[feature as usize].as_deref()?;
            Some(leak_text(format!(
                "Anomaly {} refused: {reason}.",
                FEATURE_NAMES[feature as usize]
            )))
        })
    };
    let build = |features: &[Feature]| refusal(features).map_or(Ok(()), Err);
    Records {
        native: build(&[Core]).map(|()| {
            leak(crate::native_profile::discovered(
                &found,
                identity.executable,
                identity.sha256,
                identity.image_size,
            ))
        }),
        geometry: build(&[Core, Engine, Geometry])
            .map(|()| leak(crate::geometry_profile::discovered(&found))),
        sr: build(&[Core, Engine, Sr]).map(|()| leak(crate::sr_profile::discovered(&found))),
        motion: build(&[Core, Engine, Sr, Motion])
            .map(|()| leak(crate::motion_profile::discovered(&found))),
        input: build(&[Core, Input]).map(|()| leak(crate::input::discovered(&found))),
    }
}

/// The image's SizeOfImage from its PE32+ header.
#[cfg(not(any(test, feature = "recorded-profiles")))]
fn image_size(read: Read) -> Option<u32> {
    let mut word = [0; 4];
    let nt = read(0x3c, &mut word).then(|| u32::from_le_bytes(word) as usize)?;
    read(nt + 24 + 56, &mut word).then(|| u32::from_le_bytes(word))
}

#[cfg(not(any(test, feature = "recorded-profiles")))]
fn discover() -> Records {
    use crate::engine_guards::guarded_copy;
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::ptr::with_exposed_provenance;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    unsafe extern "C" fn read_live(address: usize, out: *mut c_void, size: usize) -> bool {
        unsafe { guarded_copy(out, with_exposed_provenance(address), size) }
    }
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    let read = |rva: usize, out: &mut [u8]| {
        base.checked_add(rva).is_some_and(|address| unsafe {
            read_live(address, out.as_mut_ptr().cast(), out.len())
        })
    };
    let path = unsafe { rsf_game_sys::win32::module_path(std::ptr::null_mut()) }
        .map(|path| PathBuf::from(OsString::from_wide(path.units())));
    let executable = path
        .as_deref()
        .and_then(|path| path.file_name())
        .map(|name| leak_text(name.to_string_lossy().into_owned()))
        .unwrap_or(c"");
    let sha256 = path
        .as_deref()
        .and_then(rsf_windows::sha256_file_hex)
        .map(|hash| leak_text(String::from_utf8_lossy(&hash[..64]).into_owned()))
        .unwrap_or(c"");
    let code =
        signature::mapped_sections(base, read_live, |_, flags| flags & signature::EXECUTE != 0);
    let (Some(code), Some(image_size)) = (code, image_size(&read)) else {
        let refused = c"Anomaly refused: executable sections unreadable.";
        return Records {
            native: Err(refused),
            geometry: Err(refused),
            sr: Err(refused),
            motion: Err(refused),
            input: Err(refused),
        };
    };
    let resolved = resolve_image(&signature::views(&code), base as u64, &read);
    let identity = Identity {
        executable,
        sha256,
        image_size,
    };
    records_of(&resolved, &read, identity)
}

/// The running executable's records, discovered once per process.
#[cfg(not(any(test, feature = "recorded-profiles")))]
pub(crate) fn records() -> &'static [Records] {
    static DISCOVERY: std::sync::OnceLock<Records> = std::sync::OnceLock::new();
    std::slice::from_ref(DISCOVERY.get_or_init(discover))
}

/// Tests and fixtures run on the recorded Standard (0) and AVX (1) records instead.
#[cfg(any(test, feature = "recorded-profiles"))]
pub(crate) fn records() -> &'static [Records] {
    static RECORDED: std::sync::LazyLock<[Records; 2]> = std::sync::LazyLock::new(|| {
        std::array::from_fn(|build| Records {
            native: Ok(&crate::native_profile::RECORDED[build]),
            geometry: Ok(&crate::geometry_profile::RECORDED[build]),
            sr: Ok(&crate::sr_profile::RECORDED[build]),
            motion: Ok(&crate::motion_profile::RECORDED[build]),
            input: Ok(&crate::input::RECORDED[build]),
        })
    });
    &*RECORDED
}

/// The admitted profile index (the running executable's), or why its native sites refused.
pub(crate) fn admit() -> Result<usize, &'static CStr> {
    match records().first().map(|records| records.native) {
        Some(Ok(_)) => Ok(0),
        Some(Err(reason)) => Err(reason),
        None => Err(c"Anomaly refused: no executable records."),
    }
}

pub(crate) fn native(build: usize) -> Option<&'static NativeProfile> {
    records().get(build)?.native.ok()
}

pub(crate) fn geometry(build: usize) -> Option<&'static GeometryProfile> {
    records().get(build)?.geometry.ok()
}

pub(crate) fn sr(build: usize) -> Option<&'static SrProfile> {
    records().get(build)?.sr.ok()
}

/// Log lines for profile `build`: the executable's identity (evidence only) and every refusal.
pub(crate) fn report(build: usize) -> Vec<String> {
    let Some(records) = records().get(build) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if let Ok(native) = records.native {
        lines.push(format!(
            "Anomaly executable {} (SHA-256 {}, logged only): native sites discovered.",
            native.executable.to_string_lossy(),
            native.sha256.to_string_lossy()
        ));
    }
    for refused in [
        records.native.err(),
        records.geometry.err(),
        records.sr.err(),
        records.motion.err(),
        records.input.err(),
    ]
    .into_iter()
    .flatten()
    {
        let line = refused.to_string_lossy().into_owned();
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines
}

/// The SSE (AnomalyDX11.exe) and AVX (AnomalyDX11AVX.exe) encodings of the position binder body
/// RSF's replacement mirrors, from its entry through the final `ret`.
const POSITION_BINDER: &CStr =
    c"40 57 48 83 EC 30 F3 0F 10 05 ?? ?? ?? ?? 48 8B FA F3 0F 59 05 ?? ?? ?? ?? F3 0F 59 05 ?? \
    ?? ?? ?? F3 0F 5E 05 ?? ?? ?? ?? E8 ?? ?? ?? ?? 0F 28 D0 F3 0F 59 15 ?? ?? ?? ?? 0F 28 CA F3 \
    0F 5E 0D ?? ?? ?? ?? 0F 57 0D ?? ?? ?? ?? 48 85 FF 0F 84 ?? ?? ?? ?? F6 47 10 01 0F 57 C0 8B \
    05 ?? ?? ?? ?? F3 0F 11 4C 24 20 F3 0F 58 C9 F3 0F 11 54 24 24 F3 0F 58 D2 48 89 5C 24 40 BB \
    04 00 00 00 F3 48 0F 2A C0 8B 05 ?? ?? ?? ?? F3 0F 5E C8 0F 57 C0 F3 48 0F 2A C0 F3 0F 11 4C \
    24 28 F3 0F 5E D0 F3 0F 11 54 24 2C 74 56 45 33 C0 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? \
    ?? 44 0F B7 4F 14 44 8B C3 4C 03 48 60 C6 40 68 01 0F B7 4F 16 83 E9 01 74 16 83 E9 01 74 0B \
    83 F9 01 75 0C 44 8D 43 FE EB 06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 8D 54 \
    24 20 E8 ?? ?? ?? ?? F6 47 10 02 74 59 41 B8 01 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? \
    ?? ?? ?? 44 0F B7 4F 18 4C 8B C3 4C 03 48 60 C6 40 68 01 0F B7 4F 1A 83 E9 01 74 16 83 E9 01 \
    74 0B 83 F9 01 75 0C 44 8D 41 01 EB 06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 \
    8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 08 74 59 41 B8 02 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8B D7 \
    E8 ?? ?? ?? ?? 44 0F B7 4F 1C 4C 8B C3 4C 03 48 60 C6 40 68 01 0F B7 4F 1E 83 E9 01 74 16 83 \
    E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 EB 06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B \
    C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 10 74 59 41 B8 03 00 00 00 48 8D 0D ?? ?? ?? ?? 48 \
    8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 20 4C 8B C3 4C 03 48 60 C6 40 68 01 0F B7 4F 22 83 E9 01 74 \
    16 83 E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 EB 06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 \
    49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 20 74 56 44 8B C3 48 8D 0D ?? ?? ?? ?? 48 8B \
    D7 E8 ?? ?? ?? ?? 44 0F B7 4F 24 4C 8B C3 4C 03 48 60 C6 40 68 01 0F B7 4F 26 83 E9 01 74 16 \
    83 E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 EB 06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 \
    8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 40 74 54 41 B8 05 00 00 00 48 8D 0D ?? ?? ?? ?? \
    48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 28 4C 03 48 60 C6 40 68 01 0F B7 4F 2A 83 E9 01 74 14 83 \
    E9 01 74 0A 83 F9 01 75 0A 8D 59 01 EB 05 BB 03 00 00 00 4C 8D 04 9D 00 00 00 00 49 8B C9 48 \
    8D 54 24 20 E8 ?? ?? ?? ?? 48 8B 5C 24 40 48 83 C4 30 5F C3 | 40 57 48 83 EC 30 C5 FA 10 05 ?? \
    ?? ?? ?? C5 FA 59 0D ?? ?? ?? ?? C5 F2 59 15 ?? ?? ?? ?? C5 EA 5E 05 ?? ?? ?? ?? 48 8B FA E8 \
    ?? ?? ?? ?? C5 FA 59 15 ?? ?? ?? ?? C5 EA 5E 0D ?? ?? ?? ?? C5 F0 57 05 ?? ?? ?? ?? 48 85 FF \
    0F 84 ?? ?? ?? ?? F6 47 10 01 8B 05 ?? ?? ?? ?? C5 FA 58 C8 C5 FA 11 44 24 20 C5 F8 57 C0 C4 \
    E1 FA 2A C0 8B 05 ?? ?? ?? ?? C5 F2 5E C8 C5 F8 57 C0 C5 FA 11 54 24 24 C4 E1 FA 2A C0 C5 EA \
    58 D2 C5 FA 11 4C 24 28 C5 EA 5E C8 48 89 5C 24 40 BB 04 00 00 00 C5 FA 11 4C 24 2C 74 56 45 \
    33 C0 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 14 44 8B C3 4C 03 48 60 C6 40 \
    68 01 0F B7 4F 16 83 E9 01 74 16 83 E9 01 74 0B 83 F9 01 75 0C 44 8D 43 FE EB 06 41 B8 03 00 \
    00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 02 74 59 41 B8 \
    01 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 18 4C 8B C3 4C 03 48 60 \
    C6 40 68 01 0F B7 4F 1A 83 E9 01 74 16 83 E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 EB 06 41 B8 \
    03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 08 74 59 \
    41 B8 02 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 1C 4C 8B C3 4C 03 \
    48 60 C6 40 68 01 0F B7 4F 1E 83 E9 01 74 16 83 E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 EB 06 \
    41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 10 \
    74 59 41 B8 03 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 20 4C 8B C3 \
    4C 03 48 60 C6 40 68 01 0F B7 4F 22 83 E9 01 74 16 83 E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 \
    EB 06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 \
    10 20 74 56 44 8B C3 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 24 4C 8B C3 4C \
    03 48 60 C6 40 68 01 0F B7 4F 26 83 E9 01 74 16 83 E9 01 74 0B 83 F9 01 75 0C 44 8D 41 01 EB \
    06 41 B8 03 00 00 00 4E 8D 04 85 00 00 00 00 49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? F6 47 10 \
    40 74 54 41 B8 05 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8B D7 E8 ?? ?? ?? ?? 44 0F B7 4F 28 4C 03 \
    48 60 C6 40 68 01 0F B7 4F 2A 83 E9 01 74 14 83 E9 01 74 0A 83 F9 01 75 0A 8D 59 01 EB 05 BB \
    03 00 00 00 4C 8D 04 9D 00 00 00 00 49 8B C9 48 8D 54 24 20 E8 ?? ?? ?? ?? 48 8B 5C 24 40 48 \
    83 C4 30 5F C3";

#[cfg(test)]
mod tests;
