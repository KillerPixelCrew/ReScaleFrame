# Fallout 4 native resource and camera reader, 16 November 2026

## Question and evidence boundary

The reader must identify the native world view and logical render targets at the
verified imagespace boundary, without treating allocation order or an outer
swap-chain buffer as engine intent. This work uses static PE/Capstone inspection
and source comparison. It has not been built, device-tested or game-tested by
this worker. It establishes a guarded reader, not complete SR/FG support.

The installed executable is Fallout4.exe 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
All addresses below are RVAs. The reference checkout is
`fallout4-community-shaders` revision
`e0e711594291c1a8e447ac758099492514810fbb`, with CommonLibF4
`7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` and its shared dependency
`5408d22adf937cc00e0ccd4bdeef66021172884d`. The community-shaders shared
revision is `6e67083a54090d10806484549d07f75b7fbc8dc2`.
Reference code was studied, not copied. The old 1.11.221 address library supplied
search leads only; no reader address is justified by that old map alone.

The companion [render integration map](fallout4-render-integration-20261116.md)
owns hook placement. Local static receipts include `render-ui-disassembly.txt`,
`WorldBegin-capstone.txt`, `TemporalUpdate-capstone.txt`,
`RenderPreUICandidate-capstone.txt`, `proof-CameraCacheBuilder.txt` and
`proof-UpdateDynamicResolution.txt` under `.local/fallout4-research/`.
`native-input-reader-guards.json` compares all seventeen reader instruction guards
against this executable and records its SHA256. Matching bytes establish the
selected instructions, not the live validity of every field or shader path.

## Native owners and logical mapping

| Fact | Current native evidence and reader use |
| --- | --- |
| RendererData | Constructor `18a246` selects `3a25d10`; publication `18a28c` stores it at pointer `38e13a0`. This is renderer object `3a25d00` plus `10`, not the object base. Device/context/window/chain are data offsets `48/50/58/70`. |
| Graphics State | WorldBegin `21f0e3d` selects static `3d871a0`; frame is `98`, output size `78/7c`, viewport `88`, TAA state `ac/b0`, camera cache pointer/count `140/150`. |
| World camera | Getter `c37dd0` loads graph pointer `32dd338`, then graph camera `140`. Cache finder `182bf20` matches camera key `238` and use-jitter byte `240`, with record stride `250`. Exactly one matching record is required. |
| Colour mapping | RenderTargetManager `2f4d750`, logical-to-platform map `df4`, confirmed at `1839be2`. Main is logical 1, motion logical 32, FrameBuffer logical 0. Each platform target is RendererData `a58 + index*30`, with texture/RTV/SRV at `0/10/18`. Platform indices must be below 101. |
| Depth mapping | Manager map `f84`, confirmed at `1839cbb/183a4a0`. Logical main depth 1 resolves to RendererData `1d48 + index*98`; texture `0`, depth SRV `88`. Platform index must be below 13. |
| Active extent | Manager ratios `fb8/fbc`, with dynamic-resolution writers traced in `1839ed0`; the native-raster reader requires both ratios exactly 1 and one validated full-output immediate-context viewport. State `88` remains diagnostic metadata, not the active raster rectangle. |

An earlier nearby getter candidate `c38090` was rejected: it follows an unrelated
owner through offsets `150/30`. The correct `c37dd0` body and its world-camera
callers distinguish the accepted graph and layout. The reader does not use the
rejected getter or infer a world camera from proximity.

The pre-imagespace reader leases Main, motion and depth through their mapped
native views. It verifies the views refer to the expected textures, canonical
device identities match the immediate context and swap chain, the colour RTV
matches Main, and allocations have the expected full output extent with one
mip, slice and sample. Current admission is R11G11B10_FLOAT Main,
R16G16_FLOAT motion, and R24G8_TYPELESS/R32_TYPELESS/R32_FLOAT depth with a
corresponding supported typed SRV. These are strict source-informed admission
conditions, not observed allocation descriptors. Other formats refuse.

## Camera, units and history

Native builder `182d750` writes view `50`, projection `90`, unjittered VP `110`,
position adjustment `210`, camera key `238` and jitter selection `240`.
History rolling at `182c050` provides previous unjittered VP `190` and previous
position adjustment `228`. The reader copies these values, rather than retaining
native cache pointers after the callback.

