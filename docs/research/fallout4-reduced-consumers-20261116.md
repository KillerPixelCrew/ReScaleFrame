# Fallout4 reduced-resolution consumer prerequisites

## Question, method and acceptance boundary

Determine the smallest adapter handoff that preserves VATS/menu contents,
imagespace triangles, native resize and lens-flare behavior when SR changes
active extent. This is static research, not activation of reduced quality.
Production rendering code remains frozen. The executable is Fallout4.exe
1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
Reference CS revision is `e0e711594291c1a8e447ac758099492514810fbb`, with
CommonLibF4 `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60`.

The approach follows native instructions from verified render seams and native
MSVC RTTI rather than trusting old address-library deltas. Capstone decodes
complete chained-unwind families; PE reads confirm RIP targets and constants.
Windows D3DDisassemble inspected retained DXBC bytes offline, without creating
a device. The missing upstream REA session was not reopened. No build, fixture,
game, deployment or production edit was performed by this worker.

The first coordinator native-raster trial
`native-raster-20261116-093321-487`, PID48088, prepared/started and admitted the
scoped outer chain, then refused physical creation twice with
DXGI_ERROR_INVALID_CALL `0x887a0001`; exit was `0xc0000005`, observation was not
completed and21/21 files were restored. Shared startup investigation owns that
failure. It supplies no SR, reduced-quality, FG, latency or visual acceptance.
The prior coherent Windows Release gate remains221 passed/14 skipped with
clean fmt/Clippy. Later trials are outside this note's evidence.

## Native VATS consumer and preservation boundary

| Proof | Exact native evidence |
| --- | --- |
| Actual class identity | ImageSpaceEffectVatsTarget TypeDescriptor30ded18, COL2aee818 signature1/object offset0, vtable2914550 |
| Effect index12 | Constructor21dafd0 called2187750; resulting object stored manager.effectList data+60 at21877a6 |
| Logical15 input | Setup21db0c0 allocates5 texture entries; index2 selected21db221, EDX15 at21db21c, setter22533b0 called21db228 |
| Setter meaning | 22533b0 writes renderTarget+10, clears NiTexture+8, sets depth/stencil+14/+18 to-1 |
| Child input/output association | Setup21db625 calls2254dd0 with input indices1,2,3,4;21db645 calls2254620 with output index0, which is stored in the parent output-index array |
| Synchronous consumer | Native inherited Render2253d50 calls texture binding2254120, child virtual Render and restore/return-textures before returning |
| Native activation | Vtable slot8 points21a8230, reading byte[this+8] |
| Following effect13 | ImageSpaceEffectFullScreenColor vtable2917d48; ctor223e2d0 called2187b58 and stored effectList[13] at2187ba6 [array+68]; setup223e370 has two chain slots, not a logical15 auxiliary |
| Pixel resources | Exact native ordinal3819 reads mask t1, image t0, effects t2 and scan t3; only SV_Target0 output, no UAV |

The native range2185420 iterates the selected0..13 effects synchronously and
calls2185610 for active effects. Its RenderEffect helper assigns temporary
chain textures0/1 before Render and clears those bindings afterward. VATS's
persistent logical15 is a separate parent input2, not that chain output.
Its parameter update21db690 calculates texel size from native target dimensions
multiplied by RTM width/height ratios, so the ratio change and full-output mask
must agree during its actual use.

This establishes the ordinary native VATS read inside the selected imagespace
range, followed by the two-chain-input full-screen colour effect. The source
reference independently restores its temporary logical15 swap immediately
after RenderEffectRange, with whole-RenderUI cleanup as a fallback
(TemporalRenderHooks lines200..211 and273). The smallest existing-service
candidate is therefore an in-place nearest auxiliary transaction for15 with
RESTORE_CONTENTS, restored immediately after the unchanged native range call.
Other auxiliaries can use flags0 so the same restore retires snapshots while
preserving their full-output contents for later consumers. No new allocation
or native target-table swap is established as necessary.

