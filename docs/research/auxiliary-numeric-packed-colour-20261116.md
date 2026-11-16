# Numeric packed-colour auxiliary transactions, 16 November 2026

Built and WARP-tested extension of the existing generic auxiliary COLOUR
transaction. Coordinator results are recorded below; no game or physical
scanout acceptance follows from the synthetic numeric coverage.

## Question and mechanism

The colour whitelist in `auxiliary_sr.cpp` admitted several float/UNORM formats
but excluded R11G11B10_FLOAT. A leased numeric texture in that format therefore
refused the transaction even though the existing nearest path can represent its
positive finite values without a new processing path.

The existing mechanism copies native contents into a same-format snapshot,
loads a nearest texel in the pixel shader and draws into a same-format private
render target. It copies the completed private result back only at commit.
There is no transfer function, tonemap, linear filter, alpha reconstruction or
format conversion added. Numeric floating storage alone still says nothing
about the engine's colour-space convention.

The only production change adds R11G11B10_FLOAT to that path when the owning
device's CheckFormatSupport succeeds with all three required flags:

- TEXTURE2D for the snapshot/output allocations;
- SHADER_LOAD for the shader's integer-coordinate Load;
- RENDER_TARGET for the private output's RTV.

Typed UAV load/store support is deliberately not required: this path neither
creates a UAV nor dispatches a compute shader. Requiring unused UAV support
would reject devices that can perform the actual operation. Snapshot SRV/RTV
creation is still checked by the existing preparation code. A creation failure
uses the existing restore/cancel path.

Capability admission occurs in the initial all-input validation loop before any
native snapshot, context binding change or pending transaction is published.
The existing formats keep their previous whitelist/creation behavior. There is
no ABI, user option, new export, game/engine policy or parallel framework.
Native descriptor/bind flags are unchanged; only the private copies get the
existing SRV/RTV flags. RESTORE_CONTENTS semantics are unchanged.

## Focused WARP fixture coverage

The existing auxiliary_sr WARP fixture creates an actual8x6 R11G11B10_FLOAT
render-target allocation. Its source words contain distinct positive finite
normal unsigned-float channels, including magnitudes above1. These are synthetic
numeric inputs, not a new game capture or proof of an active engine producer.

The source crop `[1.25,.75,5.5,4.75]` expands by nearest selection into output
`[2,1,8,6]`, with jitter `.25,-.125`. The fixture checks the actual texture
format/extent/sample descriptor, then independently computes source texels.
Every output packed word must exactly match the selected source word; texels
outside the output rectangle must retain their original words. Decoded RGB
channels must remain positive, finite and numerically identical, with no HDR
clamping or transfer conversion. The fixture also checks:

| Transaction stage | Expected behavior |
| --- | --- |
| Prepare | Native packed texture and SR destination remain byte-identical. |
| Commit plus RESTORE_CONTENTS retirement | Expanded numeric contents are visible before retirement, then every original auxiliary bit is restored while accepted SR colour survives. |
| Failed SR after preparation | Cancelling restores the SR destination and leaves the native packed auxiliary unchanged, with no pending lease. |
| Invalid second readable destination during preparation | Preparation refuses, clears the partial transaction and preserves both original source and destination bytes. |
| Successful commit without RESTORE_CONTENTS | A later native clear survives retirement rather than being overwritten by the earlier snapshot. |

The fixture checks the same three capability bits and retains refusal/no-write
expectations for an unsupported device. It expects the WARP device used for the
numeric coverage to provide the required capabilities; no optional UAV claim is
made. Existing depth, other colour formats and depth-domain cases remain in the
same test target.

The coordinator's background Release build exited 0 in
`.local/fallout4-research/build-scene-input-marker-20261116.log`. Its safe CTest
subset exited 0 with 218 passed and 14 skipped out of 232, in 82.22 seconds
(`ctest-scene-input-marker-background-20261116.log`). The real
`auxiliary_sr_device` WARP numeric fixture passed in 0.10 seconds, covering the
packed-format transaction checks above alongside the existing auxiliary cases.
Cargo fmt and Clippy were clean. Four foreground FG tests were explicitly
excluded while the user worked; this is a partial gate and full
`eng/verify.ps1` remains deferred.

The separate hidden marker fixture (`hidden-marker-scene-input-20261116.log`)
and scoped fixture (`scene-input-scoped-fixture-20261116.log`) both exited 0.
The scoped run recorded 55 sources, 41 captures, 39 SR evaluations, 34 tags and
34 generated callbacks, with zero rendered-source latency owner, pacing and
markers. Matching Runtime SHA256:
`90E86B69D96B29D5D74F42E1A5DF61289FC0C4B3CD72EC4B1EC9C793F9F13178`.
These broader device results do not extend the packed-colour fixture into proof
of a native producer, physical scanout, successful game integration, latency
improvement or visual quality. No new game/deployment acceptance is recorded.