Matrices are native row-vector matrices. The camera helper derives unjittered
projection as inverse(view) times unjittered VP. It accepts finite, invertible,
forward-depth perspective projections with ordered finite near/far planes.
Orthographic, reversed or infinite-far projections currently refuse. Native
position rebasing is accounted for in world/view and current-to-previous clip
transforms. Units remain engine units; the host configuration supplies any
verified metres-per-unit conversion.

Actual projection displacement supplies jitter in render pixels, with the
Y-down viewport sign. The later UI path can clear the global jitter, so that
global alone is not used as the rendered displacement. Matrix checks reject
unexpected non-jitter differences. Camera/frame ABI remains 1. Scalar history
resets on the first admitted frame, a frame gap, camera or extent change,
nonmatching previous matrices/origin, or invalid reprojection. An invalid
previous transform gets identity plus RESET, never an invented previous view.

The camera ABI requires engine-measured frame time. The initial callback-QPC
approach was removed because it does not satisfy that contract. The first frozen
reader therefore left this field zero. Subsequent native writer analysis resolves
it without equating input-poll delta with simulation delta:

- OnIdle `c33a60` and `c34e07` select timer object `339f420`, then call `165ba10`
  with elapsed native milliseconds. The elapsed source comes from native counter
  `1661430`, the timer's own origin, and native frequency storage, not a new RSF clock.
- `165ba94` multiplies elapsed milliseconds by the executable float at `29293f8`,
  whose value is 0.0010000000474974513. `165ba9f` writes seconds to timer `10`.
- `165bab0` first writes those seconds to timer `14`. `165baba` then multiplies by
  the native global time multiplier and `165bac6` writes the scaled value to `10`.
  The disabled-timer branch at `165bad3` clears both adjacent floats together.
- Current `339f430` is therefore the engine-scaled delta, also consumed by
  PlayerUpdate at `c34fd7`. Distinct `339f434`, consumed by input Poll at
  `c342a6`, is the timer's pre-multiplier delta. It is still native clamped timer
  time, not an assertion of unconstrained wall-clock time.

`frame_time_seconds` now copies guarded `339f430`, checked finite and nonnegative
and included in the repeated same-callback state snapshot. A genuine zero delta
remains zero. Extra byte guards cover timer ownership, both writes, multiplier
application and conversion constant. The old 1.11.221 ID 2703181 at `33943b0`
was only a search lead, not the current-address proof. Static receipts are
`native-timer-update.txt`, `native-timer-owner-xrefs.json` and
`native-timer-storage-xrefs.json`. This is current-binary semantic proof, not a
measurement of timer behavior during pause, slow time or a moving game.

The studied DFPrepass motion convention is current-minus-previous NDC encoded
through `(-0.5,0.5)`. The adapter describes conversion to current-minus-previous
UV using `motion_to_uv={-1,-1}`, camera motion included, no jitter contribution.
This convention needs confirmation against active shipped shaders and raw data.
Linear pre-tonemap Main is a pipeline fact from the chosen seam, not an inference
from its floating-point allocation. No exposure or temporal mask is fabricated.

## Synchronous adapter contract

Call `verify_render_inputs()` during cold preparation after executable
recognition. A failed fingerprint or any instruction guard prevents all reads.
`read_native_inputs(NativeInputs&)` belongs immediately before native
RenderEffectRange at `21fa283`, before any SR mutation. `NativeInputs` owns
temporary context, chain and texture references and contains a copied render
pass/camera. The caller supplies verified session, source, submission, scope,
screen, pass role and CPU/eligibility facts; this reader does not choose them.
The native frame and camera/window identity must match the caller's source.
The host must synchronously capture depth and motion for FG before SR writes.

After the viewport call returns at `21fa2ca`,
`read_native_framebuffer(NativeFramebuffer&)` resolves logical FrameBuffer0
through the same platform map and leases its RTV resource, context and chain.
It does not use outer `GetBuffer`, which can name a different allocation with
external wrappers. It returns native frame/window keys for comparison with the
retained pre-SR scalar identity. This reader does not require a world camera or
TAA state in the UI phase. The caller owns the explicit FINAL_SCENE/WINDOW
lease through native Swap and ends it there. AddRef retains allocation, not
frame contents; no resource may silently become a later frame's input.

