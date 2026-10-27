# GTA V Enhanced renderer: initial scaffold

## Question and evidence, 27 October 2026

Determine which RDR2 RAGE concepts and shared RSF components can be reused for GTA V Enhanced,
and which game-owned producer sites must be established independently. This increment supplies
an entry and plugin scaffold; SR and FG are not implemented.

Steam app3240220 is downloading to `Grand Theft Auto V Enhanced`. Its manifest reported target
build25971420, StateFlags1026 and zero bytes staged/downloaded at inspection. The staging directory
already contains `GTA5_Enhanced.exe`, but its first64 bytes were all zero and the PE inspector
rejected it. Its apparent file length is not proof of downloaded content. No executable hash,
version, PE fingerprint, imports, RVAs, or bytes are accepted from that placeholder. Reinspect
the completed installation and record its actual identity before adding an allowlist entry.

Rockstar advertises native DLSS, FSR, ray-traced features and DirectStorage for Enhanced in its
[Steam listing](https://store.steampowered.com/app/3240220/Grand_Theft_Auto_V_Enhanced/).
Rockstar's [graphics support page](https://support.rockstargames.com/articles/2vEOZW3tzshakakcwRvakg/grand-theft-auto-v-on-pc-crashing-with-err-gfx-state-error)
describes DX12 and Vulkan options. These are product facts, not measured RSF pipeline inputs.
The game is RAGE-family, but exact structures, axes, depth conventions and code ownership remain
unverified for this build. Do not reuse RDR2 RVAs, bytes or structure offsets by resemblance.

## Existing references and concrete leads

- The existing local [OptiScaler source](https://github.com/optiscaler/OptiScaler/blob/92337ebfbf07d533eabe3731fd8de1bb19437aea/OptiScaler/misc/Quirks.h)
  disables its HUD-fix path for `gta5_enhanced.exe`. Its separately versioned
  [game-specific wiki](https://github.com/optiscaler/OptiScaler/wiki/Grand-Theft-Auto-V-Enhanced)
  reports DLSS and FSR3.1 inputs, hardcoded quality presets and a BattlEye compatibility issue.
  These are external observations, not proof for the current build. Do not copy its implementation.
- The existing local [RenoDX GTA V Enhanced implementation](https://github.com/clshortfuse/renodx/tree/da608c4d4feaa41b261e252440a73337613c2705/src/games/gtav-enhanced)
  has composite, depth-of-field, motion-blur, calibration, Bink and other shader candidates.
  Its addon excludes `RGSCD3D12_TEMPWINDOW` and describes an HDR10 swapchain conversion path.
  Those are capture targets and a Social Club coexistence lead, not an established HUD-less
  boundary or a reason to reuse the RDR2 Social Club patch.
- [Script Hook V's developer](https://www.dev-c.com/gtav/scripthookv/) describes GTA V ASI loading
  and says Script Hook does not work in GTA Online. A version-compatible Enhanced loader,
  initialization order and coexistence with this game's current protection are not yet tested.

The shared runtime, native D3D12 overlay, backend contexts, presentation bridge, bounded census,
frame capture and frame/camera ABI can be reused once the game plugin supplies validated leases.
RDR2's worker-to-view association, carrier selection, quality transaction, camera decoder,
Social Club fix and scene/HUD boundary are hypotheses to investigate, not portable constants.
The intended control model follows RDR2: RSF owns the upscaler and FG choices in its overlay,
routes game inputs into one canonical adapter, and avoids running the game's own FG alongside
RSF. Native FSR3.1 is a candidate input carrier where DLSS is unavailable. These are design
targets, not implemented paths or a hardware restriction.

## Next measurements

1. After Steam completes, fingerprint the installed executable and inspect imports, strings and
   code availability. Record its actual SHA-256, PE metadata and file version in engine.json.
2. Establish the usable ASI and initialization path for this exact build. Keep existing loaders,
   RTSS and launcher/protection settings intact. Use the generic vendor census in an explicitly
   opted-in diagnostic run and correlate DLSS/FSR, Present and submission threads.
3. Trace native SR/FG ownership, render-resolution production and quality transitions. Identify
   current/previous camera, jitter, motion and depth inputs at the upscaler, with units and frame
   identity. Verify native DLSS and FSR inputs before choosing their shared adapter.
4. Capture the completed scene before UI and the final image, including map, pause, cutscenes and
   post-HUD changes. Verify the boundary on pixels, then validate output reinsertion and real FG.

No game capture, runtime hook, upscaler dispatch, UI, output reinsertion or device acceptance has
been performed for GTA V Enhanced. The scaffold refuses unknown fingerprints and reports
`rendering_ready = 0`. Release VS2026 verification passed99 discovered tests with11 optional
skips, formatting and Clippy. The plugin contract and a separate stand-in process fixture load
the real ASI to verify default-off behavior, Legacy exclusion and unknown-fingerprint refusal.
These tests do not establish compatibility with an installed GTA V Enhanced build.
