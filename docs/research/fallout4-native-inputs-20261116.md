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

## Reduced producer handoff, source only

The reader now accepts an optional copied NativeDepthSnapshot as its third
argument, after the existing report callback. Reduced inputs require this
same-source post-prepass producer record; the legacy call without it still
requires ratio1 and a full native viewport. The producer separately observes
actual immediate-context RS viewport and bound Main DSV, validates pending
shadow/camera-port agreement and scissor coverage, and reports its integer
sample coverage as LTRB. Later reader RS state is checked for local stability
but does not replace that producer observation. Allocation/output dimensions
remain full output, and ratios are not rounded to invent render dimensions.

Camera projection/VP are adjusted into that integer cropped raster with the
native fractional viewport affine. Existing history stores current/previous
raster maps and resets on missing or changed metrics; jitter and native UV
motion conversions use the actual viewport extent. Target depth transforms
match the same adjusted CameraABI1. Detailed predicates, mathematical limits
and source-only evidence are in
[the depth-domain note](fallout4-depth-domains-20261116.md#reduced-producer-raster-and-canonical-crop-implementation).
The new fallout4_camera_contract fixture exercises actual camera.cpp and is
registered but has not been run here. No game/build/deploy/ratio write or
quality/capability activation was performed by this worker.


## HUDless refusal attribution, 16 November 2026

Retained report `native-raster-20261116-140447-020/loader-world-first-refusal.log`
from the coordinator's PID54248 run records source4733/native5213 passing leased
reader resources, GAMEPLAY screen facts and one DLSS SR publication at
2560x1600. The next message says HUDless frame/window/context/outer-chain
mismatch. That aggregate message was misleading: its first OR term was
`!read_native_framebuffer(frame)`, which could fail before any identity term
was evaluated. The retained message alone does not prove an identity mismatch,
a bad FRAME basis, or the native location/content of another texture.

Source review finds `screen_basis` copied from the accepted pre-SR pass;
`world.pass` copies that same pass through FG_INPUTS, clearing borrowed resource
pointers while retaining scalar frame/window/camera identity. The context and
outer chain remain pinned in World. SR must already match the pre-world FRAME
native frame/view/window and those context/chain pointers. No concrete basis
comparison defect was established, so no check or texture provenance changed.

The guarded Effects0..13 call at21fa283 returns21fa288; native viewport call
21fa2c5 returns21fa2ca and is the existing HUDless callback. Its reader still
resolves logical FrameBuffer0 through RTM2f4d750+df4 and native platform record
RendererData+a58+index*30, leasing the actual RTV resource. It validates
canonical COM device/texture identity, full allocation extent, outer chain/window
and rereads owner/map/target stability. It neither substitutes GetBuffer nor
claims a private view resource is FrameBuffer0.

The source-only followup adds first-occurrence-per-named-stage receipts inside
this existing framebuffer reader, independently of the shared16-message render
diagnostic budget. They expose the first failed map/RTV/resource/device/extent/
swap/owner-reread check and actual scalar/pointer/descriptor values. The caller
now distinguishes framebuffer-read from native-frame, window, context,
outer-chain and output-extent refusal, with pre-SR FRAME/screen native-frame
basis values. Existing exact identity predicates and lifetime remain unchanged.
The exact next fact is the first named framebuffer or HUDless stage from the
next coherent coordinator artifact, not a higher generic log budget.

Source4734/native5214 then failed the independent near-W depth model, which the
native-facts owner handles separately. One SR submission is not sustained SR,
HUDless/FG acceptance, reduced-quality validation, visual quality or latency.
No worker build, game/process query, UI, attach, deployment or input occurred.
The static receipt `hudless-attribution-static-20261116.json` records source
signatures and matched callback bytes; it is not runtime validation.

## Latest trial disposition and followup gate

The full scene/input/marker batch passed
`verify-scene-input-marker-20261116.log`: exit0,221passed15skipped,144.40seconds,
fmt/Clippy clean. Its subsequent PID54248 trial is retained in
`native-raster-20261116-140447-020`, including `disposition.json` and
`loader-world-first-refusal.log`. Reader source4732/native5212 passed;
GAMEPLAY source4733/native5213 reached one DLSS SR evaluation before the
aggregate HUDless refusal described above. No framebuffer/identity comparison
cause was established. Source4734/native5214 then refused near-W. The named
HUDless followup remains source-frozen for the next coherent build, not
runtime-validated by that older aggregate message.

HWND41225678 client2560x1600/DPI144 and actual core-accepted stage5 window
messages do not establish input consumption: no RESERVE/SAMPLED/BIND/ABORT
receipt was observed. World-Off marker-order violations were0 in the bounded
receipt, not latency acceptance. There was no complete SR capture or sustained
SR/FG/quality acceptance. NativeDesktopQuit exitedC0000005; the30second trace
expired before the final action and captured no close fault. All20 entries
were restored, custom INI unchanged and prior user settings SHA13D80689...
preserved, with no forced game kill.

The composed-view-depth helper passed
`camera-composed-view-depth-20261116.log`. The initial newer full batch
`verify-composed-depth-hudless-20261116.log` failed only the overlay SetCursorPos
precondition; `overlay-cursor-refocus-recheck-20261116.log` then exited0 after
normal desktop focus without a source change. Complete
`verify-composed-depth-hudless-r2-20261116.log` remains pending at this freeze.
The named HUDless and CPU candidate-attribution source is not yet assigned a
full-gate or gameplay pass. No new experiment or production edit was performed
for this documentation update.


## Matched Release snapshot-padding defect and repair

The coordinator's `native-raster-20261116-143511-988` PID61128 report now records
sustained SR after the independently owned composed-view Z correction. Its
first framebuffer refusal is `owner-changed`, with before/after native4131,
context1bb716a2110, chain1bb7156efd0 and window111652 equal. Logical0 has a
null optional native texture pointer but a valid leased RTV resource,
2560x1600, format28, one mip/array/sample, with the preceding canonical device
and resource checks passed. That legitimate RTV ownership path is retained.
The log does not dump every after-owner field or the differing object bytes.

Read-only inspection of the actual MSVC Release object and linked Entry proves
an independent comparison defect, rather than relying on padding speculation.
The Entry SHA256 is
`b403df6af9bf8cce128aedc0d698d6beac9f68bf60c47127c752a4749f985f98`, matching
the run's files receipt. The object's read_native_framebuffer section matches
all6011 linked code bytes except49 linker-relocated fields, uniquely at RVA bcd0.
Its before/after snapshots start at RSP+1b0/+2b0. Initializers clear exactly122
field bytes, ending with16-bit stores to RSP+228/+328 at function+152/+1d0;
the six bytes at22a..22f and32a..32f remain untouched. Function+c66 loads
R8D=128; c6c/c74 load the two snapshot addresses; the c7d relocation names
memcmp. Thus a comparison really includes six uninitialized tail bytes in each
snapshot. Retained `reader-padding-proof-20261116.json` and
`reader-object-disassembly-20261116.txt` contain the binary/object provenance.
This proves the emitted padding-read defect, not a live dump of the exact
bytes that differed in PID61128.

The repair compares exactly the fields read by each routine. owner() rereads
renderer/device/context/chain/window/frame/width/height and compares all eight;
it no longer compares padding or unused camera/temporal fields. The pre-SR
state() comparison additionally checks camera/cache/count/TAA/disable/logical
maps, both ratio elements, all four viewport elements, delta and both selector
bytes. Float fields retain exact bit comparison, including signed zero and
NaN payload representation; this change does not introduce numeric tolerance
or loosen state admission. All owner rereads, map/target rereads, canonical
COM/device/RTV/resource, extent and lifetime checks remain unchanged.

The small game-private `native_input_state.h` holds the same snapshot and the
actual two comparison helpers. Registered pure fixture
`fallout4_input_state_contract` uses those helpers, poisons only the real six
padding bytes to demonstrate the old false difference, and negatively checks
every read pointer/integer/array element/float/selector. It also verifies that
owner-only comparison ignores unread non-owner fields while full state does
not. Fixture source is prepared for the coordinator's gate, not compiled or
run by this worker. No new budget, hook, resource substitution, CPU policy,
capability, build, process query, game/UI/attach or deployment was added.
Actual HUDless/FG acceptance and the separate native-depth/input predicates
remain pending the next coherent run.

## Later sustained SR trial and exact HUDless padding finding

Complete `verify-composed-depth-hudless-r2-20261116.log` subsequently passed:
exit0,221passed15skipped,171.65seconds, fmt/Clippy clean. This supersedes the
earlier pending r2 status in this note, not the separate latest padding batch.
The new padding repair is source-frozen, not deployed in PID61128;
`verify-reader-padding-20261116.log` is running/pending at this documentation
freeze and has no result assigned here.

Root's later `native-raster-20261116-143511-988`, PID61128, used Runtime90E86B69...
and EntryB403DF6A.... Actual manual API quality/status receipts show sustained
DLSS SR execution at native2560x1600, Quality1707x1067, Balanced1485x928,
Performance1280x800 and Ultra Performance853x533 to2560x1600, then return native.
The apply files verify only settings roundtrips; effective backend1/result0
status and evaluation logs establish the separate execution scope. Base FPS
remained approximately4to6 even with SR Off, with invalid output FPS; no
performance attribution or visual-quality acceptance follows.

The named HUDless owner-changed refusal now has exact static attribution:
`reader-padding-proof-20261116.json` matches deployed EntryB403DF6A to its
compiled object and proves the128byte NativeState memcmp includes uninitialized
tail bytes. This corrects the earlier unresolved aggregate comparison hypothesis
without changing resource provenance. The semantic-field repair is source-only
in the next pending gate and was not deployed here. FG was not attempted.
Accepted removed-MSG facts and marker-order0 do not establish prospective input;
no reservation acknowledgements were observed. The manual manifest remained
capturing, with no completed capture or full integration/latency acceptance.

The controller600second scene and120second quit waits timed out; late manual
trials/scene-continue are not controller acceptance. Native-Quit input did not
complete. Ordinary AltF4 close was covered by fresh durable trace-close-3,
which captured4CB273 read0x50 with the exact baseline stack and
NtTerminateProcessC0000005. The post-close disposition's restored=false is
superseded by the later exact-run recovery receipt ending84c7a0a118e143d6b24c41db1e459b4d:
restored=true,20/20 verified, custom INI unchanged, root API-test mutation sealed
and original user settings SHA13D80689... restored. Native borderless prefs and
custom INI were preserved. This documentation reconciliation performed no
code, build, game/UI/process query, attach or deployment.

## Padding repair admitted; next refusal is input ownership

Complete `verify-reader-padding-r2-20261116.log` passed223/14 in119.87seconds,
fmt/Clippy clean. The initial221/15/1 XeSS inactive/generated_windows failure
remains recorded; its unchanged focused recheck passed6.90seconds before full
r2. The repair is now built/deployed evidence, superseding earlier source-only
and pending descriptions, with narrow live native framebuffer-reader acceptance.

PID70400 report `native-raster-20261116-151505-988` used Runtime90E86B69... and
Entry463B1EED.... Manifest complete/matchedPresent at source4121/native4598 has
16 captured images among30 entries. HUDless colour is explicitly unavailable
in that FG-Off capture despite the accepted reader. Later same-source FG-on
logs independently show inputs1/HUDless1/identity1/matched1/scene_supported1/
continuous1/render_complete1. They refuse with cpu_mask1/flags0, paced0,
prospective0, owner_generation0 versus provider_generation2 and reservation_read0.
That is missing bound/paced input ownership, not missing resources. The
reserved_bound1 predicate is vacuous for nonprospective sources and proves no
accepted BIND. Removed-MSG acceptance and marker-order0 do not repair that gap.
Foreground1/base54/output54/active0/tags0 is not generated FG or latency acceptance.

World observation was approximately55fps, without performance attribution.
Capture observation completed, but controller quit cleanup timed out120seconds.
Ordinary AltF4 followed Escape not opening the native menu; trace-native-quit's
filename is not a native-Quit receipt. Actual trace captured baseline4CB273
read0x50 stack and NtTerminateProcessC0000005. Recovery receipt ending
3103b81a21864e34ac33e15f1721a1b1 proves20/20 restored, custom INI unchanged and
user settings SHA13D80689... back after sealing the root test mutation; native
prefs/custom were preserved. No full visual/motion/quality, captured HUDless,
performance, FG, prospective input/pacing, latency or normal-close acceptance
is claimed. This followup changes documentation only.
