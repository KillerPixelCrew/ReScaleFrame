# Anomaly geometry draw coverage, draw-time call site and context producers

## Question

A live Observe run on `l01_escape` (`.local/anomaly/wave2-observe/logs/observe-ReScaleFrame.Anomaly.log`)
registered 2654 visuals and retired 265, but identified only 891,728 of 16,104,294 draws (5.5%), with 0 faults.
Which draws bypass the identified visuals, which of those need object motion, and what is missing to feed the
draws that do? Standard 1.5.3 DX11 only (`AnomalyDX11.exe`, SHA-256 `c857b6f4...`), xray-monolith `028ccf5`.

## Why this approach

The log shows counts, not causes. The hooked `Render` overrides are reached through the dsgraph maps, so the
engine source (`r__dsgraph_build.cpp`, `r__dsgraph_render.cpp`, `r4_R_render.cpp`) says which visual enters which
map and with which world matrix. The installed binary then confirms each address, byte guard, global and call
order with `tests/anomaly_geometry_evidence.py --verify`. Nothing here was run in the game.

## What the 5.5% is (source and log)

The log's last line is 8040 render frames: 111 identified and 1892 unidentified hooked `Render` entries per
frame. Identity comes only from `CRender::model_Create/CreateChild/Duplicate`, i.e. models from the model pool
(actors, items, props, HUD hands and weapons). Everything else reaches the same six `Render` overrides without
ever being registered:

| Path | Why it is unidentified | Needs object motion for MVs? |
| --- | --- | --- |
| Static level geometry (`Fvisual`, `FProgressive`) | Loaded by the level loader, not the model pool. `r_dsgraph_insert_static` queues it as `_NormalItem{ssa, visual}`; `r_dsgraph_render_graph` sets `RCache.set_xform_world(Fidentity)` once and `mapNormal_Render` calls `Render`. Landscape (`pLandscape_*`) and sorted/emissive/wallmark statics carry `Fidentity` too. | No. Camera-only motion is exact for a world-space static. |
| Shadow maps, rain, Z-fill | `phase = PHASE_SMAP` (`r2_R_sun.cpp` 446/1019/1363, `r2_R_lights.cpp` 103, `r4_R_rain.cpp` 231, Z-fill `r4_R_render.cpp` 272). Each pass re-runs `render_graph` over the static and dynamic sets, so the static share is multiplied by the number of cascades and shadowed lights. | No. These write no motion. |
| Level trees (`FTreeVisual_ST/PM`) | Level statics, `insert_static`. The wind sway is in the vertex shader. | Yes, but there is no deformation schema yet. A known unsupported gap, never "static". |
| Skinned or moving visuals outside the pool | `CKinematics::m_lod` LOD visuals are not in `children`, so the hierarchy walk never registers them. | Yes. A small real gap, measured as `unidentified_dynamic`. |
| FLOD billboards | `FLOD::Render` resolves to the shared stub `0x2e9770` (also `CParticleGroup`); distant LODs draw through `r_dsgraph_render_lods`. Not a hooked `Render`. | No (static billboards). |
| Particles (`CParticleEffect::Render` `0x8d8420`), grass (`CDetailManager::Render` `0x86a8c0`), wallmarks, sky, UI, deferred light volumes, post-process quads | Own geometry and draw calls. Not a hooked `Render`. | Particles: no per-instance schema, normally a reactive mask. Grass: sway, not supported. Others: no. |
| HUD hands and weapon | Pool models, so identified. They draw from `mapHUD` in `r_dsgraph_render_hud` under the HUD projection. | Yes (first-person domain). |

So the 95% is mostly static level geometry plus the shadow passes that replay it, neither of which needs an
instance identity. The log cannot split the two; the new counters do.

## What was added

All in `games/anomaly/src/{native_geometry,geometry_profile,geometry_feed}.*` plus a two-line change to
`native_observer.cpp`. Every new site is an expected-16-bytes guard installed through `HookSet`, refused on
mismatch before any byte is written.

1. **Classification at visual entry** (no new site). Each hooked `Render` reads `CRender::phase` and, only for an
   unregistered rigid visual outside shadow, the live world matrix, then assigns one verdict:
   `identified`, `ignored_shadow`, `ignored_static_world` (unregistered rigid, world exactly `Fidentity`),
   `unidentified_dynamic` (unregistered skinned, or non-identity world), `unidentified_deformed` (unregistered
   tree), `unidentified_ambiguous` (one visual live under two owners), `unidentified_unreadable`,
   `unidentified_nocapture`. A skinned visual is never static, a tree is never static. The world matrix is used
   only to classify, never as a motion input.
2. **Draw-time call site: `CBackend::constants.flush` `0x84b930`.** `CBackend::Render` is inlined at 128 sites, so
   its D3D `DrawIndexed` call is not hookable. The flush runs once per draw (and compute dispatch), after the SRV,
   RT/ZB, vertex-layout and state applies and immediately before `DrawIndexed` (`FF 50 60`) in `Fvisual`,
   `FProgressive`, `FTreeVisual_ST/PM` and the shared `CSkeletonX::_Render` `0x849aa0` (9 flush sites checked).
   The hook calls `geometry_observe_draw()`: it counts every flush by pass and whether it is inside a hooked
   visual, and for an identified main-pass draw calls `capture_current_draw()` and passes the copy to an optional
   `DrawSink`. `feed_draw()` maps such a capture to `TemporalHistory` on the owner's thread.
