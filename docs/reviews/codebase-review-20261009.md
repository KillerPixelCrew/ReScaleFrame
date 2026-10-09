# Codebase review, 2026-10-09

Static source review of the orchestrator, graphics, presentation, backends (SR and FG), AC7, Unity Mono
and Project Wingman plugins, overlay (Rust) and the game SDK. Nothing was built, run or edited; no
finding here has been tested in a game. Line numbers refer to the working tree on 2026-10-09
(HEAD `1da33ef` plus uncommitted edits to `native_fg.cpp` and `d3d11_present_bridge.cpp`).

Five passes contributed: reuse, simplification, efficiency and altitude (`/simplify` angles), and a
correctness pass (`/code-review xhigh`, section 7). Findings reported by more than one pass are merged
and tagged with every source: `R` reuse, `S` simplification, `E` efficiency, `A` altitude, `C`
correctness. Confidence is the reviewer's: **H** high, **M** medium.

Nothing has been acted on. Items marked *behaviour* would change runtime behaviour; *ABI* needs an
`RSF_GAME_ABI_VERSION` bump.

## Contents

1. Cross-cutting consolidation
2. Ownership and altitude
3. Hot-path efficiency
4. Dead code and write-only state
5. Per-component simplification
6. Checked and not flagged
7. Correctness findings (`/code-review`)

---

## 1. Cross-cutting consolidation

### 1.1 D3D12 utility header [R S, H]
- Transition barrier, 6 copies: `fsr_generation.cpp:61` (only one that skips before == after),
  `native_fg.cpp:278`, `native_fg_d3d12.cpp:74`, `native_sr_d3d12.cpp:38/89`, `sr_bridge.cpp:62`,
  `d3d11_present_bridge.cpp:55`.
- Copy-with-four-transitions: `native_fg.cpp:663-667`, `sr_bridge.cpp:291-295`,
  `d3d11_present_bridge.cpp:482-486` and `539-543`, `fsr_generation.cpp:92-96`.
- Fence wait (GetCompletedValue / UINT64_MAX / SetEventOnCompletion / 10 s): `sr_bridge.cpp:44`,
  `d3d11_present_bridge.cpp:135`, `native_fg.cpp:268`, near-copy `dlss_generation.cpp:768-770`.
- Allocator + list + Close: `native_fg.cpp:289-298`, `sr_bridge.cpp:136-142`, `d3d11_present_bridge.cpp:372-376`.
- UAV output texture: `native_sr_d3d12.cpp:103` `texture()` exists; re-done at `sr_bridge.cpp:90-101`, `native_fg.cpp:315-322`.
- **Change:** `runtime/backends/common/d3d12_helpers.h` (next to `sr_helpers.h`, which already includes `<d3d12.h>`) with
  `transition`, `copy_transitioned`, `wait_fence`, `create_command_list`, `create_uav_texture`. ~90 lines.

### 1.2 Printf logger `say()` [R S, H]
13 copies of the same 512-byte `vsnprintf` + sink body: graphics `d3d11_observer.cpp:381`, `frame_tap.cpp:381`,
`fullscreen_pass.cpp:132`, `motion_decode.cpp:85`, `overlay_input.cpp:141`, `overlay_renderer.cpp:338`,
`present_blit.cpp:97`, `scene_promote.cpp:102`, `texture_dump.cpp:221`, `ui_layer.cpp:35`; presentation
`shared_surface.cpp:15` (already generic: `say(log_fn, user, fmt, ...)`); `dlss_pipeline.cpp:153`; `dlss_streamline.cpp:95`.
**Change:** move the `shared_surface.cpp` form into one internal header usable by graphics and the DLSS lib. ~140 lines.

### 1.3 HLSL compile boilerplate [R S, H]
`LoadLibraryExW(d3dcompiler_47, SYSTEM32)` + `GetProcAddress(D3DCompile)` + create + free in ~12 files:
`colour_fidelity.cpp:92`, `colour_transport.cpp:75`, `fullscreen_pass.cpp:101/145`, `motion_decode.cpp:66/100`,
`motion_resolve.cpp:62`, `overlay_renderer.cpp:84/300/451-488`, `present_blit.cpp:69/110`,
`native_regions.cpp:46`, `native_translucency.cpp:109`, AC7 `truesky_depth.cpp:40`, `truesky_motion.cpp:64`,
`motion_capture.cpp` (see 3.26).
- Compile flags already differ (STRICTNESS vs 0), as does the "release blobs before FreeLibrary" step.
- The orchestrator links the `d3dcompiler` import lib (`runtime/orchestrator/CMakeLists.txt:40`) and
  `native_sr_d3d12.cpp:126` calls `D3DCompile` directly, while `native_regions`/`native_translucency` in the
  same target load it dynamically.
- **Change:** one `rsf_compile_shader(device, src, entry, profile, log)` in `runtime/graphics`; AC7 render_scope
  does not link Graphics, so header-only or add the link. ~150 lines.

### 1.4 `present_blit.cpp` is a subset of `fullscreen_pass.cpp` [R S, H/M]
Shader (`present_blit.cpp:20-55` vs `fullscreen_pass.cpp:20-90`), `SavedState`/save/restore (133-221 vs 168-244),
`load_compiler`, `compile_one` are copies; `fullscreen_pass.h` says it was "generalised from the blit".
**Change:** keep the `rsf_present_blit_*` ABI (used by `loader/proxy/src/dlss_bridge.c`) as a thin wrapper over
`rsf_fullscreen_pass_draw(TONEMAP|COPY)` (`fullscreen_pass.h:107`). ~300-350 lines.

### 1.5 Hand-rolled pipeline state save/restore [R S, H]
`fullscreen_pass.cpp:168-244`, `present_blit.cpp:133-221`, `overlay_renderer.cpp:114-290` re-implement
`rsf_d3d11_state_save/restore` (`d3d11_state.h:31-38`, used by `colour_fidelity`, `colour_transport`,
`motion_resolve`, `native_sr`, `native_translucency`). Coverage already differs (overlay saves class instances
and HS/DS/GS; blit omits scissors). Caveat: `overlay_renderer` deliberately never touches render targets
(comment 159-164); check against `d3d11_state`'s OM/UAV handling first. ~340 lines. See also 3.10.

### 1.6 SRV / typed view format mapping [R A, H]
- Depth to SRV: `native_regions.cpp:36`, `colour_fidelity.cpp:160-163`, `motion_resolve.cpp:111-113` (no `D*`
  aliases, no R16), `native_sr_d3d12.cpp:240-244` (maps every unknown to R32_FLOAT).
- Typeless colour to typed: `colour_transport.cpp:88` (2 cases, no R8G8B8A8 / R10G10B10A2),
  `native_translucency.cpp:57` (4), `scene_promote.cpp:153-160` (5), `native_sr.cpp:56-59` (4).
- The same input therefore passes one pass and fails another.
- **Change:** one `rsf_srv_format(DXGI_FORMAT)` in `runtime/graphics`, next to `is_depth_format` (`resource_roles.cpp:19`).

### 1.7 Bytes-per-pixel table [R, H]
`texture_dump.cpp:186` `bytes_per_pixel` vs `texture_dump_raw.cpp:10` `pixel_bytes` (superset). The PPM dumper
rejects formats the raw dumper accepts. Use `pixel_bytes`.

### 1.8 vtable `patch_slot` [R S, H]
Byte-identical in `d3d11_observer.cpp:127` and `frame_tap.cpp:394`. One internal graphics helper.

### 1.9 Constant-buffer readback [R, H]
`d3d11_observer.cpp:525-555` re-implements `rsf_read_constant_buffer` (`constant_buffer_read.cpp:11`, used by
AC7 `motion_capture.cpp:566`). The observer copies the source desc for staging; the helper deliberately builds
it from scratch (`constant_buffer_read.cpp:62-64` explains why).

### 1.10 Game-plugin boilerplate [R S A, H]
`validate<T>` (struct_size + abi_version), `ascii_lower`, `equal_ascii`, name+SHA `detect`, API preamble in
`games/ac7/src/plugin.cpp:16/127/147`, `games/project-wingman/src/plugin.cpp:13/67/87`,
`games/unity-mono/src/plugin.cpp:36/115/135`. Wingman's `equal_ascii` already lost the `!expected` null check.
**Change:** header-only, MIT, self-contained helper in `sdk/game/include/rescaleframe/` (precedent:
`game_frame.h:184` ships `static inline` helpers). ~35 lines per plugin.