This is not a proof that arbitrary modded effect graphs cannot write15. Before
activation, retain and compare logical-map/texture/device/frame identities,
confirm actual descriptors and producer/consumer use, and do not overwrite an
intervening foreign write. The static default graph and source mechanism
support the bounded range lifetime; they do not prove every game/menu/mod path.
No persistent VATS resource is modified in the current native-first batch.

## Dynamic triangle and native output ownership

Native RenderEffect2185610 selects partial manager+40 when manager+48 is set;
otherwise effect+A8 plus RTM+FE4 selects manager+38, and the other branch selects
manager+28. FE4 is the viewport selector, distinct from FE5 activation.
At2185716/218571a it loads the selected geometry and calls effect Render.

Native producer2186110, `void(ImageSpaceManager*)`, loads manager+38, obtains
its geometry buffer, writes half fields+6/+8 from twice height/width ratios,
and commits through native buffer helpers16e4e50/16e4eb0. The multiplier at
2929498 is exactly float2. RenderPreUI calls UpdateDynamicResolution1839ed0
at21f9f2f, then loads the manager at21f9f34 and calls2186110 at21f9f3b.
Creator218d3c0 likewise initializes dynamic geometry and publishes manager+38
with native refcount increments/decrements at object+8. These are real native
producer/ownership facts, not source names inferred from a delta.

Resetting FB8/FBC after SR cannot change those already baked UVs. A correct
output scope must also arrange full-output geometry, then restore native
ownership at its final consumer. CS's ImageSpaceTriangleSwap substitutes the
unit NiPointer during the chain and restores after whole RenderUI. An adapter
must preserve manager identity, unit/dynamic/partial pointers and native
reference ownership; a blind raw pointer overwrite is not an ownership proof.
Calling the identified native geometry producer after a ratio change is an
alternative research lead, not yet permission for manual reentry or a claim
that its buffer mapping is safe at that different phase. No triangle mutation
or new GPU/proxy framework is added here.

Native UpdateDynamicResolution1839ed0 writes ratiosFB8/FBC and FE5;
SetEnableDynamicResolution183a2d0 instead owns the FE8 counter. That counter
must remain native. Existing ratio consumers are widespread, including native
VATS parameter calculation and imagespace geometry updates. CS replacement
shader proxy requirements do not establish a need to replace vanilla's native
pre-SR allocation/sampling framework. That question remains for live reduced
input/consumer evidence after the post-SR handoff is correct.

## Resize and window lifecycle are now instruction-proven

| Native entry and ABI | Proven operations and boundaries |
| --- | --- |
|18179c0, `void(Renderer*,u32 windowID)`|Five chained unwind parts through1817d62; GetWindowRect, chain GetFullscreenState, release old native backbuffer views, ResizeBuffers at1817b9c, rebuild1825120 at1817ba2, target-create callback at1817bbf, then native RT0 view republication1817bd0..1817bea and dimensions1817d40/45|
|1817d70, `void(Renderer*,u32 windowID)`|Five chained parts through1817ee9; GetClientRect, chain GetDesc/GetFullscreenState, conditional SetFullscreenState at1817e67 and ResizeTarget at1817e80, preserving native window-style operations|
|1825120, `void(u32 windowID)`|Resolves current chain and GetBuffer; creates SRV/RTV/UAV at native renderer-data window slots, obtains texture descriptor, updates native dimensions and invokes1839110|

The receiver of the first two methods is Renderer, whose RendererData starts
at+10. Thus their HWND+68/chain+80 and window stride50 correctly correspond to
creation's RendererData HWND+58/chain+70. Treating those as the same base would
misidentify ownership. Native call181838b invokes WindowSizeChanged, then
18183a3 ResetWindow; alternate18183cc invokes ResetWindow independently.
Import-directory confirmation corrects an initial worker message calling the
first API GetClientRect:1817a05 imports GetWindowRect through2440ac0, whereas
ResetWindow1817dae imports GetClientRect through2440b78. Do not conflate native
outer-window dimensions, physical client bounds and actual texture dimensions.
ResetWindow also uses GetForegroundWindow, IsIconic, ShowWindow and SetWindowPos.
This confirms the source distinction that ResetWindow need not include the
render-target creation callback.

