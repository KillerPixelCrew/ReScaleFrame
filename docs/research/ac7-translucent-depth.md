# AC7 translucent depth for camera motion

7 September 2026. Implemented in the research proxy, cross-built with MinGW-w64, and
synthetic-tested under Wine. Deployed for game verification. No AC7 run or DLSS quality
measurement was performed for this change. No MSVC/Windows verification.

## Question and established evidence

Give Streamline depth at the static briefing relief so its existing camera-motion resolve
uses that surface, with the engine's `ClipToPrevClip` and `cameraMotionIncluded = false`.
The task establishes that composed colour already contains the relief, but it retains input
pixelation under the continuously panning camera. No new investigation of that cause was needed.

The relevant recorded build is `Ace7Game.exe`, Steam build 9855922, SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`, D3D11 under Proton/DXVK.
Reference UE4 `4.18.3-release` is commit `0a14a8d537a31ecc77488ced41dbaa0166612ef8`.
See [capture evidence](ac7-frame-capture.md), [velocity research](ue418-hook-map.md), and
[composed colour](ac7-composed-scene-color.md).

The briefing capture `ac7_briefing_frame18425` records RGBA16F layer #46630 at events
1486–2015, 59 draws / 540,030 indices, consumed by recombine #46633 at event 2146.
These IDs describe the evidence, not runtime matching. The 22 examined flight captures
have no corresponding separate layer. Exported action lists omit shader resource bindings;
matching the recombine's actual read is done by the live tap.

Established source facts from the task: separate translucency binds
`DepthRead_StencilWrite`; the relief actor is an `AStaticMeshActor` at
`Nimbus.CampaignBriefingWidget::BriefingMeshActor`, offset `0x0528` in the local SDK;
`AddVelocityStaticMesh` rejects non-movable meshes, and the two-sided translucent cook
has no velocity permutation. The measured blend-gate patch left unwritten flight coverage
at 0.9196. This change does not repeat or extend that experiment.

The older assertion that no velocity necessarily prevents reconstruction is too broad.
For static geometry, correct surface depth allows the existing camera resolve to supply
motion. Independent object motion still needs a separate solution.

## Decision and runtime use

`frame_tap` now shadows all 32 IA vertex bindings (buffer, stride and byte offset), index
buffer/format/byte offset, input layout, primitive topology, VS and all 14 VS constant
buffer slots. The tap ABI is 6. It observes Draw, DrawIndexed and both direct instanced
forms, including start/base/instance arguments. DrawAuto and indirect calls are reported
as unsupported. Other contexts bypass observation. ClearState and command-list execution
without state restoration invalidate the shadow; inherited IA/VS state is read once on the
owning render thread when needed. Context1 binding-range metadata is not recorded.

The geometry callback runs immediately after the game's draw, with hook reentry suppressed.
It reports single-target geometry with depth. AC7 selects the captured RGBA16F layer shape;
the graphics module validates depth and pipeline suitability. No shader hashes, resource IDs,
or game-specific structures enter the graphics module.

Replay happens synchronously in that callback. It uses the original, still-bound vertex shader,
constant contents, vertex/index buffers, vertex resources, layout, topology, viewport,
scissors and rasterizer. It reissues the observed draw arguments, with no pixel shader and
reversed-Z depth writes into a private target. This avoids retaining mutable constant/vertex
buffers for a later replay, where the same allocation could already describe another draw.
The first accepted draw copies the opaque depth; subsequent draws accumulate nearest depth.
Depth values stay in device-depth units, with larger values nearer. There is no depth conversion,
velocity pass, previous-object transform, camera change, or change to the original depth texture.

A narrow scope in `d3d11_state` saves/restores only the disturbed OM targets, depth/stencil
state and reference, and PS including class instances. It leaves input/resource bindings,
viewport/scissors, rasterizer and predication in place. This also avoids the existing broad
helper's limited vertex/resource slot ranges. OM UAVs, stream output, HS/DS/GS, nontrivial
stencil tests, depth-disabled materials, non-triangle topology, unsupported draw forms,
multisampling, arrays/mips, non-full viewports and incompatible depth formats are refused.
An unsupported candidate invalidates that replay object for the frame rather than returning
an incomplete layer. This is deliberately conservative and may decline a real game layer.

Two `R32G8X24_TYPELESS` targets and their writable DSV/depth states are created during
bridge startup on the existing setup worker, before hook installation. Their dimensions
are output size and half output size, covering the recorded 100% and 50% scene sizes.
The layer, source depth and viewport must all match one prepared extent exactly.
Other sizes, including a separately downsampled layer, fall back. No textures, buffers,
state objects, shader compilation, growable draw lists or heap records are allocated by
the new per-frame replay path. At 2048x1152 output, the two depth textures total 22.5 MiB
of texel storage before driver overhead. They live for the bridge session; failed installation
releases them. The proxy has no successful-session teardown path today.

The source depth and layer are retained only until Present ends the frame, to prevent address
reuse. Their contents are not treated as immutable merely because references are held.
The recombine selector retains the exact layer it reads, alongside composed colour. Backend
depth changes only when that frame selects composed colour and the replay matches its layer,
original depth, and context. Ambiguous layer identity keeps the colour selector's existing
behaviour but declines depth substitution. Without composition, the original depth pointer
is passed unchanged, even if a private replay copy exists from this or an earlier frame.
Both Present evaluation and the existing reinsertion gate use this same selection.

The bridge logs its first four depth substitutions and includes candidate draw, replay and
selected-evaluation counters in the existing status report. These distinguish successful
replays from copies actually submitted to the backend.

## Verification and limits

`tests/frame_tap.cpp` drives a real D3D11 device through the installed hooks. Pixel readback
checks a triangle gaining depth 0.6 while its opaque seed remains 0.25 outside the triangle
and the original resource remains unchanged. Updating the same constant buffer before a second
indexed-instanced draw produces depth 0.8 elsewhere while retaining the first result.
Additional checks cover nonzero IA/index/draw offsets, high IA/VS/resource slots, all four
direct draw forms, nearer opaque occlusion at 0.9, scissors, restoration of targets/PS/depth
state/stencil reference, same-frame composition identity, absent/wrong layers and source depth,
frame reset, whole-frame refusal, ClearState, and deferred-context isolation/command-list reset.

The initial fixture used vertex slot 31. System WineD3D reported failed IA getters for slots
16–31 and crashed its rendering worker with that fixture. The final fixture uses slot 15,
still above the broad state helper's eight saved vertex slots, and retains slot 13 constants
and slot 100 shader resources. Production shadow capacity remains 32. This is a recorded test
runtime limitation, not evidence that AC7 uses high vertex slots.

The focused test passes under system WineD3D and under system Wine with the existing DXVK
`v3.1-1-gadeda6639a09ad1`, NVIDIA GeForce RTX 4070 Laptop GPU, driver 610.57.4.
One DXVK launch printed all passing assertions but exited 137 during shutdown; it is not
counted as a pass. The subsequent launch with two compiler threads exited successfully.
Local logs and deployment metadata are under `.local/depth-replay-test/`.

`cmake --build --preset linux-cross-debug` passed with warnings as errors.
`ctest --preset linux-cross-debug` passed all 15 tests in 68.55 seconds, including the
new depth/composition checks. `VERSION` matches the Cargo workspace version and
`git diff --check` passes. No Rust files changed. `eng/verify.ps1` cannot run here
because PowerShell is absent; its Windows/MSVC preset remains unverified.

Still untested: actual matching/replay coverage of all 59 briefing draws; DLSS acceptance and
improved temporal reconstruction with the augmented depth; flight regression; F6 reinsertion;
frame cost; camera cuts, dynamic resolution, and additional game views or pooled-resource
rewrites after the observed layer. The test proves immediate draw replay and depth selection,
not game shader identity or the game's resource lifetime through evaluation.

A null PS reproduces geometric coverage, not opacity, alpha discard, pixel depth offset or
blended multi-layer visibility. Thus a transparent hole can receive geometric depth, and a
single nearest depth cannot describe every overlapping translucent contribution. Material
coverage and actual relief quality need a game capture/live pan. Tessellated, moving,
particle and depth-test-disabled layers are outside this verified contract. Refusals keep
opaque depth; they do not claim those cases are fixed.

## Delivery

After the final shadow-reset cleanup, the focused Wine test passed again (4.74 seconds)
and the final DXVK test exited 0 with all depth/composition assertions passing. The proxy
was then copied to the requested AC7 installation and its hash checked against the build.
Deployed `dinput8.dll` SHA-256: `8e64896fa820d6714f0d7e75ed3f1b1eb8e8934bd427948a37a251f8527b2b22`.
Previous DLL SHA-256: `301220baa297f49d94201c64cdd615ce7b3db3871d414ccf14e0cb4057286567`.
Backup: `.local/depth-replay-test/backups/20260907-120459/dinput8.dll`.
No commit was created. Deployment is not an injected/game-tested result.