### 1.11 SHA-256 file hashing [R S, M/H]
BCrypt loop with 64 KiB buffer and manual handle cleanup: `fsr4_compat.cpp:26-50`, `unity_sr_host.cpp:83-109`,
`games/unity-mono/src/bridge.cpp:54-72`, `loader/proxy/src/dinput8_proxy.c:46`. The two runtime copies can share
an RAII `rsf::sha256_file`; the plugin and loader copies need a small C header to share.

### 1.12 DLSS / Streamline loading and constants [R S, M/H]
- Interposer loading three ways: `dlss_generation.cpp:676-687` re-implements `rsf::load_runtime`
  (`sr_helpers.h:11`) to fit a signature check between path and load; `dlss_native12.cpp` has its own
  `widen` (75/76), `entry` template (41, duplicates `rsf::entry` at `sr_helpers.h:35`) and `DEFAULT_DIRS` (93);
  `dlss_streamline.cpp` has `widen` (123), `resolve` (140) and plain `LoadLibraryW` (249). Three DLL search
  policies. **Change:** split `load_runtime` into `runtime_path()` + `load()` and add one
  `load_interposer(path, require_signature, log)`.
- Signature check: `dlss_streamline.cpp:236-245` inlines `rsf_dlss_verify_runtime_signature` (same file, 35);
  `dlss_native12.cpp:18/84-86` forward-declares and calls `sl::security::verifyEmbeddedSignature` directly,
  bypassing the `RSF_HAVE_SIGNATURE_CHECK` guard.
- Two hand-written resolve chains for the same `Entries` struct: `dlss_streamline.cpp:256-274`, `638-644`.
- Row-major matrix copy 3x: `dlss_streamline.cpp:150`, lambda `dlss_generation.cpp:592`, `dlss_native12.cpp:56`.
  One copy in `streamline_frame.h`.
- Quality to `DLSSMode` 2x: `dlss_native12.cpp:46`, `dlss_streamline.cpp:170`.
- Two `sl::Constants` builders for the same camera record: `dlss_generation.cpp:591-608` (FG) and
  `dlss_native12.cpp:181-205` (SR). They differ: `cameraFwd` negated only in SR; `cameraAspectRatio` from the
  projection in FG and output size in SR; `prevClipToClip` inverted with `sl::matrixFullInvert` vs
  `XMMatrixInverse`. Some differences may be intentional; worth one shared filler with explicit overrides. [M]
- `nvapi_QueryInterface` lookup duplicated: `dlss_generation.cpp:83`, `reflex_audit.cpp:137`.
- UTF-8 widen also at `dlss_generation.cpp:676`.

### 1.13 SR frame validation [R, M]
`dlss_native12.cpp:165-168` hand-validates `rsf_sr_frame` instead of `rsf::validate_frame` /
`rsf::validate_d3d12_resources` (`sr_helpers.h:41/82`) used by FSR (`fsr_sr.inl:135`) and XeSS
(`xess_sr.inl:109`). `native_sr_d3d12.cpp:285` sends the same frame to either, so backends accept different inputs.
Related: `fg_helpers.h` writes the "owned by device" check 4 times (`fg_setup`, `fg_command`, `fg_frame`,
`sr_helpers.h validate_d3d12_resources`) and six `isfinite` loops in `fg_frame` → `owned_by()`, `all_finite()`. [S, H]

### 1.14 Shared FSR/XeSS SR helpers [S, M/H]
- FFX version selection loop duplicated: `fsr_sr.inl:60`, `fsr_generation.cpp:158` → `select_ffx_version()`. [H]
- `probe()` / `say()` near-identical in `fsr_backend.cpp` and `xess_backend.cpp`; `sr_release()` and
  `sr_version()` identical in `fsr_sr.inl` and `xess_sr.inl`. [M]

### 1.15 FG provider stubs and `status()` [R S, H]
- "Not compiled" generation providers identical in `dlss_generation.cpp:810-828`, `fsr_generation.cpp:383-402`,
  `xess_generation.cpp:250-268`.
- `status()` identical in `dlss_generation.cpp:638`, `fsr_generation.cpp:360`, `xess_generation.cpp:231`.
- **Change:** templates in `runtime/backends/common/fg_helpers.h`. ~60 lines. (Legacy FG stubs: see 4.4.)

### 1.16 Unity configuration in the orchestrator [R S, M/H]
- Two `[UnitySR]` path resolvers: `native_fg_d3d12.cpp:305` `path_setting` and `unity_sr_host.cpp:44-45`
  `setting`. Only the second canonicalises via `GetFullPathNameW`; absolute-path tests differ, so a relative
  `Streamline=` resolves differently for SR and FG.
- `preferences.ini` path + `rsf_fg_choice_start` with the same 4-backend mask: `native_fg_d3d12.cpp:338-341`,
  `unity_sr_host.cpp:323-328`.
- UTF-16 to UTF-8 three times: lambda `native_fg_d3d12.cpp:363`, inline 344-346 in the same function,
  `unity_sr_host.cpp:59` `utf8()`.
- **Change:** one small Unity-config unit in the orchestrator. ~40 lines.

### 1.17 MinHook install-and-rollback loop [R, M]
`native_renderer.cpp:2489`, `motion_capture.cpp:517` and `1023`, `reflex_audit.cpp:148`,
`d3d11_present_bridge.cpp:717`. Rollback differs (some call `MH_RemoveHook` after disable). Three copies inside
AC7 justify an AC7-internal `install_hooks(targets, detours, originals, n)`.

### 1.18 SEH-guarded memory reads in AC7 [R S, H]
`native_renderer.cpp:102` `copy` + `read<T>` and `motion_capture.cpp:166` `copy_memory` + `read<T>` are identical
including the non-MSVC `VirtualQuery` fallback; signature drift forces ~50 casts in `motion_capture`. One header
in `games/ac7/include`.

### 1.19 FG transfer slot vs SR bridge staging [R, M]
`native_fg.cpp:283-325` re-does `sr_bridge.cpp:74-104, 133-145`: shared colour/depth/motion/exposure surfaces,
UAV output, shared fence, per-slot lists. Colour format differs (source format vs fixed RGBA16F); exposure and
hints differ. The slot ring is legitimately extra; the surface-set allocation could be shared.

---

## 2. Ownership and altitude

### 2.1 Second FG screen whitelist bypasses `rsf_frame_allows_fg()` [A, H] *behaviour*
`runtime/backends/common/fg_helpers.h:85`, `fg_session.cpp:166`, `native_fg.cpp:44` (used 149, 620, 722),
`native_fg_d3d12.cpp:204` hardcode AC7's screen list on top of `rsf_frame_allows_fg()` (`game_frame.h`), which
exists so the answer is the same everywhere. The copies disagree: `native_fg` omits `RSF_SCREEN_REPLAY`, the
backend helper and `fg_session` allow it. Unity fakes `record.screen = RSF_SCREEN_FLIGHT` to pass the gate.
**Change:** use only `rsf_frame_allows_fg()`; the plugin decides via screen class or `RSF_FRAME_FLAG_NO_FG`.

### 2.2 Game identity hardcoded in the presentation bridge [A, H]
`d3d11_present_bridge.cpp:196-198, 322, 338`, `unity_sr_host.cpp:255`, `loader/proxy/src/dlss_bridge.c:3694`:
Streamline engine type, version ("4.18.3", "6000.3") and project GUIDs are literals; the depth/units fallback
(`native12 ? 1.0f : 0.01f`, infinite depth) uses "is D3D12" as a stand-in for "is Unity". The bridge header says
no game-specific policy lives there. Versions already drifted: SR sends AC7 as "4.18", FG as "4.18.3".
**Change:** engine type, version, project id in `rsf_d3d11_present_setup` from plugin / `engine.json`; require
`units_to_meters > 0` instead of guessing.

### 2.3 Vendor retirement join written twice, on different queues [A, H] (uncommitted)
`native_fg.cpp:842-875` uses `rsf_d3d11_present_queue()`; `native_fg_d3d12.cpp:256-284` uses
`rsf_d3d11_present_graphics().queue`. Both: query retirement fence, queue Wait, lazily create private fence,
Signal. **Change:** one bridge export, e.g. `rsf_d3d11_present_retire(provider, list_or_null, &fence, &value)`.
Also `native_fg_d3d12.cpp:264-278` repeats its 3-statement failure exit 3 times. [S, M]