The create callback executes before WindowSizeChanged republishes native RT0
views into the main table. Future adapter invalidation should drain any owned
auxiliary/output leases before rebuild, preserve the complete native calls,
and reacquire target/camera/dimension state after the whole resize function
returns. A target-create callback alone is too early to certify fresh RT0.
No resize hook is activated in this batch and no native resize is suppressed.
Candidate2233e60 is now identified as the target allocation factory, repeatedly
building descriptors and calling1839130/1839390. It is not a VATS last-consumer
function. Initial old-delta names and short .pdata-only views were search
leads, not semantic proof; chained-family decoding supplied the missing bodies.

## Lens flare is a genuine pre-SR depth consumer, exact fix still bounded

Native initialization C39367 loads callback21c13d0, then C3936e calls setter
21fa730. The setter stores it in3e62290. Forward21f9100 invokes that exact slot
at21f96a5 with camera from3e62210; RenderPreUI calls Forward at21fa036,
before the later SR seam. A direct-E8-only scan missed this callback and was
not used as absence proof.

Callback21c13d0 spans eight chained unwind parts through21c16fa. It acquires
logical65 at21c141a, calls native visibility effect index204 from the actual
ImageSpaceManager effect array (array+660) at21c1447, performs camera/light
sprite work, and releases65 at21c167b. The sprite image pass uses effect200
(array+640). Distinguish that image pass from depth visibility.

Native LensFlareVis pixel shader ordinal3904 declares t0/s0 and CB2 light
positions. It samples t0 at light.xy plus fixed UV offsets and compares the
sample against light.z; no RTM ratio constant is declared in this shader.
Native sprite shader3896 instead samples the flare image t0 and loads visibility
from t1, so these are distinct resources. The reference shared LensFlare shader
replaces VISIBILITY only and explicitly adjusts screen positions by dynamic
ratios. Its TemporalRenderHooks wraps lens-flare rendering with a full-depth
override when reduced, rather than inheriting a general SR-only assumption.

The native pre-SR ordering and shader sampling are proven. This investigation
does not yet prove the live t0 native resource/layout or whether CPU-populated
CB2 coordinates already account for the active allocation. Therefore it does
not authorize a shader replacement, force a full-depth copy, or claim a missing
native ratio fix. If live reduced depth is a cropped region in a full allocation
and CPU constants are full-output UVs, this pre-SR consumer needs a scoped
readable full-depth view or equivalent native-coordinate repair before its
callback. The existing post-SR auxiliary transaction alone cannot supply an
earlier read. That conditional gap is precise; a new API is not yet justified.

## Exact shader corpus and remaining producer coverage

Coordinator offline inventory found one uncompressed Shaders011.fxp entry in
the v8 GNRL BA2. Archive SHA256
`6fecbd744f0f3235dea45dda4c259abd90fb599897ecb04226c9916a723889d3`,
FXP SHA256
`13569a497f9ba091a2be293859bcf5585ad8faa8fc1d3ad74991b8512eea125b`.
There are3939 valid DXBC containers, no rejected candidates: VS1339, PS2357,
HS69, DS69, CS105. All3801 VS/PS/CS entries match both raw SHA1 and full-corpus
ordinal in pinned CS stock-shader-identity-AE.tsv;138 HS/DS remain unmapped by
that table. The first compressed-ordinal assumption failed with KeyError2463;
including tessellation in the ordinal fixed the join and validated all mapped
entries. Receipts are native-shader-container-inventory.json and
native-shader-reference-join.json in the ignored Fallout4 research directory.

Matched native VATS3819 SHA1 `7f5fa325b71ee6bac39abebc81914e8c9f6f54f8`,
LensFlare3896 SHA1 `10f2c847a65e99d371be283323b4cf7ccbc05376` and
LensFlareVis3904 SHA1 `3431a18227b93dddc508e1b80312b72fcfdc7b9c`
were disassembled offline in this follow-up. Family/descriptor identities can
now guide motion and depth producer audits without guessing shader provenance.
They do not prove live bindings, constants, history, material/draw coverage or
object/deformation vectors. Those remain real capture/producer-audit gaps.