Each read checks owner identity again after collecting its snapshot. The
pre-imagespace read also rechecks camera data; the framebuffer read rechecks
the logical mapping and target record. These guards complement the verified
callback lifetime, rather than making arbitrary concurrent native reads safe.

## Coverage and remaining validation

| Path | Established here | Remaining evidence |
| --- | --- | --- |
| World opaque/deferred | Logical motion/depth ownership and source motion convention | Native active shader variants, vector values, disocclusion and moving-camera capture |
| Skinned/instanced/terrain | No substitute vectors introduced | Previous transforms, bones, deformation and material coverage |
| Transparency/particles/water/volumetrics | No fabricated mask or zero-vector fallback | Producer coverage, compositing, depth and history behavior |
| HUD and imagespace | Separate logical Main pre-imagespace and FrameBuffer0 after imagespace contracts | Actual texture descriptors, contents and subsequent UI/Swap lifetime |
| Reduced resolution and auxiliaries | Ratio and viewport reads only | Engine resolution ownership, jitter changes, bloom/exposure/DoF/reflections and output reinsertion |
| Loading/pause/VATS/multiview/resize | Conservative reader refusal/reset | Native lifecycle and per-view acceptance |

Next checks belong to the integrated coordinator batch: compile the adapter
against game API 22/host API 8, validate camera arithmetic and instruction guards,
then obtain bounded native descriptor/rect/camera/resource-content receipts at
the two real callbacks. Complete temporal admission still needs source/input ownership and live
validation of the mapped native timing state. No readiness flag, visual quality, FG output,
latency benefit or successful game injection is established by this source work.

## Optional auxiliary leases, later 16 November follow-up

`NativeInputs` now also exposes optional texture references `refraction_normals`,
`pipboy` and `vats`, mapped from logical IDs 14, 30 and 15. Logical 30 is
GBufferAmbient carrying a Pip-Boy material-class lane, not dedicated Pipboy60;
logical 15 is MenuBackground reused for VATS coverage. Their names describe the
intended native consumers and do not prove an active producer this frame.

Each auxiliary follows the same native logical-to-platform map, retains the
actual RTV resource, checks identity against the target texture and immediate
context device, and rechecks its map/target record. Only one-mip/slice/sample,
full-output allocations are admitted, so the existing Main render/output rects
provide the crop context. The reader does not infer formats from target names,
compute shader data, or invent a crop for a differently sized allocation. The
adapter must inspect actual descriptors for generic auxiliary-service support.
Absent, unstable or incompatible resources leave only that optional reference
null and do not invalidate otherwise valid Main/depth/motion. Native feature
requirements determine which null resources require refusing a consumer.

These leases must be consumed synchronously before SR changes Main or auxiliary
contents. Depth1 and motion32 remain the existing required pre-SR leases. Actual
producer coverage, image-space consumer lifetime, restoration versus later native
writes, and feature-specific jitter interpretation remain the adapter hook audit's
responsibility. No GPU processing or vendor policy was added to this reader.

## Active viewport correction after the world trial

The coordinator's visible-world trial PID23856,
`native-raster-20261116-105111-944`, reached reader stage `viewport-bounds`
at native frame5382. The verified owner and 1280x720 extent were present, world
camera/cache count5 were valid, TAA1/disable0 and ratio1/1 were reported, but
State `88` contained four zeros. This contradicts the earlier assumption that
that metadata field must contain the full normalized world viewport. It does
not justify interpreting zero metadata as full raster. No texture-format or
camera-math guard had been reached, so an ENB Main-format explanation remains
unproved. The trial still had no SR admission, ended with `C0000005` on close,
and all20 staged entries were restored; no close-stack repair is claimed.

Static native tracing identifies the actual viewport producer and consumer:

- `1839ec0` writes RTM `fe4`, selected as true at WorldBegin `21f0ae6` and
  changed after imagespace at `21fa2c5`.
- `1839e34` reads `fe4`, passing the derived default/dynamic selection to
  `1817ef0`. That builder obtains active target dimensions, applies the current
  Context normalized rectangle at `6d0..6dc`, and copies depth bounds from
  `6e0/6e4`. It writes a six-float D3D viewport into Context shadow `90..a4`.
- `182430d` selects that `90` record and `182431c` calls Context vtable slot44,
  `RSSetViewports`, with count1. This path does not consume State `88`.