### 2.4 `rsf_d3d11_present_queue()` recomputes instead of recording; cold-start path duplicates `generation_setup()` [A S, H]
`d3d11_present_bridge.cpp:302` (new) rebuilds `generation_setup()` to read one field. The non-switching cold start
(383-408) has an inline copy that differs: it uses `graphics.queue` (Streamline proxy) for every backend where
`generation_setup()` uses the native queue for non-DLSS, and forces `ui_mode = NONE` where `generation_setup()`
uses `settings.ui_mode`. With FSR/XeSS on NVIDIA without runtime switching, the accessor can return a queue the
provider was not created on. **Change:** cold start calls `create_physical(settings.backend)`; store the queue the
provider was created with and return that.

### 2.5 Streamline command-list unwrap at each call site [A S, H]
`host && backend != DLSS ? rsf_streamline_host_native(host, list) : list` at `d3d11_present_bridge.cpp:488, 544`,
`native_fg.cpp:864`; the device/queue half lives in `generation_setup` (200). One bridge helper (or fold into 2.3).

### 2.6 Backend-specific resource-state policy in the orchestrator [A, M/H] *behaviour*
`native_fg.cpp:809` sets `read_state` only for FSR3/FSR4, carrying `temporal_read`/`hudless_read` to retire;
DLSS and XeSS get COMMON. The Unity path (`native_fg_d3d12.cpp:213-228`) moves every backend's inputs to the read
state. **Change:** always declare the read state and restore at retire (the retire list now exists).

### 2.7 `record.frame_time_ms` meaning depends on the backend [A, M] *behaviour*
`native_fg.cpp:781` rewrites it from CPU simulation time to source-frame delta for non-DLSS providers. Pick one
meaning (source delta, already in `camera.frame_time_seconds`); a provider that wants something else derives it.

### 2.8 Backend list and SDK directory mapping re-spelled [A S, H/M]
- Validity chain `!= DLSS && != FSR3 && != FSR4 && != XESS` and the choices mask: `fg_choice.cpp:11`,
  `native_fg_d3d12.cpp:317, 341-342`, `d3d11_present_bridge.cpp:712-713, 743-744`, `unity_sr_host.cpp:327`,
  `dinput8_proxy.c:873`. `backend_api()` falls back to DLSS for unknown ids, so a missed site fails quietly.
  **Change:** `rsf_fg_backend_known(id)` and `RSF_FG_BACKEND_ALL` in `frame_generation.h`.
- Backend to SDK directory / ini key / env var as nested ternaries: `d3d11_present_bridge.cpp:194`,
  `native_fg_d3d12.cpp:343`, `dinput8_proxy.c:874-877`. Already differ (proxy merges FSR3/FSR4 into one
  fidelityfx folder; Unity ini keeps them apart). One `{id, ini key, env var, default subdir}` table.

### 2.9 "NVIDIA means Streamline" checked by callers [A, M/H] *behaviour*
`d3d11_present_bridge.cpp:320, 335` (335 is new) test `VendorId == 0x10de`; on D3D11 a refused host is fatal
(`DXGI_ERROR_UNSUPPORTED`) even with FSR/XeSS selected. `gpu_policy.cpp` encodes the rule a third time.
**Change:** `create_host` in `dlss_generation.cpp` refuses non-NVIDIA with `NOT_SUPPORTED`; the bridge treats that
as "no host" unless DLSS was strictly requested.

### 2.10 Latency dispatch keyed on backend id [A S, M]
`d3d11_present_bridge.cpp:749-785` (begin, marker, abort) branch on `active_backend == DLSS`, though the DLSS
provider already forwards `begin_frame`/`marker`/`abort_frame` to the host (`dlss_generation.cpp:349-357`).
`marker` reaches the host for every backend, `begin` only for DLSS; the test needs `sleeps_expected` bookkeeping
to follow it. Five exports repeat the try-shared-lock + provider guard. **Change:** dispatch on
`status.pacing_owner` once, or call the provider unconditionally and keep only host-only extras.
`native_fg.cpp:64-72` `frame_acquire/marker/abort` ignore `host`; the prepare callback's `host` parameter is
ignored by both consumers (`native_fg.cpp:825`, `native_fg_d3d12.cpp:157`). [S, H/M]

### 2.11 Private bit in a public ABI field [A, H] *ABI*
`games/unity-mono/src/bridge.cpp:220` sets `pass.flags |= 0x80000000u` ("configuration changing"), read at
`unity_sr_host.cpp:203`; `game_renderer.h` does not define it. Define `RSF_GAME_RENDER_RECONFIGURE` (append) or
carry it on the Unity host interface. Related: Unity packet flag bits are unnamed literals across C# and C++. [S, H]

### 2.12 SR backend ids 1, 6, 7 as bare numbers [A, H]
`sr_session.h` names only NONE and FSR2..XeSS. Literals in `dlss_pipeline.cpp:723, 748, 758, 764, 781, 783, 785,
1072, 1172, 1249, 1276, 1309`, `native_sr.cpp:134`, `unity_sr_host.cpp:126, 135, 148, 190, 236, 330, 334`,
`gpu_policy.cpp:6-8`, `UrpAdapter.cs:152, 154, 175, 246, 314` (managed code branches on `Backend == 6`),
`panel.rs:160-178` (FSR1 offered unconditionally outside the loop). Name tables duplicated
(`unity_sr_host.cpp:126`, `panel.rs`). **Change:** named constants in the contract header; a semantic Unity field
("engine spatial, no jitter"); drive the overlay SR list from a choices mask like FG.

### 2.13 DLSS-only input conditioning in the generic SR pipeline [A, M] *behaviour*
`dlss_pipeline.cpp:723` (dense motion resolve), 748 (colour correction), 764 (colour transport), 783 (FG capture)
run only when `backend == 1`; others get `zero_means_unwritten` via `rsf_sr_legacy_evaluate`. The sentinel-motion
and banding fixes reach only DLSS, against "fill every vendor input". Express each as a declared backend need.

### 2.14 Motion units converted in the producer keyed on the consumer [A, M/H]
`native_sr.cpp:134-138` scales UV motion by (2, -2) to NDC for non-DLSS; `sr_legacy_adapter.cpp:79-80` then
scales by (-0.5, 0.5). The producer queries pipeline status every frame to learn the consumer. Fix one unit
(canonical UV) at the `rsf_dlss_pipeline` boundary and convert only in the legacy adapter.

### 2.15 Two FG capture entry points chosen by the caller [A, M]
DLSS evaluate fills the FG transfer slot as a side effect; other backends call `rsf_native_fg_capture`
(`native_fg.cpp:584/604`); `dlss_pipeline.cpp:783` chooses with `(!host || backend != 1)`. Make capture idempotent
per frame and call it unconditionally.

### 2.16 AC7 capture rules in `runtime/graphics` [A, M]
`resource_roles.cpp:11-155` encodes AC7 role rules (1x1 R32G32 exposure, RT R16G16_UNORM motion, R10G10B10A2
normals); `frame_tap.cpp:1700-1720` has its own scene-colour list despite the comment that all format rules live
in `resource_roles.cpp`. Already drifted: classifier accepts only RGBA16F, `frame_tap` also R11G11B10. Pass a
role table from the plugin through frame-tap options. Legacy loader path only.

### 2.17 FG slot lookup repeated; dead guard after uncommitted change [A S, M/H]
`slots[id % 6]` with a literal plus per-site identity checks at `native_fg.cpp:560, 587, 612, 707`; identity
checks already differ between `final` and `prepare`. The lock + `cpu_frames[id % size]` + `id ==` pattern appears
~12 times; logger-under-lock fetch 5 times (39, 137, 218, 349, 836); signal+flush pair repeated (579-582,
596-598). The uncommitted change added `hudless_ready` to `inputs`, so `if (selected->hudless_ready)` at 802 is
always true and `cached.reason` reports a missing HUD-less frame as generic code 7. **Change:** `slot_for(pass)`
with `std::size(slots)`, `with_frame(id, f)`, and a dedicated reason code.

---

## 3. Hot-path efficiency

### A. Per-draw and per-bind hooks (AC7 D3D11 frame tap, AC7 native renderer)

**3.1** `frame_tap.cpp:2567-2586` (+ `sampler_original` 2538, `biased_sampler` 2547) [H]: every PSSetSamplers takes
`sampler_guard` and linearly scans `biased_samplers` per sampler, even at zero bias; the vector grows with every
distinct sampler bound while biased. → Two pointer-keyed hash maps; forward directly at bias 0 with no clone
bound; atomic generation instead of the lock.