## Smallest next wiring and generic gaps

After startup and native-AA capture pass, use existing synchronous auxiliary
transactions for nearest motion/PipBoy/VATS and depth-stencil, with VATS
RESTORE_CONTENTS at the selected range return. Reacquire every logical target
per use and condition restoration on the admitted session/native-frame/view/
thread and unchanged engine owner. Keep output geometry/ratio/jitter restoration
separate from resource snapshot retirement, preserving later native writes.
Add resize invalidation only around the proven complete native functions.

The existing generic colour path is already nearest; the earlier linear claim
was corrected in the main integration note. Native allocation factory2233e60 also supplies a concrete format lead:
logical14 is passed to1839130 at2233ecc with width/height and descriptor
format26 (R11G11B10_FLOAT, stored2233eab). Existing generic colour_format
does not admit that format. If the live ENB descriptor confirms the native
default, adding supported numeric-format validation for26 is a small generic
implementation change, not a new ABI. Do not silently append an unsupported
optional surface and let it refuse the whole transaction.

Reference depth/refraction use linear sampling, which the current nearest
colour/depth contracts cannot express. If capture comparison establishes that need, the minimal generic
request is opt-in linear colour in the existing surface kind or flag, preserving
legacy nearest and transaction/restore semantics. Linear depth additionally
must retain stencil behavior and needs its own observed justification. Native
R32 descriptors have not been observed and are not a reason to block R24.
Lens flare's earlier read remains a separate conditional gap above. No API,
proxy framework, persistent VATS replacement or shader patch was implemented.

Retained proof receipts are `.local/fallout4-research/proof-static-*-20261116.txt`,
vats-rtti-vtable-proof.json, fullscreen-color-rtti-proof.json and
native-shader-{3819,3896,3904}-disassembly.txt. Some initial arbitrary-window
listings began mid-instruction, and the guessed22533e0 entry did not decode a
valid setter; neither was used as semantic proof. Chained .pdata and RTTI entry
starts replaced those failed approaches. The default Inkscape Python lacked
pefile; the installed Python314 interpreter supplied existing pefile/Capstone
without installation. Scoped documentation JSON/diff checks are the only
validation run by this worker.


### Priority change after the startup trace

The coordinator supplied a later durably armed PID58104 startup AV and asked
this worker to pause further reduced proof. Results above were retained before
switching to the [native startup audit](fallout4-native-startup-av-20261116.md).
No reduced-quality code was activated or changed while startup diagnosis ran.


## Reduced admission follow-up: actual viewport and jitter

This follow-up resumes static reduced-quality research while the native-depth
and generic owners finish the next batch. Production hooks, readers, camera,
plugin and CMake were not edited. PID37012 remains the older deployed trial at
the coordinator's last receipt; this worker did not query, attach, restore or
interact with it. Exact PE reads below use the same installed SHA stated above.
They establish native arithmetic and snapshot requirements, not live reduced
rendering or motion coverage.

### Native viewport builder does not round the ratio product

RTM viewport selection1839E00 obtains the native TLS/fallback Context and passes
its shadow base to1817EF0. At1839E34, FE4 determines the fifth argument: FE4=0
selects default scaling; FE4!=0 permits FB8/FBC. The builder preserves float
arithmetic throughout its viewport writes. For normalized camera port
`[left,right,top,bottom]`, target dimensions W/H and selected scale sx/sy:

- TopLeftX = W*left; TopLeftY = H*(1-top).
- Width = W*(right-left)*sx; Height = H*(top-bottom)*sy.
- MinDepth/MaxDepth are copied from shadow6E0/6E4.

