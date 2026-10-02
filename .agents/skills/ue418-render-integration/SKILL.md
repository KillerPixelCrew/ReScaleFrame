---
name: ue418-render-integration
description: Analyze shipped UE4.18 renderer behavior against the matched 4.18.3 source and AC7 evidence. Use for version-specific composition graph hooks, private view layouts, UI converters, jitter and queued RHI integration; general Unreal 4 concepts live in the Unreal 4 skill.
license: GPL-3.0-only
---

# UE4.18 render integration

For shared renderer concepts, use [unreal4-render-integration](../unreal4-render-integration/SKILL.md).
This skill adds the version-specific evidence for ReScaleFrame's AC7 integration. Read
[the ownership reference](references/ac7-native-ownership.md) and the latest game evidence for an
AC7 change. Build-specific RVAs/private layouts are not portable UE4.18 APIs.

## Match the right source and program

AC7 research uses stock `4.18.3-release`, revision
`0a14a8d537a31ecc77488ced41dbaa0166612ef8`, under `references/UnrealEngine`.
Verify the checkout or read `git -C references/UnrealEngine show 4.18.3-release:<path>`.
The full clone also contains much newer branches; do not read its default branch by accident
or silently reset a shared checkout. Stock source concepts must match the decrypted game.

Confirm the selected Ghidra program against `games/ac7/engine.json`. Guard native hook/helper
entries, calling conventions and structures before use, after decryption. Name/save verified
functions and record sites/expected bytes in [the hook map](../../../docs/research/ue418-hook-map.md).
Use the manifest/current source for exact guards instead of transcribing addresses from a skill.

## Follow the working AC7 integration

The proxy prepares the AC7 plugin before graphics activation. `native_renderer.cpp` owns
renderer/family sizing, temporal preparation, a registered pre-tonemap SR/fallback graph node,
reviewed downstream descriptors, native UI raster owners and queued resource leases.
The runtime owns vendor evaluation, cropped-region preparation and GPU state/interop.
The old texture matcher, allocation promotion and screen-percentage timer are compatibility
paths, not the default mechanism to revive when an engine-owned path refuses.

Preserve exact backend input rectangles inside engine-padded allocations. For the matched
`SceneColorHalfRes` topology, bloom/exposure consume reconstruction and preceding engine exposure
avoids a graph cycle. Unmatched topology retains reviewed auxiliary dependency order or refuses.
Keep `PF_FloatRGBA` scene production and native tone mapping; a later FP16 copy is insufficient.

Join `WaitForOutstandingTasksOnly` before editing shared views, then copy/AddRef uniforms rather
than clearing their shared slots with native move. Native parameter production and RHI uniform
creation replace uploaded-CB rewrites or dynamic drawing-resource initialization. Queue copied
identity and keep generated/pool references through RHI use; retire them on the render owner.

AC7 UMG/game-instance targets have physical output resolution with logical 1920x1080 layout.
Preserve game glow, depth, dirty/repaint and mono/stereo owner checks. Briefing translucency has
its own output-relative branch; it is not evidence that all UI or clouds should be reassembled
after SR. For cloud projection/depth production, use
[truesky-render-integration](../truesky-render-integration/SKILL.md).

The 4096-byte view bucket contains different view classes. Read constants for the actual pass,
validate current/previous matrices and explicit motion units, and inspect terrain HS/DS as the
final projection producer. Valid zero velocity must survive the unwritten-vector test.
Carrier/refuelling weapons and moving ground vehicles remain coverage questions.

## Preserve the accepted scope

The user accepted the corrected native SR/UI and wing/cloud result on 2 October, followed by
the SR bridge/FSR4 compatibility deployment. Historical build-only notes remain as a trail,
not current rejection of that accepted path. Do not extend acceptance to arbitrary view paths,
every missing-object vector, other GPU families, FG surfaces or vendor latency markers.

For changes, compare briefing, hangar, flight and pause/resume at multiple scales; verify actual
native execution packets, temporal resets, worker recording, backend/size transitions and
quiescent teardown. WARP tests prove contracts separately from shipped-game behavior.