**3.2** `frame_tap.cpp:2311-2342` `watch_update`, `2166-2201` `hooked_map`, armed "Always" at
`loader/proxy/src/dlss_bridge.c:4005` [H]: every CB UpdateSubresource / Map(WRITE_DISCARD) pays GetType, GetDesc,
pending-map scan, up to 4 KB stack memcpy and a callback that in native-owned mode only forwards to
`rsf_ac7_motion_capture_upload`, which returns unless `RSF_MOTION_CAPTURE`. → Disarm when native-owned and
capture off; copy only when the watcher may modify (`patch_input_sizes`); cache the per-buffer verdict.

**3.3** `frame_tap.cpp:2588-2650` `hooked_ps_set_constant_buffers` [H]: GetDesc per bound buffer per bind; size match
takes AddRef + `self.guard` + Release even for the same `view_constants`. Only consumer `consider_bound_set`
returns early when `on_pass` is null (native-owned). → Gate on `options.on_pass`; compare before locking; skip
GetDesc when slot unchanged.

**3.4** `frame_tap.cpp:1346-1377` SRV shadow [M]: each changed slot pays GetResource, QI, Release, GetDesc and a held
ref; in native-owned mode it is kept only for captures/candidates. → Store the view pointer; resolve lazily.

**3.5** `frame_tap.cpp:1313-1335`, `plan_render_targets` ~1388, `hooked_clear_render_target_view` 1610 [M]: with a
plan active, every view in every SRV/RTV bind and clear does GetResource + QI + Release, then the shadow repeats
it. → View-to-entry cache; reuse the shadow's texture. Compatibility renderer only.

**3.6** `frame_tap.cpp:664-677` `unbind_target_reads` [H]: every OMSetRenderTargets(+UAV) walks 128 slots x 64 B;
`consider_target_draw` repeats it when `input_watch_dirty`. → Occupancy bitmask / high-water mark.

**3.7** `native_renderer.cpp:1341-1354` `hooked_rhi_pixel_uniform`, `1355-1398` `hooked_rhi_pixel_tables`,
`1399-1435` `hooked_pixel_enqueue<40..43>` [H]: "Evidence only" hooks installed unconditionally for the whole
session. Per pixel UB bind: `GetModuleHandleW(nullptr)`, guarded read, `pixel_bindings_guard`, ring write; tables
hook does up to 14 guarded reads per draw. → Gate behind a diagnostic option; at least cache module base and make
the ring single-writer lock-free.

**3.8** `native_renderer.cpp:98-101` `EntryGuard` / `OuterGuard` [M]: seq_cst fetch_add/sub on one global atomic from
game, render and RHI threads on every hook call (cache-line bouncing). → Per-thread counters summed at quiesce,
or relaxed ops with one fence in the stop path.

**3.9** `native_renderer.cpp:1991-2009` `hooked_slate_texture` [M]: every Slate element batch (hundreds/frame) takes
`identities_guard` to compare with `final_surfaces`; same mutex as game/render-thread identity hooks. → Atomic or
seqlock snapshot of the key, or pointer prefilter before locking.

### B. Per-frame GPU pass setup

**3.10** `d3d11_state.cpp:134-160` `rsf_d3d11_state_save` [H]: each call heap-allocates a zeroed ~22 KB `State`, QIs
`ID3D11DeviceContext1`, GetDevice, GetFeatureLevel, CheckFeatureSupport, and Gets ~6 stages x (128 SRV + 16
sampler + 14 CB) with AddRef/Release. Up to six per frame nested inside `native_sr.cpp:149` `Bindings`, which
already saved everything: `colour_transport.cpp:126` (encode + decode), `motion_resolve.cpp:135`,
`colour_fidelity.cpp:167`, `native_translucency.cpp:176`. → Skip nested saves under an outer scope
(thread-local), save only touched stages (CS-only passes), reuse a buffer, cache context1/feature query.

**3.11** SRVs created and destroyed per frame on stable textures [H]: `colour_transport.cpp:100,112` (5),
`colour_fidelity.cpp:164-166` (3), `motion_decode.cpp:247` (1), `motion_resolve.cpp:115,125` (2-3),
`native_regions.cpp:99,101` (2), `native_translucency.cpp:160` (2-3 per dispatch x2, one on its own
`self.opaque`). → (texture, format)-keyed cache holding the texture ref, as `scalar_exposure` in
`native_sr.cpp:77-85` does; views on owned textures made once at allocation.

**3.12** `dlss_streamline.cpp:551` [M]: `slDLSSSetOptions` every frame with identical options. → memcmp last
`DLSSOptions` per viewport.

**3.13** Cross-API Flushes [M]: `native_fg.cpp:582` (final), 598 (capture), 658, 685 (evaluate); `sr_bridge.cpp:233,
285`. The trailing Flush after evaluate/final follows a Signal that only a CPU slot-reuse wait 3-6 frames later
needs; `d3d11_present_bridge.cpp:533` Signal + Flush in the same frame already submits it. → Keep the Flush before
`queue->Wait`, drop or defer the trailing ones. Verify ordering before removing any.

**3.14** Per-frame revalidation [M, small]: `sr_bridge.cpp:181-195`, `native_fg.cpp:563, 629`,
`native_sr_d3d12.cpp:150-158, 210`, `fg_helpers.h:62-110` QI `ID3D11DeviceContext4`, GetDevice per input, GetDesc
every frame. → Validate once per (resource, generation).

**3.15** `d3d11_present_bridge.cpp:452, 479/537` [M]: `GetFullscreenState` each present when sync is 0;
`GetBuffer` (QI + ref) each present. → Cache fullscreen state on SetFullscreenState/ResizeBuffers; back-buffer
array refreshed on resize.

**3.16** `d3d11_observer.cpp:586-660, 673-735` [M, low]: per present a GetDevice QI for API detection and 3-4
acquisitions of `self.guard`. → Atomics for `present_width`, `frames`, dump-pending; cache D3D11 verdict per
swap chain.

### C. Unity

**3.17** `overlay_d3d12.cpp:35-48` [H]: every present does `CreateWrappedResource`, `CreateRenderTargetView`,
Acquire/Release and `context11->Flush()` before `rsf_overlay_host_draw_target` returns 0 (most of gameplay), and
inside `unity_sr_host.cpp:120` holding `self.state`, the mutex every managed `Native.Configuration` call takes.
→ Skip when hidden with no HUD; cache wrapped resource + RTV per back-buffer index, drop on resize.

**3.18** `UrpAdapter.cs` reflection on hot paths [H]: `updateResolution.Invoke(__instance, new object[]{...})`
allocates and boxes `Vector2Int` per frame; `hdrOutputProperty.GetValue` boxes a bool; `rawProjection.GetValue`
boxes a Matrix4x4 up to 3x per frame from `Snapshot`; `antialiasing.SetValue(..., TemporalAntiAliasing)` boxes;
`frameDataProperty.GetValue` per camera per frame in `InputScope` and `Overlay`. → Typed delegates via
`AccessTools.MethodDelegate` / `FieldRefAccess<UniversalCameraData,T>`, pre-boxed enum.

**3.19** `Bootstrap.cs:26-34` `EnterProducer` [H]: allocates a `Producer` class and takes `lock(producerGate)` 10-15
times per frame. → `struct Producer : IDisposable` and `Interlocked`.

**3.20** `UrpAdapter.cs` `builder.SetRenderFunc<PassData>(Execute)` in `Hudless` and `Record` [H]: new delegate per
pass per frame. → `static readonly` cached delegate.

**3.21** `UrpAdapter.cs`, `Native.cs:81-86`, `CpuBoundaries.cs` [M]: `Native.Configuration(camera)` (P/Invoke +
`Marshal.SizeOf` + native mutex) and `Eligible` run 5-8 times per frame per camera; `Marshal.SizeOf<Packet/
CameraFrame>()` each `Snapshot`. → Cache per `Time.frameCount`; sizes in `static readonly`.

### D. Overlay and input

**3.22** `overlay_input.cpp:874-883`, `note_raw_function_key` 680, `record_raw_mouse` 212 [H dup / M hidden]: every
WM_INPUT (1-8 kHz with a gaming mouse, overlay hidden too) calls `GetRawInputData` once for F-key detection;
visible overlay reads the same handle twice more. → Read RAWINPUT once and pass it down. See also 4.5 (the
function-key route has no consumer).

**3.23** `ui/overlay/src/overlay.rs:133-163`, `panel.rs:507-540` [M]: with the perf HUD on, egui does full layout +
tessellation every frame and builds 3-4 `format!` strings, though rates change every 0.5 s. → Cache strings keyed
on rates/FG state; reuse last frame's geometry when inputs and display size are unchanged.