The scale affects extent, not the origin. Normal world ports `[0,1,1,0]` make
origin0. In the ordinary no-explicit-size branch,1817F3E/1817F47 load FB8/FBC;
1818013/181803A multiply them before stores181801E/1818045. No floor, ceil,
integer conversion or alignment appears on this product path. Explicit R8D/
R9D sizes skip ratio loading and use scale1. Bound cube target selection uses
shadow54/8C and descriptor DD0/DD4 through1839A40/1839A90. Otherwise getters
1839830/18398A0 use current logical color index shadow84 and RTM descriptor
stride20; if absent they use positive depth index shadow88 and descriptor
C80/C84, stride1C. These are native logical descriptor dimensions, not physical
renderer-array strides30/98 or evidence that a cube/foreign viewport is Main.

The resulting six floats occupy shadow90..A4, which182431C submits as exactly
one D3D RSSetViewports record. Shadow metadata and actual last submitted RS
state must be distinguished:1818062 marks dirty bit2, and a camera viewport
change can leave the new shadow record pending until native state application.

### Quantization is earlier, and camera changes can truncate again

The native UpdateDynamicResolution tail, before the float viewport builder,
quantizes each positive reduced dimension to nearest multiple8 with half-up
rounding. Width sites183A159..183A1B1 and height183A1E5..183A23E multiply
output dimension by ratio and0.125, truncate to integer, add1 if the fractional
part is at least0.5, multiply by8, then divide by output dimension. Constants
are2485154=float0.125,2929448=float0.5 and29294CC=float8. Ratios are clamped
to1. If still below1,183A1C7/183A254 subtract float1e-5 from24A5A18, bits
`3727c5ac`. FE5 is then derived from the final ratios. Earlier adaptive updates
also mask ratio mantissa bits at183A0A8/183A0C7 and183A104/183A113; FD8's
positive override at183A131/183A13A still flows through the common quantizer.
Reading the post-update values avoids reimplementing all those native policies.

Separately, camera viewport setter182C4E0 stores normalized port/depths, recovers
base dimensions from the current shadow viewport divided by port extents, and
uses CVTTSS2SI at182C581/182C592 before calling1817EF0 with explicit dimensions.
Thus a subsequent camera change can truncate a fractional viewport; it is not
safe to describe all native paths as the same integer rounded ratio product.
The prepass finally restores world camera at21F3DB6 and returns without another
D3D draw, so its new shadow rectangle may differ from its last submitted RS
rectangle. The final pending record alone does not prove the earlier raster.

Offline float32 examples reproduce only those inspected operations:

| Output and requested ratio | Quantized nominal | Float builder extent after native bias | Camera-setter truncation example |
| --- | --- | --- | --- |
| 1920x1080,2/3 | 1280x720 | 1279.980835x719.989197 | 1279x719 |
| 1280x720,2/3 | 856x480 | 855.987183x479.992798 | 855x479 |
| 1920x1080,1/2 | 960x544 | 959.980774x543.989197 | 959x543 |

These are not measured game extents, and the truncation column is not an
assertion that every world draw takes that path. They disprove treating
`lround(output*ratio)` as an observation. Integer sample coverage at a fractional
viewport edge, scissors and any intervening target rebuild must be accounted
for before choosing the integer crop; none is certified by this scalar table.

### Smallest copied producer snapshot

Extend the existing post-prepass snapshot rather than create another source or
hook ledger. At exact caller return21F9F78, under the admitted ordinary frame,
copy the existing CPU/native-frame/view/owner identity together with:

- Context normalized port6D0..6DC, six-float shadow viewport90..A4 and dirty bit2.
- Current color/depth/cube logical selectors84/88/8C and cube binding54, their
  guarded logical descriptor dimensions, FE4/FE5 and the final FB8/FBC values.
- Exactly one actual RSGetViewports record from the same leased native immediate
  context, and matching bound depth/resource identity where used for admission.
- Cached world/near camera projection, native jitter fields and output dimensions
  already copied by the depth producer.

The submitted RS record is a real active pixel viewport at that producer return;
its provenance still has to match the Main world/depth owner and source. A dirty
shadow record is a pending next state, not a substitute for it. Repeated owner,
ratio, camera and native-frame checks must hold through SR, but the later
pre-SR RS state need not equal the producer's world depth interval: foreground
work already proved why a last-bound MinDepth0.01 is not a whole-source summary.
If shadow/RS extents disagree or the last submitted viewport belongs to a foreign
or ambiguous pass, refuse that reduced publication and retain both receipts.
A more precise guarded world draw/bind snapshot would then be necessary; finite
numbers or full allocation dimensions do not close that ambiguity. This is the
smallest proposed observation, not an assertion that every reduced path already
satisfies it. Preserve actual fractional viewport values in each depth-domain
descriptor even when the validated sample rectangle is integer.

