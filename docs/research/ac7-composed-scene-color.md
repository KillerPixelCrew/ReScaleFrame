# AC7 composed scene colour selection

7 September 2026. Implementation based on repository revision `56ed05b` and the already established
[briefing and hangar evidence](ac7-frame-capture.md#menu-screens-render-two-scenes-and-only-one-is-reconstructed).
No new game run or capture replay was performed for this change.

## Question and evidence

Which texture should DLSS receive when the colour identified by the existing colour/depth/velocity/
exposure binding lacks separate translucency? The missing briefing relief is absent from the input.
Changing motion cannot restore a layer that was never supplied.

The recorded AC7 build is Steam build 9855922, `Ace7Game.exe`, SHA-256
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`, using D3D11 through Proton/DXVK.
The reference source is UE `4.18.3-release`, revision
`0a14a8d537a31ecc77488ced41dbaa0166612ef8`. The established source ordering places separate
translucency recombination at `PostProcessing.cpp:1453` before temporal AA at line 1467. This is a
reference source explanation, not a verified AC7 shader identity.

In the briefing capture, target 1723 holds the base scene, 46630 holds the relief layer, and the
three-index fullscreen draw at event 2146 writes their composition to 46633. That output is
full-size `R11G11B10_FLOAT`, with no depth target. The layer is `R16G16B16A16_FLOAT`. In the hangar,
the corresponding separate translucency is black. Resource IDs identify that capture only.
The exported action lists do not supply shader resource bindings; they were not used to rediscover
reads for this change. The source and capture conclusions above are inherited evidence.

## Decision and runtime use

The graphics tap now has a persistent `rsf_frame_tap_watch_input` query alongside its two existing
render-target watches. It reports draws with the named texture bound as a pixel shader input,
including the output descriptor, viewport origin/extent, depth-target presence, target count,
index/vertex count, and input descriptors. Binding changes refresh the cached read match. Draw
reports use fixed stack storage. Truncated reports are marked explicitly. Frame-tap ABI version is
5; the public game-plugin ABI and `rendering_ready` are unchanged.

The AC7 rule lives in `games/ac7/src/scene_color.cpp`, separate from generic graphics observation.
It accepts the first observed single-hop composition of the identified colour in each frame:

- The draw reads the retained source, an `R11G11B10_FLOAT` texture at the held render dimensions.
- It also binds a distinct RGBA16F input at full or half render dimensions. Half size allows the
  engine's reduced separate-translucency shape; accepting it is a policy choice, not a new capture
  measurement. Black contents still qualify, since the hangar's empty layer must not require readback.
- It draws three indices or vertices to one different, single-sample `R11G11B10_FLOAT` target at
  those dimensions, with no depth target and a full viewport starting at zero.
- It uses the installed context and an untruncated input report. It never follows the composed
  texture onwards through later post-process outputs.

A full-size fullscreen consumer of the source that writes RGBA8/BGRA8 closes discovery for that
frame. This is the captured AC7 tonemap/composite boundary. A later floating-point output cannot
reopen discovery. This rule is specific to the observed AC7 SDR tail, not a general proof that a
floating-point texture contains linear colour or that a shader actually reads every bound SRV.

The bridge arms the watch after accepting the main-view camera and current reconstruction inputs.
It keeps the same watch across Presents. `evaluate_held` selects the composed texture only if this
frame observed its qualifying draw for the held source; otherwise it hands the backend the original
colour. Depth, motion, exposure, camera, and evaluation timing retain their existing paths. Repeated
qualifying input sets with the same source do not erase a composition already observed this frame.

The source and composed allocation are retained until replaced, with at most two extra texture
references. There are no new GPU allocations, copies, staging resources, or heap allocations in
this selection path. Present clears frame validity and the discovery boundary, without releasing
and reacquiring the composed allocation. A source, context, or render-size change releases the old
association. If the recombine happens before the first accepted input set, the first frame falls
back; the persistent watch can observe it before that set in subsequent frames.

The single shadow is now limited to the installed immediate context. Other contexts are forwarded
without observation or substitution, preventing their bindings from being mistaken for this view.
The output-slot-zero read/write hazard is cleared from the shadow when OM implicitly unbinds a PS
input. The reciprocal conflicting PS bind is also rejected in the shadow. This is not full command
list replay, arbitrary MRT/UAV hazard tracking, or a new view-identification system.

## Verification and remaining work

The change was cross-built with MinGW-w64 using `cmake --build --preset linux-cross-debug` with
warnings as errors. The synthetic `frame_tap` test drives real D3D11 draws under Wine and uses the
same AC7 selector called by the bridge. It checks selection of the composed allocation, same-source
pass repetition, no-recombine fallback, per-frame freshness, resource and extent changes, high-slot
inherited SRVs, indexed/unindexed triangles, both OM binding entry points and KEEP semantics,
implicit unbinding, and isolation from a deferred context. Negative sequences cover geometry,
partial viewports, downsampling, depth-bound draws, missing layers, byte-format outputs, later
float outputs past that boundary, and attempts to follow a different source. The test checks
resource identity and lifecycle decisions; its simple pixel shader does not reproduce UE's math.

`eng/verify.ps1` could not run because `pwsh` is unavailable. The Linux cross-build/Wine gate is
used instead. `VERSION` and Cargo both remain `0.1.0`; `cargo fmt --all -- --check` and
`cargo clippy --workspace --all-targets --locked -- -D warnings` passed with the pinned toolchain.

`ctest --preset linux-cross-debug` passed all 15 tests in 69.14 seconds. The test log confirms the
new composition stage ran and reported `frame_tap: pass`; it did not take the no-device skip path.

The cross-built `dinput8.dll` was deployed to the requested AC7 installation after backing up the
previous DLL under the ignored `.local/deploy-backups/` directory. Build and installed SHA-256
both equal `301220baa297f49d94201c64cdd615ce7b3db3871d414ccf14e0cb4057286567`. Backup filename:
`ac7-dinput8-before-composed-20260907T112035Z.dll`. Deployment verifies bytes, not a game run.

Game verification remains open: actual recombine recognition in briefing and hangar; relief and
contour-line recovery in the DLSS input/output; flight fallback and false positives; resolution,
scene, and camera transitions; shader identity and precise valid rectangles. The retained composed
allocation can still be reused or overwritten before Present. Retention preserves allocation
lifetime, not contents. Its last write/use boundary needs runtime validation, as do colour/exposure
conventions and render-thread overhead. No DLSS evaluation, image-quality result, MSVC/Windows
build, latency result, or target-device performance result is claimed for this change.

The F6 reinsertion plan still names the original scene resource. This change selects backend input;
it does not establish the correct composed-colour substitution site or validate F6 in game.