**3.24** `overlay_renderer.cpp:355-381` `report_target_once` [H, low]: `reported_srgb_target` is never set for a
non-sRGB target, so OMGetRenderTargets + GetDesc + Release run every visible frame. → Mark checked after the
first non-null target, or recheck on target change.

### E. Startup and diagnostics

**3.25** `d3d11_observer.cpp:852-858` `genuine_slot`, 1045 `install_present_route` [H, startup]: dxgi.dll read whole
from disk up to four times; the file is read before the cheap in-module early-out. → Check first; read once.

**3.26** Diagnostic-only paths [M]:
- `motion_capture.cpp:593-613, 1044`: with `RSF_MOTION_CAPTURE`, each shader creation does LoadLibraryExW/
  FreeLibrary of d3dcompiler (loader lock) and D3DDisassemble. Load once.
- `motion_capture.cpp:1066-1073`: in that mode every CB upload is copied into `unordered_map<void*, vector>`
  under a mutex even when no capture is active. Gate on `s.active` / armed interval.
- `native_fg_d3d12.cpp:79-103`: two full-resolution PPMs written synchronously in `prepare()` while holding
  `guard` on the present path. Move to a worker.

---

## 4. Dead code and write-only state

All "unused" claims were grep-checked across runtime, games, loader, sdk, ui and tests including `.inl`.
Correction during review: `rsf_fsr4_enable_int8` is used (`fsr_sr.inl:38`).

**4.1 DLSS preset plumbing [S, H]**: `rsf_dlss_pipeline_select_preset` / `_get_preset` (`dlss_pipeline.h:322-323`)
declared, never defined or called. `Pipeline::dlss_preset` (`dlss_pipeline.cpp:110, 758`) is never written, so
every frame calls `rsf_dlss_set_preset(AUTO)`, its only caller; `rsf_dlss_set_preset`, `State::preset` and
`dlss_streamline.cpp:546-550` then carry nothing. (The altitude pass confirmed the preset stays AUTO, so the
"never force a preset" rule is respected; the plumbing is simply dead.)

**4.2 DLSS pipeline / Streamline [S, H]**: `rsf_dlss_pipeline_layer_output` (`dlss_pipeline.cpp:1152`, `.h:243`) no
caller; `sole_viewport()` (`dlss_streamline.cpp:196-199`) unused; `"info"` label unreachable after the early
return (`dlss_streamline.cpp:114-119`) → `eError ? "error" : "warning"`; `State::supported`
(`dlss_streamline.cpp:76, 428, 706`) only read where written → local; `dlss_native12.cpp:24` `owned_module`
equals `!shared_host` when loaded [M].

**4.3 Native FG [S, H]**: `CpuOwner::render_expected` (`native_fg.cpp:165, 465`) write-only, derivable from
`expected_count`; `!begin &&` at 499 always true after the return at 495; `if (!begin)` at 551 right after
`if (begin)` → `else`. `native_fg_d3d12.cpp:166-167` and `182-183` compute `window_generation` twice → drop
182-183.

**4.4 Orchestrator and contract layers used only by tests [S, H facts / M removal]**:
- `fg_session`, `fg_providers`, `fg_leases`, `render_links`, `frame_sequencer` (~700 lines);
  `d3d11_present_bridge.cpp:36-44` re-implements `rsf_fg_get_provider`.
- Legacy `rsf_fg_provider` vtable (`backend.h:348-378`): `rsf_dlss_sr_provider` / `rsf_dlss_fg_provider`
  declared, never defined; FSR and XeSS FG stubs (`fsr_backend.cpp:82-131`, written twice differing only in
  return code; `xess_backend.cpp:76-106`) used only by `tests/fsr_backend.cpp`; `rsf_fg_swapchain_result` exists
  only for them; `fsr_backend.h` / `xess_backend.h` only re-declare `backend.h`. ~100 lines.
- `runtime/contract` negotiation: `negotiate.cpp`, `backend_registry.h`, `rsf_sr_route`, `probe` have no
  production caller; `RSF_BACKEND_COUNT`, `RSF_REASON_NONE`, `RSF_FG_STAT_GENERATED_PRESENTED` unused even by
  tests. ~300 lines.
- Decide: delete, or label as scaffolding for the planned ABI 2 work.
- Exported `RSF_RUNTIME_API` functions with no caller anywhere in the repo (whole-repo grep, including loader,
  apps, integrations, tools and managed code): `rsf_native_cpu_set_sink`, `rsf_native_window_last_present`,
  `rsf_plugin_session_status`. Public entry points, so removal is a judgement call. [M]
  (`rsf_d3d11_present_session` is used only by `tests/fg_present_bridge.cpp`. Not dead: `rsf_unity_sr_stop`,
  looked up by name by the loader, and `rsf_get_runtime_version`, a public API.)

**4.5 Graphics [S, H]**:
- `rsf_overlay_input_take_function_keys` route (`overlay_input.cpp:107, 668-693, 724, 875, 1050`), ~50 lines,
  and the extra `GetRawInputData` per WM_INPUT (3.22).
- `rsf_frame_tap_set_override_target`, `override_targets[]` and its loop (`frame_tap.cpp:229, 1021-1028, 3024`).
- `DivertState::blend_patched` (`frame_tap.cpp:278, 922`).
- `original_present1` (`d3d11_observer.cpp:67, 1322`); `present_*genuine_block` and `*_displaced` write-only
  (`d3d11_observer.cpp:65-87`).
- Double `installed = false` (`frame_tap.cpp:2830, 2845`); always-true `if (view)` (656); unused
  `RSF_FRAME_TAP_REFUSED_UAV`; stale misplaced comments at `frame_tap.cpp:679, 965`.

**4.6 AC7 [S, H / M test-only]**: `rsf_ac7_native_renderer_current` (`native_renderer.cpp:2597`);
`GraphPlan::node` (831, 1065); `EngineRecord::history_called` (`motion_capture.cpp:65, 189`);
`RSF_AC7_ROLE_SCENE`, `RSF_AC7_ROLE_UI_FILTER`; impossible `!site.expected` check (2414); test-only
`rsf_ac7_ui_is_widget_target`, `rsf_ac7_ui_is_candidate`. Left out on purpose: the 11 unused AC7 helper
fingerprints (removal would drop a refusal path).

**4.7 UI, Unity, SDK [S, H]**: ~23 `Stats` fields read only by tests (`model.rs:131-188`) plus `render_scale`,
`evaluated_fraction`; 11 `Intent` fields the panel never sets (`model.rs:298-319`); `RSF_UNITY_RENDER_EVENT`;
`Bootstrap.cs:18` `failure` can be local; unused `RSF_FRAME_ID_NONE`, `RSF_PHASE_*` except `UI_COMPLETE`,
`RSF_FRAME_FLAG_UI_DIVERTED`. *ABI:* `present_index`, and `rsf_game_render_pass::jitter_pixels` duplicating
`camera.*`.

---

## 5. Per-component simplification

### Backends
- `dlss_generation.cpp:294-306` `create()` inlines `rsf_streamline_host_upgrade_chain` (548-555); the
  native()+Release idiom appears 4 times (296, 553, 567, 806) = `rsf_streamline_host_native`. [H]
- `dlss_generation.cpp:454-476` marker looks up the same slot 3 times → `Token* find_slot(host, id)`, also for
  389, 440, 451, 481, 488, 503. [H]
- `dlss_generation.cpp` option-equality test repeated (316-320, 331-333); clamps at 338, 579 → `std::min`;
  ternary 410-412 → `std::max`. [M]
- Manual COM release where `ComPtr` fits: `dlss_streamline.cpp:388-408` (also mis-indented),
  `XessSession::no_response`, both structs in `shared_surface.cpp`. [M]

### Orchestrator and presentation
- `dlss_pipeline.cpp` plan-to-range commit 3x (487-496, 1213-1220, 1250-1259) → `commit_range()`. Do not fold
  the fourth (1310-1315): it skips zero-defaulting. [H]
- `dlss_pipeline.cpp:1262, 1319` identical `rsf_sr_legacy_create(..., sdk_directories[0..3] ...)` →
  `create_alternate()`. [H]
- `dlss_pipeline.cpp:439-479` vs `1007-1052` identical target creation + black clear with manual Release →
  `create_cleared_target()`; use `ComPtr` (already used at 1273). [H]
- `dlss_pipeline.cpp:170-186` three log forwarders; 270-313 parallel scene/layer `finish`/`worth_saying` →
  prefix template + one `Counters` struct. 853-876 five prefix locals → suffix table in the loop. [M]