### Native jitter units and CameraABI1

Temporal update182D580 generates the native8-phase sequence from State+0 and
Halton bases loaded at182D5A8/182D5DC. It stores, before camera-cache building,
`ox=(2*H2-1)/State.backBufferWidth` and
`oy=(2*H3-1)/State.backBufferHeight` at State+4/+8. Width/height reads are
State78/7C, not screen80/84 and not reduced pixel counts. Camera builder182D750
then divides these offsets by FB8/FBC at182DA48/182DA5B and postmultiplies the
projection by that NDC translation. The actual cached displacement is therefore
`dx=ox/rx`, `dy=oy/ry`; the unjittered VP was cached before this operation.
Using State+4/+8 directly as reduced NDC would understate displacement by the
scale. Conversely dividing the already cached projection displacement by ratio
again would double-correct it.

CameraABI1.render_width/render_height must describe the independently validated
integer input sample rectangle, while output_width/output_height describe the
full output allocation/window. They must not both become the reduced size or
both stay full while a reduced-quality claim is made. The current reader already
overrides camera output dimensions with full native dimensions after camera
conversion; the blocking ratio1/full-viewport gates are deliberate, and its
otherwise unreachable lround calculation is not a future proof of reduced size.
For the adapter's current row-vector convention, actual pixel jitter is
`(+dx*viewportWidth/2, -dy*viewportHeight/2)`, using the producer's raster metric.
When that metric equals the validated integer render extent, the existing
camera conversion can use that render size directly. Fractional or changed
metrics require explicit reconciliation with crop/sample coverage, not silently
multiplying cached NDC by full output size. Keep copied native camera/reset/
history facts and continue reporting them; do not select a vendor jitter pattern
or fabricate simulation/input timing in this adapter.

### Triangle and consumer consequences, minimum next edits

Native2186110 consumes manager+38, writes packed half UV extents from twice
FBC at218616D/2186175 and twice FB8 at2186186/218618E, and commits through
16E4EB0. The helper770110 rounds normal half values to nearest-even (biasFFF
plus retained low bit at77019A..7701A8). The already baked UVs survive a later
ratio reset. Existing unit/dynamic/partial selection28/38/40 and native pointer
ownership must therefore participate in the scoped output handoff as previously
proved, alongside FE4/FE5, ratios, viewport and jitter restoration. No proxy
geometry or copied reference framework is necessary merely to report this fact.

The concrete next implementation is to add those copied producer fields and
checks to the existing native snapshot; consume them in the reader/camera and
depth descriptor instead of full-allocation assumptions; then wire the existing
auxiliary transaction plus scoped native output state and proven consumer
restoration boundaries. Reconcile requested quality dimensions with actual
native8-pixel quantization through the existing generic planning handoff. Do not
remove ratio1/native-quality gates until that whole path is coherent.

Maintain two different depth products. Vendor SR/FG receives the normalized
crop produced from the depth-domain descriptor. Native later consumers instead
need nearest expansion of the original partitioned device-depth/stencil values,
with their raw near/world intervals intact. Existing depth_scale uses Load and
preserves stencil bits; copying normalized vendor R32 back into the native depth
would destroy this contract. Motion/PipBoy/VATS discrete fields can use the
existing nearest colour path; retain VATS15 RESTORE_CONTENTS at its proven last
consumer. Optional refraction/normals14 has native R11G11B10_FLOAT format26,
which the current generic colour whitelist still lacks. If the actual leased
optional descriptor matches that default, admitting the numeric format is a
small generic implementation change, not another ABI or proxy allocation.
Missing/unsupported optional surfaces must remain independent of Main admission.
Linear refraction is a source preference, not yet a measured requirement.

