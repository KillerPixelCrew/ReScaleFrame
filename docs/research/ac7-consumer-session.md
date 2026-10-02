# AC7 briefing isolation and consumer session controls

29 September 2026. Source review and implementation against `37360603681ce20e6afff86745384173872a5df0`
plus the existing local gate tracing additions. No new binary addresses were discovered. The
inherited AC7 evidence names Steam build 9855922, `Ace7Game.exe`, SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`, and UE
`4.18.3-release`, `0a14a8d537a31ecc77488ced41dbaa0166612ef8`. The reference checkout is absent
on this machine; this change reuses the documented sites, not a new matching-source analysis.

## Question and evidence

The briefing terrain and aircraft symbols shimmer. The user requires their vanilla resolution to
survive scene upscale quality changes: the vanilla layer is half the native scene in each dimension.
At 50% scene scale, leaving its multiplier at 0.5 would produce quarter-output dimensions. Preserve
half-output dimensions instead, then either draw without jitter or resolve its jitter before recombine.

The current code had advanced to a second native-resolution DLSS feature. Saved September 26 logs
record 6,533 layer integrations with zero refusals and 3,268 finished recombinations. This proves the
feature ran, not image quality or correct per-view history. Those aggregate counts warrant a view
and gate-order trace; they do not establish that the same visible view was evaluated twice.
The existing local tracing changes are retained.

The earlier [composed-colour notes](ac7-composed-scene-color.md) attributed residual changing detail
to stochastic materials from two route dumps. That is still a hypothesis about the material producer,
not a shader-disassembly result. A stationary layer with changing pixels can also show sampling
aliasing. Simply increasing resolution, or counting successful DLSS calls, does not distinguish them.

## Implementation and runtime use

The existing allocation-scale patch at RVA `0x10be329` now defaults to `target / render = 100 / S`,
where S is the successfully applied scene percentage. Before applying a preset, S is 100. At 3840x2160
output the nominal briefing stays 3840x2160 for Native, Quality, Balanced, Performance and Ultra
Performance. This changes the upstream engine scale used for layer allocation, viewport and depth;
it does not enlarge an already rasterized texture. The engine's pooled allocation padding and
integer rounding remain, so exact resource extents across arbitrary aspect ratios still need live
measurement. Explicit diagnostic target/scale overrides remain possible. This currently applies to
the separate-translucency pass generally; it is not a new briefing-widget-specific hook.

Default `RSF_TRANSLUCENCY_UNJITTER=1` bypasses the layer depth replay, second DLSS evaluation and
integrated-layer substitution. On the active recombine route, the captured single-target RGBA16F
geometry-with-depth shape gets its view twins in VS and PS. The generic tap prefilters by format
before the first draw into a new allocation; the AC7 rule validates the remaining pass facts. This
is the existing capture-derived shape, not verified material identity. No pixel shader code is
replaced, and no temporal/material noise input is frozen. Other shader stages and indirect draws
are not covered by this override and remain a validation limit.

Frame-tap ABI 12 appends pixel-stage bindings and extends override slots to VS 0..13 / PS 14..27.
The original buffers are restored after each draw, including instanced draws. Inherited bindings
are seeded before the first override; ClearState invalidates the shadow. The main scene keeps its
jitter and is resolved before recombine. The layer retains the game's blend/alpha convention and
is composited before tonemapping. `RSF_TRANSLUCENCY_UNJITTER=0` selects the previous experimental
1:1 temporal route without changing the output-relative resolution policy.

DLSS is requested by default and starts on the Present thread once patching and the device are
ready. Reinsertion waits for discovered resources and a fresh successful evaluation. The overlay
has only Enable DLSS and preset selection; Insert is the only bound hotkey. Disable restores scene
percentage 100, removes reinsertion and closes forced jitter. Re-enable reapplies the selected
preset. The backend remains loaded while disabled; no frames are evaluated in that state.

The previous preset setter only accepted a current screen percentage of 100, so a second preset
change could update DLSS without changing the game. It now accepts the last successfully written
percentage, with 100 as the level-load fallback. The jitter sample-count setter similarly accepts
its previous value. A failed scene-scale write rolls the backend preset back. Successful changes
reset temporal history and rebuild reinsertion after fresh evaluation. Native/DLAA is allowed to
reinsert at equal resolution, including the engine's four-pixel pool padding.

Enable/preset choices are stored in `%LOCALAPPDATA%/ReScaleFrame/AC7.ini` and validated when read.
Installation settings supply first-launch defaults; saved choices override them. Without a capture
path, startup uses a local application-data log directory. Executable dumps/module sampling are
opt-in through `RSF_DUMP_MODULE=1`. Vendor info spam is not forwarded; errors/warnings remain.

## Validation

The Windows/MSVC Release native suite passed all 24 tests during implementation, including new
D3D11 WARP pixel readback of VS/PS overrides, first-draw inherited buffers, state restoration,
instanced draws and ClearState, and preference round trips for all five presets. Scale-policy tests
exercise repeated 100/50/67/58/34 percent changes and confirm the nominal output-relative baseline.
The existing view-matrix tests cover projection/inverse corrections. Native/DLAA reinsertion is
covered by the promotion test. An unrelated Release-only fixture failure was corrected by making
its asserted GetProcAddress import an explicit test dependency; optimization had removed it.

Final verification results are recorded in the implementation tracker. No new AC7 visual test,
GPU DLSS quality comparison, MSE result, independent icon-motion validation or flight regression
is claimed. The next game check is launch without hotkeys, switch every preset, disable/re-enable,
restart to verify saved choices, then compare stationary and moving briefing terrain/icons. Check
raw layer and final composition separately if shimmer remains. The original recording cannot be
used as an after-change comparison.

## User validation and full-output sharpness follow-up

29 September 2026. After trying the deployed unjittered half-output path in the briefing, the user
reported "Looks good" and supplied a still showing the terrain contours and aircraft symbols.
This is user-reported acceptance of that path, not a new measured temporal sequence. They then
requested 100% output resolution for sharper detail. The default and installed target are now
100, with the unjittered path retained. The recombine matcher accepts up to the scale patch's
four-times ceiling so full-output layers remain discoverable at Ultra Performance (roughly three
times scene dimensions). Scale tests cover both half and full output; the D3D11 composition test
covers a three-times layer. Full-output image quality remains to be checked after relaunch.

The prior live launch was log-verified at 800x450 to 1600x900 with Performance and automatic
reinsertion. It also emitted Streamline's missing presentCommon bookkeeping warning on the existing
manual-hooking integration; this is an outstanding lifecycle issue, not evidence that the new
briefing path failed. No preset-switch or flight visual result is claimed from that launch.

## Full-output jitter: pixel-to-world unjitter correction

29 September 2026. The user reports that terrain jitter returns at the requested 100% output
resolution. The fixed-resolution policy alone did not settle the defect; keep that failed trial
separate from their accepted 50% trial.

The next question was what differs when the layer no longer shares the scene's size. Authorized
GitHub reads of the same UE `4.18.3-release` source established that
`FDeferredShadingSceneRenderer::SetupDownsampledTranslucencyViewUniformBuffer` copies the main
view, then calls `FSceneView::SetupViewRectUniformBufferParameters` for the layer dimensions.
`SceneView.cpp:2146` constructs `SVPositionToTranslatedWorld` from pixel-to-clip multiplied by
`ClipToTranslatedWorld`. In contrast, `ScreenToWorld` and `ScreenToTranslatedWorld` at lines
2259/2266 include an additional linear-depth mapping. The reference files remain under ignored
`.local/briefing-native-research/`; no reference implementation was copied into the repository.

The unjitter helper incorrectly treated SVPosition as the second kind. It added the jitter
correction to row 2, although device-depth pixel positions require row 3. The synthetic view
builder made the same mistake by constructing SVPosition from ScreenToTranslatedWorld, so the
previous passing test did not verify the engine's actual formula. Correcting the fixture first
made the old implementation fail: SVPositionToTranslatedWorld relative error 0.000247627.

The six saved `capture00` through `capture05` 4096-byte buffers also agree exactly with the
pixel-to-clip times ClipToTranslatedWorld relationship at offset 0x240. They carry zero jitter and
1600x900 views, so they corroborate matrix construction, not the full-resolution briefing's
per-draw jitter or shader use. Their error against the old fixture's construction is about 2.98.

`rsf_ac7_view_remove_jitter` now corrects row 3 using ClipToTranslatedWorld rows 0/1, leaving the
linear-depth screen matrices' row-2 correction intact. The regression also unprojects sample
pixels at device depths 0.01, 0.2 and 0.8 for 800x450, 1600x900, and an offset rectangle in a
1600x904 layer. The corrected results agree with independently constructed unjittered matrices.
This code runs when the view upload is twinned, before the existing per-draw VS/PS override;
no new hook site, allocation policy or jitter sequence was introduced.

`eng/verify.ps1 -VS2026 -Configuration Release` passed, including all 24 native tests, formatting
and Clippy. This verifies a math defect and its correction. Whether it explains the user's
resolution-dependent artifact remains a game-test question; do not promote it to a confirmed
visual fix or rule out view selection, material sampling or other shader stages yet.

## Captured enlarged-view mismatch and tessellation gap

29 September 2026. The next user report said terrain disappeared; a subsequent run of the same
instrumented image restored it but remained jittery. This does not establish that the matrix
correction deterministically removed the terrain. The investigation moved from source-only math
to the actual briefing draws. `RSF_BRIEFING_CAPTURE_PREFIX` opts into a bounded one-shot capture
after 120 heavy separate-translucency frames: two route frames, up to 128 draw snapshots, and up to
64 MiB of created VS/PS bytecode. Raw captures stay ignored. Serial numbers distinguish multiple
recombine gates in the same presented frame. The overlay and hotkeys are unchanged.

The first capture (`briefing-regression-20260929-212823`) contains 59 draws per frame into a
1600x904 RGBA16F layer. All 236 recognized VS/PS view bindings across two frames were 800x452,
and all had zero-jitter twins. Original/twin data agree with the corrected pixel-to-world matrix
relationship. Missing unjitter copies and the old SV matrix formula do not explain these draws.

Source and decrypted-image inspection found seven unpatched consumers of the separate-translucency
scale. DrawMesh selects the downsampled view only for Scale < 1, and six template instantiations
of FSceneTextureShaderParameters::Set choose the sized depth SRV under the same condition. At
Scale 2 the allocation and DSV patches worked, but the shader inputs still belonged to the smaller
scene. The new gates select those inputs at Scale != 1. All original windows and their float-1
loads were checked with Capstone against the existing decrypted dump; the executable SHA-256 is
the same recorded AC7 build. The configured Ghidra bridge refused connection and no local Ghidra
installation was found, so no Ghidra naming is claimed. Proposed source identities and exact RVAs
are retained in the hook map for later project annotation.

The follow-up capture (`briefing-view-fix-20260929-213545`) verifies the engine patch: the same
236 VS/PS bindings now use 1600x904 views with matching unjittered twins. The user nevertheless
reports little improvement. Correct view/depth selection is required, but was not the whole defect.

Draw 8 supplies 536,400 elements with D3D11 topology 44 (12-control-point patches). Its captured
VS hash is 7a50b8ab and PS hash 7cb42112. D3DCompiler disassembly shows VS_To_DS_Position output,
not a raster SV_Position. The domain shader owns the final tessellated position; overriding VS
and PS does not remove jitter from its projection. The PS directly reconstructs positions through
cb0[36..39], confirming use of the pixel-to-world matrix. This shader evidence and actual topology
replace the earlier assumption that VS/PS covered the terrain.

Frame-tap ABI 13 extends the override to GS, HS and DS as well as VS/PS. It queries the later
stages' current constant bindings on offered draws, binds twins for the complete layer draw,
and restores every original stage/slot immediately afterward. The UI keeps its existing VS-only
policy. A D3D11 WARP test now tessellates a triangle and passes it through GS; every stage contributes
to measured pixels, and the test checks all bindings return after drawing.

Captured logs also contained old view twins for slots whose current original could not be read
as 4096 bytes. The cache previously invalidated non-view contents only on 4096-byte uploads, so
an address reused for a smaller material buffer could retain an old view pairing. All other-sized
uploads now forget it, lookups verify the live buffer descriptor, and failed twin uploads invalidate
the pairing. A synthetic stale-entry fixture verifies that a small material allocation cannot receive
a cached 4096-byte view. This fixes a concrete cache hazard; the intermittent missing-terrain report
is not attributed to it without a matching causal capture.

The Windows Release verification gate passed with all 24 native tests, including tessellation/GS
pixel readback, plus formatting and Clippy. The all-stage build is deployed for a separate automatic
capture. Its terrain image quality remains pending the user's result and captured DS bindings.

## Accepted result

29 September 2026. After the all-stage build, the user confirmed "Thats it" and declared the
upscaler finished. The final automatic capture (`briefing-tessellation-fix-20260929-214407`)
contains 472 recognized graphics-stage view bindings, with no missing twins. The terrain's
536,400-element, 12-control-point draw binds 1600x904 views in VS b1 and PS b0, and 800x452
views in HS b2 and DS b0. The latter retain the engine's stage-specific view size; their normalized
projection jitter is removed as well. Original jitter in layer pixels was (-0.327945, 0.568017)
for VS/PS and (-0.163972, 0.284008) for HS/DS. Every corresponding captured twin reads zero jitter.
Thus the final stage that projects the tessellated terrain is now covered. The user's visible
acceptance closes the briefing shimmer issue for this tested setup at 100% output resolution.

The opt-in capture setting was removed from the installed INI after verification. The installed
consumer defaults remain DLSS enabled, automatic reinsertion, full-output unjittered translucency,
and Insert for the overlay with Enable DLSS and preset selection. Existing saved preferences
remain intact. The Windows Release gate passed all 24 native tests; the consumer UI's 51 Rust
tests passed earlier in this session and were unchanged by the shader-stage correction.

This records completion of the user's AC7 upscaling task. It does not claim a new device matrix,
frame-generation/latency validation, or completion of the separate public plugin lifecycle. The
existing Streamline shutdown/manual-present bookkeeping observations remain separately recorded.

## Insert regression from a mixed deployment, 30 September

The capture proxy was rebuilt against overlay ABI 4 but deployed with the older ABI 3 Rust DLL.
The game log records panel-creation refusal before the input hook is installed. Direct calls to
the installed DLL confirmed that it accepts ABI 3 and rejects ABI 4. This explains why Insert
could not open the panel; it was not a key-binding change.

Rebuilding `rescaleframe-overlay` and deploying it with the proxy resolves the compatibility
failure. The real Rust panel passed the Windows D3D11 host fixture: Insert open, repeat suppression,
close and reopen, nonblack rendered pixels, restored state, and swap-chain resize. The initial
new key test sent separate presses within the existing 250 ms debounce window; spacing those
presses beyond that window tests the intended behaviour. Windows Release verification passed
30 tests with five vendor-environment skips. No Rust behaviour changed.

`eng/deploy-ac7-proxy.ps1` now builds and tests the actual pair before backing up and replacing
both files in an explicitly supplied game directory. Native routine verification remains isolated
from game installations. Deployment succeeded; a corrected AC7 run remains user-validated work.

## Shutdown-log clarification

During release preparation, the user clarified that they experienced no crash. Earlier wording
calling the shutdown log an observed crash overstated the evidence. The diagnostic handler recorded
an access violation in Streamline/NGX during LdrShutdownProcess, after exit had begun. No gameplay
crash or user-visible exit failure was established. Retain this as an internal SDK teardown finding;
do not present it as a confirmed game crash in release notes.