- `sr_legacy_adapter.cpp:66-73` vs `dlss_pipeline.cpp:724-732` same "rebuild motion_resolve on size change". [M]
- `native_sr_d3d12.cpp:291-306` `last_*` update duplicated across branches. [H]
- `d3d11_present_bridge.cpp`: timing reset at 132-133 and 271-272 → `reset_timing()` [H]; `rsf_fg_options off{}`
  literal 3 times (228, 407, 606) [H]; the two Present paths share a present/notify/retire/switch tail (498-519 vs
  554-578) [M].

### Graphics
- `frame_tap.cpp`:
  - `texture_behind`, `texture_behind_target`, `view_extent` (464-518) share one body via `ID3D11View`
    (`GetResource` is on the base); the comment at 477-478 ("view types are unrelated") is wrong. [R S, H]
  - Saved-state release in divert duplicated (907-918 vs 1105-1116). [H]
  - Viewport/scissor scaling duplicated (597-626 vs 934-959). [H]
  - Render-target hook tails identical (1497-1503 vs 1534-1540). [H]
  - Nine pass-through hooks + three indirect draw hooks (2121-2155, 2358-2420) → template, ~70 lines. [H]
  - Both callers of `invalidate_geometry` clear samplers (2103-2118) → move into it. [H]
  - Four draw hooks repeat a 15-line sequence → `around_draw()`. [M]
  - Install table re-declares `Tap::Patch`; 16 `extra_originals` paired by hand (2675-2722). [M]
  - "find entry, test view, `entry_applies`" 5x → `applicable_entry()`; uninstall re-implements reset; gate
    reset 3x; watched-CB test duplicated; `consider_target_draw` rebuilds `fill_divert_facts`. [M]
- `d3d11_observer.cpp`: Present/Present1 parallel field sets (6 fields + 6 ref params) → `PresentRoute`
  template [M]; VS/PS create hooks hand-copy the adjacent `RSF_CAPTURE_SHADER` macro and repeat slot list
  `{13,16,17,18,14}` (~45 lines) [H].
- `fullscreen_pass`/`present_blit`: device-ownership check ad hoc 7 times. [M]
- `overlay_renderer.cpp`: 12 manual releases in destroy, 16 in `restore_state` → `ComPtr` (~80 lines) [H];
  three Map/memcpy/Unmap blocks [M]; same release cascades in `present_blit`, `motion_decode` [M].
- `overlay_input.cpp`: mouse-button VK mapping duplicated down/up (806-814 vs 830-837) [H]; presented-scale
  computation 4x [M]; cursor-hook rollback duplicates `remove_cursor_hooks` [M].

### AC7
- `native_renderer.cpp`:
  - `sites[]` and `hooks[]` parallel arrays used via ~40 magic indices → hook stored in `Site`, enum index. [H]
  - Three byte-verify loops → `verify()`. [H]
  - Engine-global `+0x3cbbc28` read 4 times; `engine<>` RVA helper bypassed (694, 1823-1834, 2284). [H/M]
  - Render-config fetch 11 times (~25 lines); per-view loop 3x; view restore duplicated (1099 vs 1125);
    dependency loop 2x. [H/M]
  - Pool/texture accessors re-inline `method` and `native_texture`. [H]
  - Call-relay install duplicated (2205 vs 2270) with inconsistent restore. [H]
  - `cloud_depth_*` raw COM → `ComPtr`; `WindowLease` / `TextureBindingLease` → `ComLease<T>`. [H/M]
- `motion_capture.cpp`: `x==0||x==30||x==59` test 6x [H]; five bound-view JSON blocks (~40 lines) [H];
  `shader_hash` / `shader_constants` lookups duplicated [H]; blob-writing loop duplicated [H]; field-by-field
  counter reset → `SessionCounters` [M]. (SEH helpers: 1.18.)
- `truesky_depth.cpp` / `truesky_motion.cpp` duplicate shader creation (1.3); `rsf_ac7_cloud_depth` raw COM [H];
  `scene_color.cpp` repeats layer release 3x [M]; `plugin.cpp:49-112` start/quiesce/stop → `transition()` (~30
  lines) [M].
- Left out on purpose: rerouting the glow-sibling resize (would call the trampoline instead of the hooked entry).

### Overlay (Rust)
- `ffi.rs` three `catch_unwind` + poison copies → `guarded()`. [M]
- `overlay.rs` two Vec+cursor drains → `Drain<T>` [M]; `RawInput` setup duplicated (139 vs 328) [H]; misplaced
  doc comment at 430 [H].
- `panel.rs` FG provider table defined twice (497 vs 194) [H]; nested-if `format!`, one-caller `body()`, two no-op
  scopes [M].

### Unity
- `bridge.cpp` busy-fence test 7x. [H]
- `UrpAdapter.cs` temporal binding + Submit duplicated 3x. [H]
- Harmony patches applied by position. [M]
- `make_pass` carries `[[maybe_unused]]` only because it sits outside its `#if`. [M]

---

## 6. Checked and not flagged

- DLSS preset: defaults to AUTO, changes only on user selection; no forced preset.
- Engine patches: AC7 patch/hook sites (`native_renderer.cpp`, `motion_capture.cpp`, `contact_shadow.cpp`) and
  `fsr4_compat.cpp` check expected bytes before writing.
- Unreal velocity constants in `texture_dump.cpp` and the observer dump path are debug-only.
- Reflex markers and sleep (synchronous by preference).
- `overlay_renderer.cpp` per-frame save/restore is small and bounded (but see 1.5 for duplication).
- D3D12 descriptor writes in `native_sr_d3d12.cpp`: normal practice.
- `consider_bound_set`, `candidate_passes`: already dirty-gated or pointer-only.
- Already shared: NT-handle surface/fence creation (`rsf_shared_surface_create` / `rsf_shared_fence_create`),
  FSR/XeSS runtime loading (`rsf::load_runtime` / `fg_library`), FG frame/command validation
  (`rsf::fg_frame` / `fg_command`), AC7 constant-buffer reads, `motion_capture` texture dumps.

---

## 7. Correctness findings (`/code-review xhigh`)

Correctness pass over the uncommitted change (shared FG input retirement, mandatory HUD-less colour) plus the
orchestrator, presentation bridge, FG providers, SR session and bridge, overlay host, Unity bridge and the AC7 and
Unity plugins. Not built or run. The pass capped itself at 15 findings and reports that lower-priority candidates
were dropped; a rerun with `--max-findings all` would list them. Ordered most severe first.

**7.1 DLSS SR history reset every frame when FG is requested but not active** — `native_fg.cpp:620`
Default AC7 on NVIDIA: `RSF_FG_ENABLE` unset and no saved choice, so provider 0, but `RSF_FG_MODE` defaults to
`RSF_FG_FIXED`. With no provider, prepare and retire set `cached.vendor = {}` (effective_mode 0), so every flight,
hangar and briefing frame through `rsf_native_fg_evaluate` gets `translated.reset = 1` (constants also shared with
DLSS-G for the frame token). DLSS SR never accumulates history: shimmer and aliasing. Same whenever FG is
ineligible (VSync unsupported, HUD-less missing, invalid CPU frame). Reset only on an activation transition.

**7.2 HUD-less capture now gates every provider, including DLSS-G** — `native_fg.cpp:718` *(uncommitted)*
`rsf_native_fg_final` only accepts RGBA8, BGRA8 or RGB10A2 at exactly `slot.output_width/height`.
`create_bridge` accepts R16G16B16A16_FLOAT (scRGB HDR) swapchains. An FP16 or `*_SRGB` final surface, or a family
target of a different size, returns before `hudless_ready`, so `inputs` is false every frame and DLSS-G, FSR and
XeSS never activate, silently. DLSS-G previously worked with ui_mode NONE and does not need HUD-less colour.

**7.3 Missing HUD-less input is reported as "waiting for flight"** — `native_fg.cpp:755` *(see 2.17)*
`cached.reason` has no code for it: prepared = 1, submitted = 1, `scene_supported` = 0, so reason = 4 and the
overlay says "Waiting for flight, hangar, or briefing" in flight, pointing diagnosis at screen classification.

**7.4 Swap-chain forwarders dereference `physical` unguarded** — `d3d11_present_bridge.cpp:591`
When `create_physical(backend)`, `(previous)` and `(0)` all fail, `faulted = true` and `physical = nullptr`.
Later SetFullscreenState (focus loss), ResizeTarget, SetColorSpace1, SetHDRMetaData, SetPrivateData or
IsTemporaryMonoSupported crash instead of returning an HRESULT; a call from another thread during
`clear_physical()` is a use-after-release. Route them through `query_physical`.