CS's proxy passes replace resource sizes, shaders and geometry assumptions for
its own replacement shader framework. They are not proof that unmodified native
DR needs identical proxy swaps. Native G-buffer/deferred draws already have the
FE4-scaled viewport and native triangle path. Preserve the native path first;
prove any remaining pre-SR lens-flare/SSLR coordinate defect at its actual
producer/consumer before replacing shaders. Lens flare's conditional earlier
read gap remains as stated above. Live active rectangles, auxiliary descriptors,
first-person/independent motion coverage and post-SR image quality remain open.

Receipts: `reduced-native-proof-20261116.json`, per-function
`reduced-native-*-20261116.txt`, `reduced-prepass-20261116.txt`,
`reduced-temporal-20261116.txt`, `reduced-half-converter-20261116.txt` and
`reduced-native-scalars-20261116.json`. The first new .pdata-only getter lookup
failed because leaf getters have no unwind entries; exact E8 target starts and
bounded leaf/padding listings replaced it. Temporal's .pdata entry also splits
before its main body, so the retained full entry-to-return listing is used.
Earlier arbitrary triangle windows began mid-instruction; the new complete
2186110 entry-to-return decode supplies the evidence here. No dead REA/Ghidra
session was retried, and no runtime/device acceptance is implied.


## Guarded reduced handoff implementation, source only

On baseline95ffa1e plus this working batch, the adapter now implements the
reduced path. This is not a worker build or game result. The root separately
compiled the reader/camera numeric fixture and WARP auxiliary fixture, including
the shared R11 numeric path; the complete new adapter still needs its coherent
gate and a fresh game capture. No deployment or game interaction was performed
by this worker.

The added UpdateDynamicResolution1839ED0 detour uses the native five-pointer
ABI: RTM plus four NiPoint3 pointers, including the original stack fifth
argument. Exact selected call21F9F2F returns21F9F34. Every original call runs
unchanged, with original LastError preserved. Only the existing ordinary FRAME,
current CPU source, native owner/view/context/chain and early native screen
refusal checks admit a candidate. The early query intentionally lacks a current
world-camera/resource lease and may contain exactly ScreenWorldLeaseInvalid;
it is not labeled GAMEPLAY or validcamera. Native TAA must be1/disable0 and
FE8 must be0, checked again after core planning. The adapter never writes FE8.

The core RECONFIGURE/render_config services supply dimensions and backend
policy. A valid enabled plan publishes FB8/FBC ratios and FE5 after original
native DR; FE4 must already be the native ordinary-world true selector.
Native21F30B0 then refreshes near/world jittered and unjittered caches, before
prepass. Exact calls21F32A5/BB/D1 target182BF20, which unconditionally calls
builder182D750 at182C02D. The first static check caught an incorrect direct
builder-target assumption from an old JSON label; installed instructions and
the full native update receipt supplied this corrected guard chain. No cache
invalidation, native history reset, vendor jitter or copied proxy code was added.

Publication consumes read_native_inputs with the current producer snapshot.
Integer crop fields are LTRB; actual_raster is floatXYWH. Reader-provided
canonical camera/jitter/history/motion facts and the same copied depth-domain
descriptor precede planning, explicit FG_INPUTS and any output mutation. Full
positive native screen/camera/source admission is still required there. Missing
facts retain native rendering rather than relabel a full allocation as reduced.

Reduced SR calls the existing auxiliary transaction exactly once for actual
evaluation. Mandatory raw D24/stencil and motion32 use nearest reconstruction;
optional PipBoy30, VATS15 and refraction14 are independently filtered by device,
allocation/crop, numeric format, feature support and alias identity. VATS alone
requests RESTORE_CONTENTS. Its contents restore immediately after the unchanged
Effects0..13 return21FA288; flags0 auxiliaries retire snapshots without undoing
subsequent native writes. The transaction's depth is raw partitioned game depth,
not the separately normalized vendor crop. No bilinear discrete filtering,
persistent native target replacement, private GPU allocation or new ABI exists.