Bounded disassembly receipts are `native-viewport-consumer-20261116.txt` and
`native-rs-viewport-submit-20261116.txt` under the existing ignored research
folder, alongside the retained viewport-default/dynamic-resolution receipts.
Four new expected-byte guards anchor the RTM selection, Context normalized
bounds use and final viewport submission. Reader executable guards now total21.

The reader therefore retains State `88` and RTM `fe4/fe5` as diagnostic facts,
while admission uses `RSGetViewports` on the same canonical-device-validated
immediate context at the real pre-SR callback. Exactly one viewport is required,
all six components must be finite, origin must be `(0,0)`, width/height must
exactly match native output, and the initial implementation required depth range
`[0,1]`. The follow-up correction below replaces that last-pass assumption. It rereads this
viewport before returning and refuses a changed or incompatible result. Both
native resolution ratios must be exactly1. Thus subviews, multiple or missing
viewports and reduced raster remain refused; no
missing metadata or resource is substituted. Existing camera/cache/resource,
view/format/device identity and repeated-state checks remain in force.

This is a source-only repair. Scoped whitespace and all21 executable guards
were checked offline; the coordinator still owns compilation and the next
combined reader-refusal plus post-world/close-fault observation. Live RS
viewport values and the next reached texture/camera stage are not yet claimed.

## Native pass depth bounds, 16 November 2026

The next coordinator trial, PID27344 in
`native-raster-20261116-110223-800`, admitted native frame5599. Main format26,
motion34 and depth44/SRV46 were all1280x720 with valid identities and camera
projection. Later frames, starting5601, retained the same full raster but reported
RS viewport depth bounds `0.01,1`; the strict endpoint check refused them. Public
effectiveSR1 records an evaluation path, not image quality or completed F10
capture. The separate close fault remains outside this reader repair.

The exact executable's Context setter `182c4e0` writes caller XMM2/XMM3 into
shadow `6e0/6e4` at `182c51b/182c523`. Camera upload calls that setter at
`182dd4d`. A native draw block with unwind-chain entry `21f4490` saves both
bounds at `21f4901/21f4909`, sets its camera viewport at `21f4911`, and restores
the saved bounds at `21f4d85`. Its selected minimum comes from `3e62194`, written
by WorldBegin at `21f0e44`; another range scalar `3e62190` is read by pre-UI
setup. Thus the viewport builder and final RS submission described above consume
mutable pass state, including restored prior bounds. The live `0.01` endpoint is
observed; the numeric contents of the initialization globals in disk BSS are not
static proof of a literal `0.01`, nor has this draw block been conclusively named
as the last first-person draw. This distinction avoids inventing a producer from
an address or attributing every stored pixel to the last bound viewport.

Bounded aligned native receipts are
`native-depth-range-producer-20261116.txt`,
`native-depth-range-writers-20261116.json` and
`native-depth-range-foreground-20261116.txt` in the ignored research directory.
The reference first-person-alpha anchor resolves `21f963d` to `21d7120`; it is
not the `21f4490` block, so those two have not been conflated.

Pinned CS reference `package/Shaders/FO4/Depth.hlsli` distinguishes first-person
depth at or below0.01 from world depth and remaps their projection intervals.
`src/Render/CanonicalDepth.cpp` and `CanonicalDepthCS.hlsl` reconstruct a separate
R32 product using the first-person inverse projection and world projection.
`Engine.h` reads that first-person inverse from Context camera data at offset8a0.
Those are CS processing contracts, not proof that the shipped native D24 texture
already contains canonical world depth. They explain why last-pass RS endpoints
are not a whole-texture encoding declaration.

The minimal reader correction accepts a finite normalized nondegenerate depth
interval, `0 <= MinDepth < MaxDepth <= 1`, while still requiring exactly one
viewport, origin0/0, the exact full native width/height, ratio1/1 and an unchanged
same-callback reread. All resource, camera, identity and history checks remain.
There is no depth conversion, resource substitution or reduced/subview admission.
In particular, raw native partitioned depth plus the current world projection
still needs a separate producer/conditioning audit before whole-depth, weapon
coverage or visual SR/FG correctness can be claimed. A last-bound interval must
not be used to normalize the entire texture.

Validation here is source-only: all21 existing executable byte guards and scoped
whitespace checks pass offline. The coordinator owns the focused rebuild and next
world/F10 observation; no new game or device run was performed by this worker.