**7.5 Failed retirement poisons the shared slot ring** — `native_fg.cpp:872` *(uncommitted)*
Any failure of retirement allocator/list Reset, Close, queue Wait/Signal or fence creation (e.g. E_OUTOFMEMORY)
sets `vendor_value = UINT64_MAX`. SR evaluation uses the same 6-slot ring via `resources()`, which then calls
`SetEventOnCompletion(UINT64_MAX)`, blocks the render thread 10 s and returns FEATURE_FAILED, every 6th frame for
the rest of the process. The change made this reachable on every prepared frame (tagged_slot now set for FG-off
frames too, not only interpolated ones).

**7.6 FG provider choice saved before the switch succeeds** — `loader/proxy/src/dlss_bridge.c:2901`, also
`unity_sr_host.cpp:175`
Selecting FSR4 on RTX: `rsf_d3d11_present_request` returns OK, `rsf_fg_choice_save` writes FrameGeneration=4,
then `switch_provider` fails and keeps the old provider. The overlay's pending flag (requested != active) stays
set, greying out the FG checkbox and multiplier; the next launch uses the saved FSR4, fails and starts Off. Save
only after `rsf_d3d11_present_switch_result` reports success.

**7.7 ~1.2 GB VRAM of SR-only storage in every FG slot** — `native_fg.cpp:308`
`resources()` allocates render colour, exposure, an output-size shared surface and an output-size committed
D3D12 output per slot, even on the capture path (FSR/XeSS SR via `rsf_native_fg_capture`) that uses only depth and
motion. At 4K from 1080p: ~0.2 GB per slot, ~1.2 GB for six, ~0.9 GB untouched on the capture path. Only depth,
motion and HUD-less need per-slot lifetime. *(See also 1.19.)*

**7.8 Overlay host retries a failed start every frame; wraps the back buffer when idle** —
`loader/proxy/src/overlay_host.cpp:425` *(see 3.17)*
`rsf_overlay_host_start_device` never sets `stopped_after_failure` and reloads the panel DLL each call;
`rsf_overlay_d3d12_frame` retries it every frame. On Unity with the input hook refused, each frame runs
LoadLibraryW (leaking a reference), panel and renderer create/destroy and a log line. In the normal hidden state
every frame still wraps the back buffer, creates an RTV, acquires/releases and flushes 11on12
(`overlay_d3d12.cpp:36-47`) before `draw_target` returns 0.

**7.9 HUD-less capture runs with FG off** — `native_fg.cpp:579` *(uncommitted)*
With FG unchecked or provider Off, every frame still does an output-size CopyResource, an ID3D11Fence Signal and a
mid-frame immediate-context Flush; with a provider it also records COMMON-to-NPSR barriers and submits a
retirement list. Skip when `requested.mode == RSF_FG_OFF` or no provider. *(See also 3.13.)*

**7.10 Exposure presence flip rebuilds the FSR/XeSS context** — `sr_legacy_adapter.cpp:65`
`rsf_sr_bridge_set_auto_exposure` waits for all SR GPU work, then `select_session(force=true)` reopens the provider.
The adapter starts with `auto_exposure = 1`, so the first frame with exposure already rebuilds. If
`scalar_exposure()` returns null intermittently, each flip costs a CPU stall and lost history.

**7.11 Streamline host failure on NVIDIA disables the whole bridge** — `d3d11_present_bridge.cpp:342`
*(uncommitted; see 2.9)*
Missing or unsigned Streamline folder, or another mod's `sl.interposer` (NEEDS_RESTART), returns
`DXGI_ERROR_UNSUPPORTED`; the game keeps its swap chain and FSR3/FSR4/XeSS FG and the Off facade are unavailable on
RTX, though none needs the host. With runtime switching, fall through to the hostless branch and mark only DLSS-G
unavailable.

**7.12 CPU frame-ring entry without FRAME_END is never reclaimed** — `native_fg.cpp:425`
An entry is reinitialised only when empty or ended. A begun frame without `RSF_GAME_CPU_FRAME_END` (abandoned
tick, hook deactivated mid-frame) leaves `cpu_frames[id % 128].id` stale; every later frame on that index fails
`frame.id == id`, records no pacing or submission, gets no RenderSubmitStart marker and no FG: 1 in 128 frames
for the rest of the session, with periodic generation drops and history resets.