Full-output borrowing saves ratios/activation, FE4, jitter and shadow viewport.
It selects ratios1/FE5false/FE4false so the guarded native RenderEffect branch
uses existing unit manager28. Manager28/38/40 pointers and refcounts are never
mutated; the genuine manager48 partial branch remains native. Thus baked
manager38 UVs cannot accidentally select reduced sampling during this scope.
The existing native1817EF0 explicit-size ABI rebuilds full-output shadow viewport
from verified Context and W/H, rather than the previous auxiliary target size;
its native camera-setter caller182C597 and explicit-size branch are guarded.
Only successful SR suppresses native TAA for this RenderUI. Failure restores
owned output state before unchanged native imagespace/TAA fallback. Known SR-Off
plans do not borrow output state. Genuine native FE4 writes, even same-value
false, update the eventual restore intent; true writes are temporarily borrowed
as false during the output scope. Normal HUDless false leaves native full-output
viewport intent intact instead of resurrecting old FE4true/reduced metadata.

Input ratio ownership ends after original Swap. An observed complete native DR
writer supersedes our input values, including equal writes, while the source
obligation remains until Swap. During output, that writer's new restore intent
is preserved while the temporary full-output values are reasserted. Unexpected
producer overlap refuses this temporal source. Unknown scalar values differing
from both saved and owned values belong to the new writer: preserve them, retire
our field flag and mark the source refused. A changed borrowed triangle pointer
likewise refuses the source without a false busy hold after actual writes retire.
Failed reads/writes retain ownership. Native shadow restoration additionally
requires the exact active TLS/fallback Context; renderer/device-context/chain/
window/frame/view changes prevent unsafe writes. A recreated owner during an
unretired scope is unsupported and remains busy rather than writing into a new
or freed object. Global input fields can retire at Swap without assuming the
old native shadow remains bound, because they do not dereference that shadow.

The auxiliary obligation is marked before calling the service and cleared only
on explicit restoreOK. The shared owner added the approved idempotent empty
acknowledgement with host/session/thread checks; mismatched pending work still
refuses. Neither a nonzero SR result nor a native pointer change proves GPU
cleanup. Pending auxiliary or actual native-state retirement blocks replacing
the existing World and its resource pins; no busy counter is reset to restart.
Hooks remain enabled while quiescing scopes drain.

The existing standard Native/Quality/Balanced/Performance/Ultra Performance
mask0x1F is exposed in source. Backend choices remain core-owned. The coordinator
also authorized named FG_INPUTS|INPUT_TIMELINE declarations3 for cold tests of
the implemented facts, with no RENDERED_SOURCE and readiness0. These flags are
not accepted-input, latency, tags or generated-output claims. No fake SIM or
INPUT_SAMPLE event was added, and fallback unbound sources retain core refusal.

Static PE checks passed8 entry guards,17 direct calls,7 Present continuations
and11 resolution guards. Eleven scalar decision-model cases cover owned,
already-restored and superseded values, read/write failure, borrowed geometry
change, unsafe owner, same-value native viewport intent and independent explicit
auxiliary retirement. This model/source audit is not compiled production test
coverage. Receipts: reduced-implementation-guards-20261116.json and
reduced-ownership-scalars-20261116.json. Resize/backend/quality changes require
fresh core plans and producer facts; an in-flight owner mismatch refuses and
retains actual obligations. Live reduced raster, pre-SR lens-flare/SSLR binding
coverage, first-person/raw-motion coverage, partial/mod paths and moving output
quality remain required capture evidence. The existing conditional lens-flare
gap is not a demonstrated defect and no shader replacement is inferred from it.

Later coordinator correction: old PID37012 eventually exited orderly0 and20
restorations completed. Checkpoint95 ABI23 PID37768 then ran borderless
2560x1600 at144DPI, foreground1, with held keys and world observation; only the
first2 SR publications were reported before a target-math refusal at
source3486/native3957, with no F10. Native Quit exitedC0000005 without a new
trace/cause claim, and20 entries were restored. None of that exercised this
unbuilt reduced adapter. Root owns the combined gate and next bounded run.