3. **Main versus scope view: `CRender::Render` (already hooked by the observer).** `RenderViewScope` opens a
   `DrawContextScope{Main|Scope, World}` for the call, using the engine's own predicate
   `CSecondVPParams::IsSVPFrame` (`isActive && dwFrame % frameDelay == 0`, object `0xd37b30`, frame `0xd37680`).
   A scope frame is a full second `CRender::Render` that ends in `RenderToTarget(rtSVP)` and no Present, so one
   scope per `Render` is the boundary.
4. **First-person projection.** `r_dsgraph_render_hud` `0x84ffa0` is scoped `FirstPerson` for its whole body.
   `r_dsgraph_render_sorted` `0x850cc0` and `r_dsgraph_render_emissive` `0x84ec80` draw a world half and then,
   after `rmNear()` (CRender vtable slot 55, `0x83a450`), a HUD half under the HUD projection; `rmNormal()`
   (slot 57, `0x83a4d0`) ends it. The sorted/emissive hooks mark a window and `rmNear`/`rmNormal` open and close
   a `FirstPerson` scope inside it only; `rmNear` elsewhere (particles, HOM, light volumes) opens nothing.
5. **Counters in the periodic log** (every 120 frames, with the existing geometry line): a summary of render
   entries by pass and verdict, one row per non-empty pass and visual type, and the draw-time line
   (`flush` per pass with `in_visual`, `captured`, `sunk`, `stale`, `no_world`, `no_palette`, `no_context`, view
   counts, `hud_scopes`, `near_windows`).

## Evidence

`py -3 tests/anomaly_geometry_evidence.py --verify` against the installed exe passes. It re-checks all 21
expected-16 guards, the CRender RTTI slots (36-39 lifecycle, 55 `rmNear`, 57 `rmNormal`), the 128 flush callers
and the `DrawIndexed` that follows each flush in the five draw bodies, `CRender::phase` at `0xd64f90` (the
`Fvisual::Render` compare, the two `mov [rsi+0x150]` stores in `CRender::Render`, and `RImplementation` =
`phase - 0x150` with 70 `lea` references), the `IsSVPFrame` shape and its `End` call site,
`psHUD_FOV` `0xd37f78` shared by `render_hud`/`hud_ui`/`sorted`/`emissive`, rmNear before rmNormal in each of
the three HUD functions, `render_sorted` called right after `render_graph(1)` in `render_forward`, and the
viewport depths 0.02 and 1.0.

Not provable from the file: `Fidentity` (`0xd3f870`, the argument of 18 `set_W` callers including
`render_graph`) is filled at startup, so its exact identity contents are a source fact. If it were not
bit-exact, static geometry would show as `unidentified_dynamic` in the next run, which the counters expose.

## Expected coverage per category (hypotheses from source, not measured)

| Category | Expected verdict |
| --- | --- |
| Any hooked type, `pass=shadow` | 100% `ignored_shadow` (includes identified models drawn into shadow maps) |
| `pass=main`, static, progressive | Almost all `ignored_static_world`; a small `unidentified_dynamic` tail (LOD sprites of pool models) |
| `pass=main`, skeleton ST/PM | Mostly `identified` with `FirstPerson` for hands and weapon; a small `unidentified_dynamic` tail |
| `pass=main`, tree ST/PM | `unidentified_deformed` (level trees); any registered tree is `identified` but refused by `feed` as `UnsupportedKind`. With `Motion=1` (foliage identity on) level trees are `identified_foliage`, named by address; see `anomaly-foliage-motion.md` |
| Main-pass flushes outside a visual | Particles, grass, LOD billboards, wallmarks, sky, UI, light volumes, post; not split yet |

## What the next live run must measure

1. `ignored_static_world` share of `pass=main` static/progressive. If it is low and `unidentified_dynamic` is high
   for those types, `Fidentity` is not bit-exact or the classifier is wrong.
2. `pass=shadow` share of all render entries (this is the bulk of the former 95%).
3. `pass=main` `unidentified_dynamic` and `unidentified_deformed` by type: the real gap for motion vectors.
4. Draw-time line: `captured` equals main-pass `identified` entries (one flush per Render); `no_world`,
   `no_palette` and `stale` are 0; `no_context` is 0 for main-pass identified draws once the producers work, and
   `views scope`/`hud_scopes`/`near_windows` are non-zero when a scope is raised and a weapon is drawn.
5. Main-pass `flush_total - flush_in_visual`: if large, attribute it with count-only hooks on
   `r_dsgraph_render_lods` `0x8e2ad0`, `CDetailManager::Render` `0x86a8c0` and `CParticleEffect::Render`
   `0x8d8420`.

Nothing here claims temporal correctness. There is still no history consumer wired to the `DrawSink`, and the
flush site, the view predicate and the first-person windows are verified statically and with fixtures only.