**7.13 `presentation_queue()` rebuilds the setup each frame; cold start drifts** — `d3d11_present_bridge.cpp:302`
*(uncommitted; same as 2.4, plus 2.17's always-true `hudless_ready` guard at `native_fg.cpp:802`)*
Lines 386-400 hard-code ui_mode NONE and `runtime_directory` instead of `settings.ui_mode` and per-backend
directories, so a HUD-less request is silently ignored without runtime switching.

**7.14 Duplicated transitions, backend whitelist and overlay FG stats mapping** — `native_fg.cpp:278`
*(same as 1.1 and 2.8)*
Adds one item not covered elsewhere: the overlay FG stats mapping is copied between `dlss_bridge.c`
`fill_overlay_stats` and `unity_sr_host.cpp` `render`.

**7.15 AC7 manifest claims validation ahead of the evidence** — `games/ac7/engine.json:14, 19` *(uncommitted)*
The edit sets `runtime_active` and `render_handoff_validated` to true alongside the new, untested HUD-less gate.
The new research note records DLSS activity as NOT RUN and AC7 gameplay acceptance as pending. AGENTS.md:
"Report capability and validation status honestly."

### Cross-reference

| Correctness | Also in |
| --- | --- |
| 7.3 | 2.17 |
| 7.7 | 1.19 |
| 7.8 | 3.17 |
| 7.9 | 3.13 |
| 7.11 | 2.9 |
| 7.13 | 2.4, 2.17 |
| 7.14 | 1.1, 2.8 |

### Suggested reading order

Correctness first, since several are live regressions in the uncommitted change: 7.1, 7.2, 7.5, 7.15, 7.4, 7.6,
7.12. Then the cheap high-value cleanups: dead code (section 4), 1.1, 1.2, 1.3, 2.1, 2.8, 3.10, 3.11, 3.17, 3.18.

## 8. Implementation status (2026-10-09)

Status after the implementation pass, the review pass and integration. Duplicate ids across groups are merged. "Behaviour" means a change a game run could show. Nothing here is game-tested.

| Item | Group | Status | Behaviour | Note |
| --- | --- | --- | --- | --- |
| 1.1 | G1a G1b G2 G5 | done | no | Shared d3d12_helpers.h used by native_fg, bridge, sr_bridge, native_sr_d3d12, fsr_generation. |
| 1.2 | G1b G2 G3a G3b G4a G4b G5 | done | no | Local say() copies replaced by rsf::say. |
| 1.3 | G2 G4a G4b G6b | done | no | All HLSL compiles go through shader_compile.h. Compile-failure log wording changed. |
| 1.4 | G4a | done | yes | present_blit is a thin wrapper over fullscreen_pass; new set_filter function; scissors now restored. Check the debug view. |
| 1.5 | G4a G4b | partial | no | fullscreen_pass uses a new narrow draw-state scope. overlay_renderer keeps its own save/restore (skipped, the full state helper rebinds targets). |
| 1.6 | G2 G4a | done | yes | Shared srv_format. Some typeless and alias formats are now accepted. Check translucency masks and region crops. |
| 1.7 | G3b | done | yes | PPM dumper uses the shared table and accepts more formats. |
| 1.8 | G3a G3b | done | no | Shared rsf::patch_slot. |
| 1.9 | G3b | done | yes | Observer uses rsf_read_constant_buffer and skips foreign devices. |
| 1.10 | G6b G7 G8 | done | no | Plugins use game_plugin_util.h. |
| 1.11 | G5 G7 G8 | partial | no | Done in C++ callers. dinput8_proxy.c skipped, the helper is C++. |
| 1.12 | G5 | partial | yes | Loader and helper split done. Shared sl::Constants filler not done. Interposer now loads with DLL_LOAD_DIR and SYSTEM32. |
| 1.13 | G5 | done | yes | DLSS native12 uses validate_frame and may refuse frames it accepted before. |
| 1.14 | G5 | done | no | Shared FSR/XeSS SR helpers. |
| 1.15 | G5 | done | no | session_status and not_compiled_provider. |
| 1.16 | G1a G7 | done | yes | Unity config unit adopted. Relative [UnitySR] paths are now canonicalised. rsf_unity_fg_start still to adopt it. |
| 1.17 | G6a G6b | done | yes | Shared install_hooks. The failure path removes hooks at once. |
| 1.18 | G6a G6b | done | no | Shared copy_memory and read. |
| 1.19 | G1a | partial | no | Per-slot split inside native_fg only. Sharing with sr_bridge skipped. |
| 2.1 | G1a G5 | done | yes | Only rsf_frame_allows_fg gates. Replay and other screens can now generate. |
| 2.2 | G1b G7 G8 | done | yes | Engine identity comes from setup, not literals. Unity SR setup still writes the nested setup.dlss fields (partial there). AC7 SR engine version is now 4.18.3. |
| 2.3 | G1a G1b | done | yes | rsf_d3d11_present_retire export; both retire paths use it. Unity join moves to the provider queue. |
| 2.4 | G1b | done | yes | Provider queue stored at publish. Non-switching cold start uses create_physical. |
| 2.5 | G1a G1b | done | no | provider_list replaces the unwrap sites. |
| 2.6 | G1a | done | yes | Depth, motion and HUD-less go to NON_PIXEL_SHADER_RESOURCE for every provider. |
| 2.7 | G1a | done | yes | frame_time_ms is the source-frame delta for all providers. |
| 2.8 | G1a G1b G8 | done | no | rsf_fg_backend_known and the backend info table. |
| 2.9 | G1b G5 | done | yes | Vendor check moved into create_host. Non-NVIDIA means no host. |
| 2.10 | G1a G1b | partial | no | Latency exports share with_provider. The unused host parameter of the prepare callback type remains. |
| 2.11 | G7 | done | no | Named packet flags. |
| 2.12 | G2 G7 G9 | partial | yes | RSF_SR_* constants and the sr_backend_choices mask done. Bare literals 1, 6, 7 remain in a few Unity and panel spots. Unity ABI bumped. |
| 2.13 | G2 | done | no | BackendNeeds table. |
| 2.14 | G2 | done | no | Canonical UV passed to the adapter. |
| 2.15 | G1a G2 | done | yes | Capture runs after every evaluate and is idempotent per frame. |
| 2.16 | G3a G8 | done | yes | Role table replaces AC7 knowledge in frame_tap. Frame tap ABI 14. |
| 2.17 | G1a | done | no | Helpers, named reasons, holds() checks native_frame. |
| 3.1 | G3a | done | yes | Sampler bias via maps; forwarding when no bias. |
| 3.2 | G3a G8 | done | yes | Buffer verdict cache, writable flag, loader arms or disarms the watch. |
| 3.3 | G3a | done | yes | A rebind to the same slot no longer refreshes the newest view constants. |
| 3.4 | G3a | partial | yes | SRV shadow dropped when nothing reads it. Lazy per-view resolution not workable. |
| 3.5 | G3a | done | no | Views resolved once. |
| 3.6 | G3a | done | no | Slot occupancy mask. |
| 3.7 | G6a | done | yes | Pixel evidence hooks off unless diagnostics are on. |
| 3.8 | G6a | done | no | Per-thread guard slots. |
| 3.9 | G6a | done | no | Lock-free final surface hint. |
| 3.10 | G4a | done | yes | Nested state saves are no-ops, spare State reused. Watch for corruption after nested passes. |
| 3.11 | G2 G4a | done | yes | SRV caches. Engine targets stay referenced up to about 2 s. |
| 3.12 | G5 | done | yes | DLSSOptions skipped when unchanged. |
| 3.13 | G1a G2 | done | yes | Trailing Flush removed; CPU waits flush first. |
| 3.14 | G1a G2 | partial | no | Caching done in sr_bridge and native_sr_d3d12. The native_fg final pass only partly (ABA risk). |
| 3.15 | G1b | skipped | no | A cached fullscreen state goes stale on focus loss. |
| 3.16 | G3b | partial | yes | Atomics instead of the guard. Per-swap-chain QI cache skipped (pointer reuse). |
| 3.17 | G7 | partial | yes | A hidden overlay skips the wrap. Per-back-buffer cache skipped (blocks ResizeBuffers). |
| 3.18 | G7 | partial | no | Some reflection replaced. Needs a Harmony 2.4.2 managed build to confirm. |
| 3.19 | G7 | done | yes | Interlocked gate replaces the lock. Stop may spuriously return busy. |
| 3.20 | G7 | done | no | Static render funcs. |
| 3.21 | G7 | partial | yes | Configuration cached per frame. Eligible() not cached. |
| 3.22 | G4b | done | yes | One RAWINPUT read. |
| 3.23 | G9 | partial | no | HUD strings cached. Layout reuse not done. |
| 3.24 | G4b | done | yes | Render target check runs once. |
| 3.25 | G3b | done | yes | Module image read at most once. |
| 3.26 | G1a G6b | done | yes | PPM writes off the present path. Constant-buffer snapshots may use readback more often. |
| 4.1 | G2 G5 | done | no | Forced preset path removed. Nothing is forced. |
| 4.2 | G2 G5 | done | no | Dead code removed. layer_output kept as an unused export. |
| 4.3 | G1a | done | no | Dead fields and branches removed. |
| 4.4 | G1b G5 | partial | no | Stubs deduped. Provider lookup stays separate to avoid inverting layers. |
| 4.5 | G3a G3b G4b | done | yes | Dead code removed, including an overlay export. Part of the ABI 14 bump. |
| 4.6 | G6a G6b | done | no | Split across two groups; both parts done. |
| 4.7 | G7 G9 | done | no | Dead Unity and panel fields removed. C layout unchanged. |
| 5.x | all groups | partial | no | Local refactors done in every group. Skipped: PresentRoute template, overlay_renderer ComPtr, overlay presented-scale helper, cloud_depth ComPtr. |
| 7.1 | G1a | done | yes | SR history resets only when the generating state changes. |
| 7.2 | G1a | partial | yes | HUD-less requirement refuted for DLSS-G (it needs it). Wider format list confirmed (sRGB, RGBA16F). |
| 7.3 | G1a | done | yes | A missing HUD-less frame reports reason 8. |
| 7.4 | G1b | done | no | Unguarded physical dereferences fixed. |
| 7.5 | G1a | done | yes | A failed retire marks the slot unknown, released by an untagged join. |
| 7.6 | G7 G8 | done | yes | FG choice saved only after the switch succeeds. |
| 7.7 | G1a | done | no | SR-only storage allocated only for SR evaluation. |
| 7.8 | G7 G8 | partial | yes | A failed overlay start is final until stop. Cheap idle early-out in overlay_d3d12 still open. |
| 7.9 | G1a | done | yes | No work when FG is off. |
| 7.10 | G2 | done | yes | Exposure policy hysteresis. |
| 7.11 | G1b | done | yes | A refused host no longer drops the bridge. |
| 7.12 | G1a | done | no | Ring entry reinitialised. |
| 7.13 | G1a G1b | done | yes | Cold start and always-true guard fixed. |
| 7.14 | G7 G8 | done | no | Shared rsf_overlay_fill_fg_stats. |
| extras | G7 G8 G9 | done | no | sr_backend_choices filled by both hosts, panel text for reason 8, shared ffi guard helper. |

7.15 (manifest claims) was not in the status data and is not recorded as done here.

### Review-phase fixes

- native_fg: a failed retirement left surfaces in the read state; they are now restored before reuse. Held slots are released by an untagged join. A mutex guards the vendor fence fields.
- Bridge: native12 adopt refuses the bridge only on INIT_FAILED. The retire fence is created before any work is queued. The fg_present_bridge test carries the engine identity.
- sr_legacy_adapter: hysteresis applies only to auto to manual, so XeSS never sees manual mode without an exposure resource.
- frame_tap: positive buffer widths are always re-queried; sampler bias resolves all slots before biasing; re-arming the constant watch resets it to the copy path.
- G5: load_interposer requires an absolute path. Other findings left unchanged with reasons.
- G7: the FG radio keeps the clicked provider while a save is pending. The export is renamed to rsf_unity_set_engine_spatial so a stale plugin fails at start. make_pass sets screen FLIGHT. One finding (overlay_d3d12 retry) was refuted: stop already resets the flag.
- G8: overlay host stop clears the failure flag. refresh_constant_watch only calls on a change.
- Integration: shadowed variable in native_fg, GuardSlot padding warning, rsf_sr_session_test link, texture_dump format check, Unity pass.screen copied into the frame record, stale dlss_pipeline.h comment.

### Final verification

Green. eng/verify.ps1 Release: build clean, ctest 49 of 49 with 7 hardware-gated tests skipped, cargo fmt, clippy and test clean (37 + 19 tests). The RTX shared-host test passed. No game run was made for this pass.
