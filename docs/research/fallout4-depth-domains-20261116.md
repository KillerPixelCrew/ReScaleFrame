# Fallout 4 depth domains, 16 November 2026

Question: which installed native state can describe the partitioned scene depth
to a generic normalizer without putting game constants in the runtime?
This is static PE/Capstone and pinned reference-source review. No game, GPU,
build, shared analysis session, production hook or ABI change was performed.

The executable is Fallout4.exe 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
Addresses below are RVAs in this installed file, independently decoded rather
than copied from 1.11.221. The source lead is community-shaders revision
`e0e711594291c1a8e447ac758099492514810fbb` and its pinned CommonLibF4.

## Producer and context ownership

The direct native prepass caller is `21f9f73: e8 38 99 ff ff` to `21f38b0`,
return `21f9f78`, within RenderPreUI on the ordinary world path. The prepass entry is
`48 89 5c 24 08 48 89 6c 24 10`. It obtains the native TLS index from
global `3e73018`, reads `gs:[58]`, then `[TLS-array+index*8]`, then the
Context pointer at TLS-block+`b20`. A null Context selects global `38e1398`.
Shadow state begins at Context+`1b70`. These exact instructions occur at
`21f38d1`, `21f38e7..21f3904` and again `21f3920..21f3940`.

The reference's GetActiveContext currently returns GetGlobalContext directly.
That is a source lead, not permission to ignore a nonnull native TLS Context.
An adapter must select the same Context as the native producer and associate
it with the current source/view and immediate-device/resource scope.

Camera setup `182bc80` has native ABI
`void(State*, NiCamera*, bool jittered, float minDepth, float maxDepth)`:
RCX, RDX, R8B, XMM3 and stack fifth argument, respectively. It finds a
State camera-cache entry keyed by camera at entry+`238` and jitter flag+`240`,
or uses State+`160` and calls builder `182d750`. It copies the projection
entry+`90..c0` to shadow+`760..790` at `182bd69..182bd9a`.

Prepass calls camera setup with camera global `3e622d0` and jitter flag1 at
`21f3983` and `21f3b10`. Each then loads shadow+`760..790` into XMM0..3 and
calls matrix inverse routine `336140` at `21f39ca` / `21f3b57`. That routine
forms cofactors, reduces the determinant and divides before returning four
inverse rows in XMM0..3. The following shuffles transpose those rows:

| Native stores | Result |
| --- | --- |
| `21f3a07: 0f 11 9b a0 08 00 00`, then `21f3a0e/15/1c` | Transposed inverse at shadow+`8a0/8b0/8c0/8d0` |
| `21f3b94: 0f 11 9b a0 08 00 00`, then `21f3b9b/a2/a9` | Replaces it with the second setup's inverse of the same near-camera projection |

This matches CommonLib's ViewData inverse-first-person field at+`1d0`, inside
shadow camera data+`6d0`. The address is Context+`2410`, not Context+`8a0`.
Native camera-cache matrices use row-vector convention; the shadow field is
the transpose of the inverse of that native projection. For row-vector math,
transpose it back. For column-vector shader math it already represents the
inverse projection applied to a clip column. Do not transpose both this field
and the native world projection identically without accounting for that.

At `21f3c08` and `21f3db6`, setup restores world camera global `3e62210`,
also with jitter flag1. The inspected setup copies through shadow+`890` but
does not copy over+`8a0`. The last near inverse therefore survives these
world-camera restores. The two qualified prepass stores are the contextual
writers found by the targeted SIMD-store scan; other matches at displacement
`8a0` are mostly unrelated stack/object writes and are not extra camera proof.
This is mutable Context state, with no native source-frame stamp beside it.
Copy it after the second store or prepass return `21f9f78`, within the
verified same-source render callback lifetime. WorldBegin entry precedes
this producer and cannot establish current-frame freshness by itself;
never retain a native pointer or infer freshness from finite values alone.
Bulk Context resets/copies or another view remain invalidation risks until
the adapter's existing scope association excludes them.

## Exact viewport intervals

Viewport setter `182c4e0` accepts the native rectangle in RDX and depths in
XMM2/XMM3, selects the same TLS/fallback Context, and stores them at
shadow+`6e0/6e4` (`182c51b` / `182c523`). Camera setup forwards its fourth
and fifth arguments into those positions at `182bec2..182bef3`.

In the prepass, the near camera uses min from global `3e62190`, max from
`2f97b30`. The world camera uses min `3e62194`, max `2f97b34`. Near and world
are not inferred from the last viewport at the eventual resource reader.
Foreground `21f4911` temporarily installs the world interval and
`21f4d85` restores saved XMM6/XMM7 values, as the reader worker's receipts
already show.

WorldBegin establishes the partition mode explicitly. It sets XMM9=1 from
`2929458` at `21f0ba0`, and XMM6=0 at `21f0cfb`. The mode byte loaded at
`21f0ddd` is global `3e62250`. Constant `2929400` is bytes `0a d7 23 3c`,
the exact float32 value `0.009999999776482582`. At `21f0e25..21f0e3a`:

- Mode byte zero selects this float for both XMM9 and XMM6, producing the
  partitioned near maximum and world minimum.
- Mode byte nonzero retains near maximum1 and world minimum0, so the two
  intervals overlap completely and a threshold-only classifier is invalid.

`21f0e2d: f3 44 0f 11 0d fa 6c da 00` writes near maximum to `2f97b30`.
`21f0e44: f3 0f 11 35 48 13 c7 01` writes world minimum to `3e62194`.
The first is writable initialized data, despite its on-disk default1; it is
not an immutable constant. World maximum `2f97b34` is initialized float1.
Near minimum `3e62190` is zero-initialized storage. No targeted scalar store
to either was found; that does not exclude indirect writes or mod changes.
The adapter should copy all four actual values and mode within the source
scope and recheck them, rather than hardcode initialization values.

For the ordinary partition mode with interval [s,1], exact world projection
depth is `(stored-s)/(1-s)`, with s being the actual float above. Coefficients
are approximately `1.01010100987` and `-0.01010100987`, not the reference
shader's approximate `1.01,-0.01`. Near projection depth is
`(stored-nearMin)/(nearMax-nearMin)`. A float multiply100 is also an
approximation to division by the represented s. Derive coefficients from
the sampled interval and use a documented precision policy.

The shared endpoint s belongs to both mathematical intervals. A bare <=s
test follows the reference but cannot distinguish a world fragment exactly
at its viewport minimum from a near fragment at its maximum. Depth-format
quantization and the exact threshold ownership remain validation gaps.

## Smallest adapter fact contract and remaining proof

The existing generic region copier has no depth-domain mapping contract.
The required facts are a same-source, same-view copied description containing
each stored depth interval, its inclusion/clear rules, the corresponding
projection transform into the target world depth space, matrix convention,
active rectangle, source depth format/sample count, and existing resource
lease. The adapter supplies these facts; generic code can normalize into an
R32 product using interval unmapping and homogeneous reprojection. No game
name, native RVA, special 0.01 threshold or camera lookup belongs in it.

For near depth the mathematical mapping is near clip -> near view -> target
world clip -> divide by w. Projection-only conversion is justified only when
the views share the required coordinate basis. The reference uses near inverse
and world projection alone; this review has not proved that near/world view
matrices are equivalent in every native camera mode. Otherwise the adapter
must supply the full near-to-target clip transform, including view change.
Likewise world CameraABI1 currently derives an unjittered projection while
this prepass setup requests jitter1. Target jitter and target projection must
be explicit and internally consistent before provider admission.

Refuse normalization when the source association/freshness is absent, mode
has overlapping domains without ownership information, bounds change, matrix
is nonfinite/singular, projected w is invalid, view basis is unproven, or depth
format/MSAA/clear classification is unsupported. This research does not prove
all transparent/later depth writers obey this partition, nor near-camera
absence behavior, quantized boundary ownership, final live resource contents
or temporal-provider quality. No new capability is enabled.

Untracked receipts: `depth-domain-proof-20261116.json`, `cpu2-21f38b0.txt`,
`cpu2-182bc80.txt`, `cpu2-336140.txt`, `cpu2-21f0ac0.txt`,
`cpu2-21f0d90.txt`, `cpu2-21f9ef0.txt` and `cpu2-182c4e0.txt` in
`.local/fallout4-research/`. Values from uninitialized PE storage were not
treated as captured runtime values.

## Follow-up: current-source snapshot and full transform

The follow-up question was whether the existing cache can supply a full clip
transform, including different camera views and floating origins, without
assuming the reference's projection-only mapping. It can describe the
matrices used by these native passes; numeric depth-domain ownership remains
a separate unresolved condition.

Before the inverse-producing call, RenderPreUI executes
`21f9f69: e8 42 91 ff ff` -> `21f30b0`. Inside that routine, the consecutive
native updates are:

| Call | Camera and cache key |
| --- | --- |
| `21f32a5: e8 76 8c 63 ff` -> `182bf20` | Camera read from `3e622d0`, jitter flag1 |
| `21f32bb: e8 60 8c 63 ff` -> `182bf20` | Camera read from `3e62210`, jitter flag1 |
| `21f32d1: e8 4a 8c 63 ff` -> `182bf20` | Camera read from `3e62210`, jitter flag0 |
| `21f32dd: e8 6e 8d 63 ff` -> `182c050` | Rolls existing camera history |

All three use State `3d871a0`. Update `182bf20` locates or allocates a
`250`-byte cache entry by camera pointer+`238` and useJitter byte+`240` and
unconditionally calls builder `182d750` at `182c02d` for the selected entry.
This differs from setup `182bc80`, which may reuse a matched entry without
rebuilding. Builder ends by storing the actual camera key at `182db2d`,
useJitter at `182db38`, and the camera's position at entry+`210..218` through
`182db0f/1b/27`. Thus a retained finite cache entry alone is insufficient;
the observed current-source update-then-use chain is the freshness evidence.

At guarded return `21f9f78`, the minimal proposed adapter snapshot copies
the three uniquely keyed cache entries, State frame identity and jitter
state, active native Context identity, near inverse, both camera pointers,
partition mode and all four interval values. Read the bounded State cache
pointer+`140` and count+`150`, find exact camera/useJitter matches, copy the
values, then reread pointer/count/keys/identity and reject changes or
duplicates. Copy at this return while the existing ordinary source lease is
unchanged. Reject nested or unmatched callers, a missed update/prepass,
another source/view, intervening cache mutation or context change. A frame
number accompanying the copy does not manufacture proof that the callback
ran. No native cache pointer should escape the callback.

This snapshot timing has static native-order support from
`21f9f69` -> updates -> `21f9f73` -> inverse stores -> `21f9f78`.
Actual adapter callback execution and stability checks have not been tested.
The guard at the prepass call remains `e8 38 99 ff ff`; do not patch the
fall-through instructions merely because `21f9f78` is the chosen timestamp.

For each entry, copy view V at+`50`, actual projection Pj at+`90`, actual
view-projection VPj at+`d0`, unjittered view-projection VP0 at+`110`, and
origin a at+`210`. Builder first forms and stores V*P0 in both VP fields at
`182d9d9..182da0f`, then, only when State+`ac` is nonzero and the entry's
useJitter byte is true, modifies P and rebuilds VPj at
`182da16..182dae2`. The target CameraABI1 uses P0, derived by inverse(V)*VP0,
with jitter reported separately. Key1 means the camera requested jitter;
the copied actual matrices establish whether any jitter was applied.

Native V is rotation-only: builder writes its rows from the NiCamera's
rotation and explicitly zeros the translation entries, ending in1 at+`8c`.
The camera's world position is copied separately to a. This matches the
reader's existing full world view `T(-a)*V`. Therefore the row-vector map
from actual near clip to target world clip is, expressed consistently as
a row-vector multiplication:

```text
Mnear = inverse(VPn_jittered)
      * T(aNear - aWorld)
      * VPworld_unjittered
Mworld = inverse(VPworld_jittered) * VPworld_unjittered
clipTarget = clipSource * Mdomain
depthTarget = clipTarget.z / clipTarget.w
```

Equivalently, the first expression is inverse(Pnear_jittered) *
inverse(Vnear) * T(aNear-aWorld) * Vworld * Pworld_unjittered. It includes
both view-basis differences and origin differences. It does not require
equal camera views or world positions. Use finite/invertible matrices and
verify copied V*Pj agrees with VPj and the transposed shadow near inverse
agrees with inverse(Pnear_jittered), within an explicitly chosen numerical
tolerance. These are proposed scalar consistency guards, not live results.

Source clip XY must come from the source render rectangle's actual pixel
centers, including any origin, and its actual jittered projection. First
undo the stored viewport depth interval for Z. The target remains the
CameraABI1 unjittered world clip space; do not add source jitter again or
replace the source inverse with an unjittered inverse. Pure XY jitter may
leave ordinary world depth numerically unchanged, but that observation
does not prove near-view equivalence. This is mathematical derivation from
the native matrix/origin layout, not a GPU normalization acceptance result.

The full transform can change clip XY as well as Z. A depth-only shader
that writes transformed Z back to the same texel has not thereby moved the
point to its target projected XY. For example, different camera rotations
or a translated near origin can introduce rotation/parallax that a depth
scalar cannot encode. Provider reconstruction from target pixel XY and that
Z would describe a different point. A same-texel normalization therefore
also needs proven alignment under the target raster/jitter convention,
checked from the full transform or guaranteed by the admitted camera mode.
Ordinary jitter handled by the existing camera contract is distinct from an
unmodelled camera-basis displacement. A general reprojected resource would
need additional coverage/occlusion semantics and is outside this research.
The full transform removes the need to assume equal views for describing
the point; it does not automatically prove a same-texel R32 output is valid.

## Follow-up: absence, clear, endpoints and later writers

The inverse writes are unconditional along the inspected prepass sequence,
even when a render-list operation submits no geometry. A valid near matrix
or nonnull near-camera pointer therefore does not prove near pixels exist.
Conversely, an empty list in one batch cannot prove the entire frame lacks
near depth. No complete native near-absence predicate was established.
The adapter can avoid needing such a predicate only after it has another
proven means to classify all pixel domains; it cannot silently select
world-only conversion from missing near state.

A native depth/stencil clear path was identified at `1823ed4`:
`ff 90 a8 01 00 00`, Context COM slot53, ClearDepthStencilView. It loads
the DSV from the selected RendererData depth-target entry at
`1823e8d..1823e95`, uses the TLS device context at TLS+`b18`, sets depth
XMM3 from constant `2929458` (float1) at `1823ec4`, and stencil byte0 in
the stack argument at `1823ecc`. Native requested set modes0/5 select
clear flags3; mode1 flags1; mode2 flags2. Other modes skip this clear call.
The clear is conditional on a nonnull selected view and is followed by
normal native target binding. This proves the value/arguments when that
native clear executes, not that Main was cleared on every accepted frame.
The selected target/DSV and clear execution must still match the source
depth lease. Do not call arbitrary depth1 samples untouched background:
native far-plane geometry can share that value, although preserving target1
can represent either without distinguishing them.

For the actual D24 scene depth observed by the coordinator, exact partition
s times `(2^24-1)` is approximately167772.14625. Adjacent stored codes are
167772 and167773, decoding to approximately0.00999999105930275 and
0.01000005066395108. The viewport boundary itself is not on the D24 lattice.
This scalar calculation is independent of, and does not prove, the GPU's
precise native rounding behavior. It demonstrates why applying float <=s
cannot be declared an exact ownership rule: both a near endpoint and world
minimum can map to the same stored code. R32_FLOAT can represent s exactly,
but the mathematical shared endpoint still has two owners. A comparison
change from <= to < only assigns the ambiguity; it does not resolve it.

D24S8 provides stencil storage, but no inspected evidence establishes a
dedicated near/world stencil bit or a stable stencil reference spanning
all relevant passes. The native clear above clears stencil0 when requested;
that alone is no domain mask. CommonLib's depth modes describe tests/writes,
not camera ownership. Do not repurpose any stencil bit without tracing its
actual depth-stencil-state masks, references and writers. Pass ordering also
does not distinguish a world-only boundary pixel from a near pixel when the
stored code and final stencil are equal or unverified.

Prepass return is a matrix snapshot point, not the final depth write. The
same RenderPreUI body continues with `21f9ff6` -> `21f4de0`, `21f9ffb` ->
`21f4130`, `21fa027` -> `21f3e80`, `21fa02c` -> `21f4fd0`,
`21fa031` -> `21f82a0`, and `21fa036` -> `21f9100` before the later
imagespace consumer. In particular, the latter routine sets the near camera
at `21f95ff` using the same near interval, calls `21d7120` at
`21f963d: e8 de da fd ff`, then restores the world camera at `21f9699`.
This is concrete evidence of a later near-camera pass. It does not by itself
prove that every draw writes depth, that its final depth target is Main, or
that other intervening effects cannot write another encoding. Reference
first-person alpha handling is therefore a lead to later writers, not
permission to treat the inverse producer as the final depth writer.

## Narrow admission conclusion

The smallest implementable matrix description is two copied homogeneous
source-clip-to-target-clip transforms plus sampled intervals, source/view
identity and existing resource lifetime. The native sequence supplies
proposed guarded snapshot sites and keyed matrix data for that description;
the transforms explicitly represent unequal views instead of assuming them
equal. Same-texel target alignment still requires its own admission proof.

It does not yet supply a universally correct numeric pixel-domain selector,
current Main clear receipt, near-absence proof or complete later-writer
coverage. For whole-texture normalization, admission must remain refused
until either actual ownership data resolves shared/quantized boundary pixels
and later writers, or a narrower proven input contract excludes those cases.
No hardcoded ambiguous-code assignment, reference threshold, unchanged
viewport, valid matrix or generic ABI can supply that missing engine fact.
No production mechanism, broad GPU framework or capability was added here.

Additional ignored receipts are `depth-prerequisites-20261116.json`,
`depth-cache-update-callers-20261116.json`, `cpu2-182bf20.txt`,
`cpu2-182d750.txt`, `cpu2-21f30b0.txt`, `cpu2-21f3280.txt`,
`cpu2-1823e10.txt`, `cpu2-21f9440.txt` and `cpu2-21f9c90.txt`.

## Implemented adapter facts for ABI23 scalar depth

The subsequent authorized implementation uses ABI23's deliberately narrower
contract: scalar depth conversion at existing composed raster samples. It
does not assert transformed XY or spatially reproject geometry. The earlier
exact-ownership and geometric-alignment gaps remain limitations; they are
not prerequisites that this scalar contract claims to have proved away.

New game-owned `depth_domains.h/cpp` provide a stack-owned
NativeDepthSnapshot, `verify_native_depth_domains()`,
`capture_native_depth_domains(cpu,snapshot,report)` and
`make_native_depth_domains(snapshot,cpu,pass,domains,report)`. The optional
report callbacks default to null. The adapter owner wires capture after the
original selected prepass returns at `21f9f78`, under its existing ordinary
CPU source lease. Capture is not callable evidence that the native prepass
ran: caller/entry guards and the adapter's entered-source scope establish
that association. Verification must run before enabling the prepass hook.

The copy includes native frame, selected Context and immediate-context
identities, renderer/device/window/chain, depth texture/SRV owner keys,
State cache identity, uniquely matched near1/world1/world0 matrices and
origins, camera ports, jitter state, interval mode and actual four bounds.
Integer keys are opaque comparison values, not retained native pointers or
new resource owners. Native reads reconstruct fresh addresses from current
globals and cache keys rather than dereferencing snapshot keys. There is
no new source ledger, timestamp substitute, vendor policy or GPU operation.

Capture samples twice, checks current CpuIdentity/source before and after,
and compares copied state exactly across reads. It checks finite/invertible
camera matrices, V*Pj agreement with VPj, near transposed inverse agreement,
world jittered/unjittered cache consistency, full normalized camera ports,
full native raster/ratio1 and split mode0 with bounds `[0,s]` and `[s,1]`.
Mode1 overlapping intervals, missing/duplicate keyed entries, missing near
camera, changed state, mismatched inverse or nonforward target W refuse.
The target W lower bound covers the source clip box with one D24 endpoint
step of tolerance. The module uses no projection-only equal-view shortcut.

The later make call independently resamples native state and matches the
snapshot, current CPU source, primary SR role, submission/scope, native
frame, view/family/window/viewport keys, full render/output rectangles,
CameraABI1 target projection/view/inverses and jitter, and leased depth and
swap-chain identities. It validates the currently owned single-sample,
one-mip/slice R24G8_TYPELESS texture with R24_UNORM_X8_TYPELESS SRV, actual
view-resource identity and canonical device identity. It resamples again
before publishing. Every failed capture leaves a zero invalid snapshot;
every failed make leaves the domain descriptor all zero. Callers must
refuse the mapped path on failure rather than treating that zero as a
request to use raw depth.

On success, the two descriptors implement explicit reference-aligned
ownership policy: near `[0,s]`, world `(s,1]`, clear1 tested before either
domain. They unmap the actual sampled near/world viewport bounds and use
the full near/world-to-target matrices derived above. Clear1 is an explicit
scalar interpretation policy, not a claim that every source texel with1
was untouched by geometry or that the clear executed on every frame.
The D24 shared-code ambiguity remains: the interval policy does not recover
primitive ownership. This is the requested composed-depth approximation.
Later depth-writer coverage, native motion consistency, weapon quality,
spatial reconstruction, temporal-provider quality and FG acceptance remain
unverified. Raw game depth, colour and motion are not modified by this module.

Refusal receipts use a fixed internal bitmask with one first receipt per
named stage, at most21 receipts per process when a report callback is supplied.
They carry source/submission/native frame, mode and four bounds. Distinct
TLS, owner, cache, range, camera-read/math, near-inverse, target-camera,
format/view and state-change failures preserve a separate next-run receipt;
loading cannot consume the budget for a later new stage. This adds no
diagnostic option or per-frame log stream.

Offline verification matched all22 module byte guards against the exact
installed executable. The independently derived analytic scalar probes
cover unequal near/world yaw, nonzero origin delta, source jitter and
different perspective matrices. Three target-depth comparisons have maximum
double-precision error `1.1102230246251565e-16`; an independent behind-target
probe has negative W. A changed clip-X result also retains the documented
distinction between scalar conversion and spatial reprojection. These probes
exercise the mathematics, not the compiled adapter or a GPU. Receipts are
`depth-domain-implementation-guards-20261116.json` and
`depth-domain-scalars-20261116.json` under the ignored research directory.
New source whitespace was checked. No worker build, game run, deployment,
shared-header/runtime edit or capability activation was performed.

## Reduced producer raster and canonical crop implementation

The next source change extends the same stack snapshot with the actual
immediate-context RSGetViewports record at the selected post-prepass return,
separately from Context shadow90..A4, dirty bits, normalized port6d0,
logical color/depth/cube selectors, selected logical descriptor dimensions,
FE4/FE5, actual ratios, bound DSV identity and scissor coverage. It reads the
native 32-bit cube binding at shadow54 as an index, not a pointer. Foreign
cube/color/depth selection, a bound DSV not viewing the current Main depth
allocation, non-immediate/wrong-device context or non-R24/full-output depth
allocation refuse. Selected color1 or absent color with depth1 is required;
the descriptor getter's native color stride20 or depth stride1c is retained.

Reduced publication requires actual submitted viewport XYWH to agree exactly
with the pending shadow XYWH and all three keyed camera ports to match that
normalized port. Depth endpoints may differ because the final camera restore
can be pending. The selected descriptor must describe the full native output;
ratios must be finite in(0,1], default viewport selection must be active and
reduced ratios require the native dynamic flag. No actual dimension is inferred
by multiplying/rounding those ratios. A scissor, when enabled, must cover the
entire copied integer sample rectangle. Two producer reads compare the raster,
shadow, selectors and DSV as well as the existing source/camera/cache facts.
The reader later rechecks stable source facts but does not reinterpret later
pre-SR RS state as the producer or require it to equal the old producer state.

The integer rectangle uses repository LTRB pixel edges. For actual float
viewport x/y/w/h, its left/top are ceil(x/y minus0.5); right/bottom are
ceil(x+w/y+h minus0.5). This is the half-open viewport support evaluated at
source texel centers, not a claim that every such pixel was covered by a
primitive. The exact fractional XYWH stays in depth-domain descriptors.
If actual/pending extents differ after native truncation, the module reports
producer-shadow and refuses. A more precise native world draw/bind snapshot
would then be required; finite pending values or nominal quantized sizes
are not substituted. Main/depth allocation dimensions remain full output.

The game-owned camera helper constructs the requested row-vector affine C
from that actual viewport into the copied crop grid and postmultiplies native
unjittered projection and current VP by C. Previous VP uses the previous
copied raster's C in the existing reader history, with reset on missing or
changed raster, size, frame/view or native history. The full depth-domain
near/world transforms also postmultiply by current C, so their target camera
agrees with the adjusted CameraABI1. No second history ledger was added.
Camera output dimensions remain full output; render dimensions are crop
right-left and bottom-top. Jitter is native cached NDC displacement times
actual viewport extent/2, with the existing Y sign, rather than a rounded
ratio product. The underlying projection is adjusted, not only its units.

The actual native PS2607 arithmetic recorded in the shader-input audit gives
previous-minus-current UV, Y down. Under steady raster, native UV displacement
maps to crop UV by viewportWidth/cropWidth and viewportHeight/cropHeight.
The reader now calls the pure game-owned camera_motion_to_uv helper to report
the corresponding negative scales. Viewport-origin offsets cancel only for
steady history; changed metrics reset history rather than claim the old
vector still has that mapping. This extends the checked native encoder's
units, not coverage or freshness of every native motion-writing variant.

`read_native_inputs(inputs,report,producer)` takes the existing copied
NativeDepthSnapshot as its optional third parameter. A supplied producer must
still be current and agree with native frame/view/cache/context and the Main
depth resource. Without one, the old ratio1/full-raster path remains required.
Adapter wiring must pass its World snapshot and derive all rectangle widths
from LTRB differences. No ratio writes, auxiliary transaction, consumer
restoration or capability/quality activation is implemented by this reader.

The module now has28 exact executable guards, all matched offline, and four
additional named producer refusal stages. Three independent ignored scalar
cases enumerate pixel centers and compare affine NDC, jitter and raw-native
motion displacement against direct geometric pixel coordinates. The new
compiled contract fixture `tests/fallout4_camera.cpp` links actual camera.cpp
and covers asymmetric projection with non-origin fractional raster, inverse
roundtrip, jitter, near/far invariants, translated previous-camera geometry,
changed/missing raster reset, motion units and invalid inputs. It is registered
as fallout4_camera_contract; it has been added but not built or executed by
this worker. Scalar receipts are `reduced-raster-adapter-scalars-20261116.json`.
The coordinator owns compilation, actual producer raster/DSV observation,
normalization, native consumer restoration and moving SR/FG quality checks.

## Checkpoint95 target-math investigation

The coordinator's borderless2560x1600 checkpoint95ffa1e ABI23 trial,
PID37768 in `native-raster-20261116-123332-773`, logged SR publication for
source3484/native3955 and source3485/native3956. The next source3486/native3957
reported the old aggregate target-math refusal with split bounds
`0,0.01/0.01,1`. F10 did not complete within its12-second bound. Prior public
effectiveSR1/result0 values describe earlier success, not sustained acceptance.
The actual Quit UI later exited with C0000005 without a close trace, so its
cause is unassigned; the earlier baseline close result does not identify this
untraced failure. The separate guarded recovery restored all20 trial entries.
No worker game interaction or recovery invocation was performed here.

That old stage combined recovered-projection mismatch, nonfinite map stores
and nonforward near/world W. The log includes the world origin approximately
`[-80300.7,90804.2,7973.14]`, near15 and far353468, but no near origin,
W coefficients or matrix-mismatch component. It cannot distinguish numerical
cancellation from camera-coordinate-model failure.

The native builder proof matters: cached V is rotation-only and the origin
is copied separately at+210. The current inverse(V)*VP projection recovery
does not insert that large world origin into the cached VP. Consequently
origin magnitude alone is not evidence of an ill-conditioned translated VP
in this implementation. The equivalent factorization through separate
inverse projections and inverse views remains a possible numerical change,
but no source-only observation here establishes it would fix this live fault.
The current projection/cache validation, full cross-camera transform,
canonical crop and tolerances are preserved rather than bypassed.

The implementation now distinguishes projection-mismatch, finite-map,
near-W and world-W, with raster-math reserved for invalid crop-affine math.
Each retains its own first-receipt bit. Projection mismatch logs the largest
normalized component mismatch, index, actual/expected values and the unchanged
0.0005 relative tolerance with unit floor. Near/world W refusal logs the four
affine W coefficients, one-D24-step clip-box minimum, interval width and both
copied origins. These extra details are emitted only with that stage's first
receipt, not per frame or as full matrix dumps. The existing W threshold
remains strictly greater than1e-6.

W-bound arithmetic is now a pure game-owned camera helper used by the actual
depth module and the compiled camera fixture. Its formula and threshold are
unchanged. The fixture independently enumerates all clip-box corners, retains
negative-W and small-positive-W refusals, and covers nonfinite/missing maps.
Additional actual-camera.cpp regressions use the observed large origin,
rotation-only V, near/far ratio and all eight jitter phases, with independent
analytic relative-camera history expectations. Inconsistent projection/cache
inputs still refuse; no tolerance is widened. These new compiled cases have
been authored but not built or executed by this worker, so they do not yet
prove or disprove an actual implementation numerical defect.

A separate semantic hypothesis remains: a near camera may represent a
virtual first-person layer rather than the same absolute world as the world
camera. In that case the shared-world translation T(aNear-aWorld) can be
algebraically valid for its assumed model yet inappropriate for the composed
layer. For example, an unobserved near origin0 paired with the recorded world
origin would imply a large origin delta; a negative near-W bound could then
be a coordinate-model refusal rather than precision loss. This example is
not a claim that the native near origin was0. The new copied-origin/W receipt
is needed first, followed by native near-root/camera producer tracing.

The pinned reference intentionally uses near inverse projection followed by
world projection for composed depth, but that alone does not prove the native
virtual basis or justify silently selecting that model here. A future explicit
virtual-view-Z convention could be appropriate under ABI23 scalar-depth
semantics without asserting equal physical views. Such a change must retain
actual near inverse freshness, sampled intervals/jitter and the target world
camera, and state the changed depth convention. No such switch was made in
this investigation. No new readback, GPU framework, capability or live-quality
claim was introduced. The coordinator owns the next compiled gate and the
next discriminating native observation.

## Post-prepass logical target correction, 2026-11-16 13:21 report

Question: why did the producer snapshot refuse before matrix validation in
`native-raster-20261116-132159-236`? The copied first receipt was source16737,
submission1, native17204, actual and pending raster0,0,2560,1600, dirty62,
selectors26,1,UINT_MAX, ratio1,1, default1, dynamic0. The old implementation
accepted only logical colour1 or none, so it returned before reading a
descriptor. Its logged descriptor0x0 was zero-initialized, unreachable metadata,
not an observed zero-sized native allocation. This corrects that interpretation;
neither target allocation failure nor a matrix failure follows from this receipt.

Read-only Capstone inspection of the same installed1.11.240.0 executable and
fingerprint recorded above establishes the distinction between logical viewport
ownership and submitted resource ownership. Pinned CommonLibF4 `BSGraphics.h`
names logical26 `kGBufferAlbedoSpecular`, while logical1 is Main. That name is
source evidence; the installed instructions supply the field semantics:

| Native site | Exact association |
| --- | --- |
| `1839BC9..1839BCD` | `SetCurrentRenderTarget` stores R8D, the caller's logical ID, into shadow+84 only when EDX, the MRT slot, is0. |
| `1839BDF..1839BE2` | The logical ID indexes RTM+DF4 at stride4 to obtain a physical target ID; none stays -1. |
| `1839C2B..1839C4B` | The physical ID is stored into shadow+2C+slot*4; cube binding becomes none and target-dirty mask0x1 is set on a binding/mode change. |
| `1839868..1839877` | Any nonnegative shadow+84 selects logical colour properties at RTM+ID*0x20. A negative selector falls back to the positive logical depth selector at shadow+88, stride0x1C plus0xC80. `18398DF` supplies the corresponding colour height+4. |
| `1839CB1..1839CBB` | The logical depth ID is stored into shadow+88 and translated through RTM+F84; the physical result is the shadow+4C binding. |
| `1823D2D`, `1823DB7..1823DD1` | Target submission tests dirty mask0x1, reads physical shadow+2C slots and loads RendererData+physical*0x30+0xA68 RTVs. These slots are not logical IDs. |
| `1823E55..1823E8D`, `1823F14..1823F17` | Physical shadow+4C selects the native DSV variant/slice, then D3D context vtable+0x108 submits RTVs and DSV through OMSetRenderTargets. |

These are data-flow observations, not an inference that a function name proves
a hook or a resource. Local instruction receipts are `cpu2-1839b70.txt`,
`cpu2-1823cf0.txt`, `cpu2-1824020.txt` and the retained width/height getter
receipts. No shared analysis session or live process was accessed.

The snapshot now reads the descriptor selected by this native logical path,
without a special case for26. It still admits only logical colour IDs0..99 or none,
logical depth1, no cube target and full-output descriptor dimensions. It
requires target-dirty mask0x1 clear; the recorded62 satisfies this one condition,
but that receipt does not prove any of the new resource checks passed.
Logical count100 follows the colour-property span0..0xC80 at stride0x20 and
the map span0xDF4..0xF84 at stride4, also matching the pinned logical enum count.
It is distinct from the physical RendererData array's101 entries.
The mapped physical colour ID must equal shadow's physical slot0. The actual
immediate-context RTV0 must be the native RendererData RTV for that physical
ID, and its queried resource must be the native texture, on the same device,
with the selected descriptor's dimensions and single mip/slice/sample. A none
selector instead requires both physical slot0 and actual RTV0 absent.
Actual DSV identity with Main depth, R24/D24 format checks and full allocation
are retained, and shadow's physical depth binding must match Main's mapped
physical depth. Main colour1 publication remains the separate SR reader lease;
it is not imposed on this GBuffer prepass viewport owner.

Actual/pending raster XY equality, camera ports, current source/frame/cache,
ratio/FE flags, scissor and matrix checks remain unchanged. Capture repeats the
native reads and actual D3D queries, including the new copied physical IDs and
colour identities, and refuses changes between the two snapshots. These are
copied scope facts with no retained COM ownership or new source history.

Bounded first refusals now distinguish `producer-selector`,
`producer-descriptor`, `producer-target-dirty`, `producer-target-map` and
`producer-colour`; `producer-target` remains the actual DSV check. The receipt
includes `descriptor_read`, so an unread zero differs from a successfully read
zero allocation, plus logical and physical selector values. No acceptance guard
or tolerance was relaxed for pending viewport state or matrix validity.

Checks: all35 activation byte guards matched the exact installed PE, including
the logical-to-physical setter, descriptor getter and native OM submission;
scoped diff whitespace checks passed. This worker ran no build, compiled
fixture, game, GPU capture, deployment or commit. The corrected source is frozen
for the coordinator's next combined gate. Live RTV/descriptor association,
subsequent snapshot/math admission and sustained SR remain unvalidated by this
change; earlier success counters are not evidence of sustained acceptance.

The later documentation reconciliation uses final `loader-post-controller.log`
for this132159 trial. FG configuration switched requested1/effective1/result0,
but prepare remained selected_source0/inputs0/HUDless0/submission_ok0/result-8;
no generated FG or latency acceptance follows. The tested pre-correction
reduced-input batch passed221/15 in108.81seconds with clean fmt/Clippy; its
generic scoped fixture passed55sources/41captures/39SR/34tags/34callbacks and
rendered-source latency remained zero. These do not validate the new target fix.
The authoritative resumed-restoration receipt verifies19 restorations plus1
exact final user-config preservation,20 verified and custom INI unchanged.
Actual exit code/method are unproven without an observer or retained exit
handle; root's subsequent fresh CIM found no game. See the render note's
132159 reconciliation for exact artifact hashes and receipt names. No further
game/UI work is authorized while the user works on the desktop.

## Near coordinate-basis investigation, bounded static result

Question: does first-person depth describe a virtual layer best normalized as
`inverse(PnearJ)*Pworld0`, or a physical point in the world coordinate system
requiring the current full camera/origin transform? This follow-up worked
backward from the verified near cache and draw setup, using only retained
instruction receipts and new read-only Capstone scans of the exact installed
1.11.240.0 SHA above. No production change or tolerance/model switch was made.

The additional installed data flow is concrete:

| Site and bytes | Observation |
| --- | --- |
| `21F2CA9: 488b0520f6c601`, `21F2CB0: 0f105370`, `21F2CB4: 0f109b80000000`, `21F2CBB: 0f10a390000000` | Near camera pointer is loaded from3E622D0; rotation data are loaded from the world camera in RBX. The following scalar stores21F2D07..21F2D7B copy world rotation components into near+70..9C. This setup aligns their rotational basis; it does not copy world translation. |
| `21F2CC2: f30f1005eef5c601`, `21F2CCA: f30f1180a0000000` | Near camera translation X comes from separate global3E622B8 and is written at near+A0. The Y/Z pairs21F2CD2/DA and21F2CE5/ED read3E622BC/C0 into near+A4/A8. |
| `21FA480: f30f1001`, `21FA484: f30f11052c7ec601` | A native three-float setter copies its argument into3E622B8..C0; the next pairs are21FA48C/491 and21FA499/49E. These globals are mutable inputs, not a proven constant zero origin. |
| `C38E5A: 488d50a8`, `C38E6A: e8b1d93e00`, `C39109: 488d4c2430`, `C3910E: e86d135c01` | One direct native setter caller passes the same stack vector at current RSP+30 previously filled by1026820. The LEA atC38E5A uses the saved entry RSP; accounting for push/sub yields current RSP+30. |
| `1026820: 488b89e0000000`, `102682F: 48ff6068` | That vector producer reads owner+E0 and, when nonnull, tail-dispatches virtual slot0x68. The null branch fills three floats from3448450..58. Static caller discovery does not resolve the active dynamic implementation, its units or coordinate convention. No default value was inferred from zero-filled/BSS storage. |
| `21F95F2: 488b15d78cc601`, `21F95FF: e87c2663ff` | The later first-person draw selects the near camera with jitter requested, preserving the known near viewport interval. |
| `21F9604: 488b05058cc601`, `21F960B: 488b4828`, `21F9614: f30f1105d44ac601` | Immediately before that draw, a separate position triple is read from world camera->field28->+60/64/68 and copied to3E5E0F0/F4/F8. The pinned NiAVObject layout identifies parent and local transform at these offsets; native shader/vertex consumption still needs tracing. This is not a copy of near+A0. |
| `21F963D: e8dedafdff`, `21F9699: e8e22563ff` | The near draw calls21D7120, then setup restores the world camera. The draw routine traverses accumulator/batch work through2221890/2221940 and2220AC0. Its initial dispatch does not alone establish the per-geometry vertex transform or skinning basis. |

The existing builder evidence remains necessary:182D750 produces rotation-only
V, copies the camera translation separately to cache+210, keys the entry by
camera and useJitter, and prepass21F38B0 stores transposed inverse near P in
Context shadow+8A0. The post-prepass return21F9F78 is the admitted same-source
snapshot point. Near translation restoration at21F2F3E..21F2F6D and
21F8DAD..21F8DDC again copies3E622B8..C0; a finite retained cache is not evidence
that an arbitrary later value belongs to this draw. These new instruction
observations neither extend snapshot lifetime nor replace current-source checks.

Pinned CS `Depth.hlsli` deliberately converts the near interval back to near
projection depth, applies its saved near inverse and the world projection,
then writes saturated target Z/W at the composed pixel. `CanonicalDepth.cpp`
supplies those matrices and publishes an R32 product keyed to its world camera
frame. This establishes the reference's projection-only scalar convention.
It does not prove equal physical camera positions or explain the active native
vertex basis. In particular, copying the world rotation while separately
supplying near translation and a distinct world-derived draw-position triple
supports investigating a virtual layer, but is insufficient to declare one.

Bounded conclusion: static proof here does not select either model as the
correct native interpretation. The full transform is mathematically justified
only if both origin triples refer to the same physical coordinate space;
the cache layout alone did not prove that premise. Conversely, reference
projection-only normalization is an explicit composed view-Z convention, not
evidence that native near vertices occupy world-camera coordinates. The earlier
checkpoint95 generic `target-math` receipt, including its large world origin,
cannot identify which premise or numerical check failed. The current target
association correction may expose the already-added discriminating stages in a
later authorized run; there is no basis for a speculative production fix now.

The smallest next same-source receipt, if existing near-W/projection diagnostics
do not settle it, is copied at the admitted post-prepass return: source,
submission/native frame, camera/cache keys and jitter flags; both cache origins;
3E622B8..C0; both camera world translations; the world-parent local position and
3E5E0F0..F8; maximum near/world rotation difference; projection mismatch detail;
and full-map versus projection-only W coefficients/minima plus Z/W for a few
analytic clip probes. Record actual viewport/ranges and inverse freshness with
it. These CPU scalars distinguish coordinate and precision hypotheses, but do
not by themselves prove geometry ownership. To close that last premise, trace
the active owner+E0 virtual slot0x68 and one actual first-person batch's
object/skinning origin subtraction and bound shader constants through the
21D7120 path. Copy only scalar identities/transform facts within that draw's
lease, with no retained queue, vertex or native object pointers. A model change
still requires that producer/consumer proof and an explicit scalar convention;
it must not assert spatial reprojection or blanket FG geometry acceptance.

Ignored receipts: `near-basis-camera-xrefs-20261116.json`,
`near-basis-position-xrefs-20261116.json`,
`near-basis-position-callers-20261116.json` and the targeted `cpu2-*` instruction
files. Byte-pattern scans were search aids; only the aligned, locally checked
instruction chains above were used as evidence. All35 depth guards still match
the installed PE; the six frozen depth/camera/reader source hashes are unchanged
against `near-basis-source-freeze-20261116.json`. No build, fixture execution,
game/process/UI interaction, shared analysis session, deployment or commit was
performed. This is an unresolved static prerequisite, not a new live finding.

Build-status correction after that freeze: the root's scene/input/marker batch
now compiles the logical-target correction with MSVC Release VS2026. Background
CTest passed218/14 of232 selected tests, with four foreground tests excluded;
fmt/Clippy and hidden generic marker/scoped device fixtures passed separately.
This is not a full eng/verify pass and does not select a near-depth coordinate
model. Current artifacts are undeployed, with no new native snapshot/math/SR
acceptance. The prior source-only descriptions remain historical to their
investigations; see the render note's background-batch section for exact logs,
hashes and excluded tests. This correction is documentation only.

Latest scheduling update: the user reports the PC available again. The root
is running complete eng/verify Release VS2026 in
`verify-scene-input-marker-20261116.log`; its result is pending at this
documentation freeze. Root owns any subsequent deployment/game/UI work;
this worker performed none. The earlier desktop exclusions describe the
completed background run, not a continuing restriction.

## Native near-W receipt and explicit composed view-Z convention

New evidence: coordinator trial `native-raster-20261116-140447-020`, PID54248,
uses Runtime90E86B69D96B29D5D74F42E1A5DF61289FC0C4B3CD72EC4B1EC9C793F9F13178
and EntryDAC24A522E6596726DE933DFAC7CABC628A6DCB02FFC5751D0EF297ACBF0C39F.
`loader-world-first-refusal.log` records the corrected target association passing,
accepted resource/camera reads at native5212, ordinary source4733/native5213
and one SR evaluation/publication. This is transient input acceptance, not
sustained SR or image-quality/FG acceptance. Source4734/submission3/native5214
then refuses specifically at near-W, not projection mismatch or resource owner:

```text
near_origin = [-0.00769183971, 0.0319054201, 120.485939]
world_origin = [-80300.6641, 90804.2344, 7973.14453]
W = [-1.49011612e-08, -3.55271368e-15, 107072.234, -107071.539]
minimum_W = -107072.177263; near interval width = 0.00999999978
```

Actual/pending raster is0,0,2560,1600, descriptor_read1, descriptor2560x1600,
logical colour26/depth1 and physical colour22/22/depth2. The receipt proves the
shared-world near transform fails its forward-W bound for these actual copied
origins/matrices. It does not prove numerical instability or hardware-depth
corruption, and supplies no reason to widen a threshold.

The previously unresolved position dispatch now has installed RTTI/construction
proof. TypeDescriptor3073088 names `.?AVFirstPersonState@@` at3073098; complete
object locator2A53DE0 precedes vtable25AAB08, whose slot+68 points to1021050.
At102552E, `488d0dd3555801` loads that exact table;1025535 stores it in the new
object and10255B2 `488986e0000000` stores the object into owner+E0. Thus the
1026820 owner+E0 dispatch identified earlier is the constructed first-person
state entry, rather than an unresolved arbitrary camera-state guess.

Native1021050 reads the selected first-person camera node from state+50
(`1021063:488b5150`), copies its world translation+A0/A4/A8 at1021070..1021087,
and conditionally derives/subtracts state-specific offsets before returning the
three-float position at102120F..1021266. It is not an unconditional copy of
the world camera origin. Through1026820, the existing C38E6A stack output and
C3910E setter call supply3E622B8..C0, which the near-camera producer copies to
near+A0/A4/A8 and then to keyed cache+210 as previously proven.

The state node selection is also explicit:1021290 traverses the supplied node
hierarchy and writes the chosen camera/fallback to state+50 at1021391
(`48897d50`). Its10212E8/10212F3/10212FE stores initialize local rotation rows
from2F4C490/A0/B0, whose floats are [1,0,0,0], [0,1,0,0], [0,0,1,0].
Correction to a preliminary worker handoff: these writes cover rotation and
padding, not local translation+60, so they do not prove an identity whole-root
transform. Neither the selected node's small recorded translation nor this
rotation initialization proves the coordinate space of every skinned vertex.
The later21F95FF near-camera selection, separate world-parent draw-position
triple and21F963D batch execution remain the verified consumer order.

The game adapter now explicitly defines the near domain as **composed view-Z**:
undo actual jittered near projection, keep that local view-space depth, then
project using the validated unjittered world projection at the same composed
raster sample. In row-vector notation the map is
`inverse(PnearJ)*Pworld0`. This adopts the pinned reference's canonical-depth
intent under ABI23's scalar Z/W contract. It does not translate the local
first-person camera pose into the physical world, assert equal absolute origins,
or spatially reproject a first-person point. The earlier full near map was
algebraically valid for its assumed shared-world model; the actual receipt and
native independently supplied first-person pose no longer justify that premise
for this composed convention. All world-domain mapping remains the original
full `inverse(VPworldJ)*VPworld0`.

No source, interval, native inverse or projection validation was bypassed.
Capture still checks near/world origins for finiteness and exact freshness;
keyed matrices, source/frame/context/resource identity, jitter, actual raster,
depth ranges and the saved near inverse are unchanged. A new named near-basis
guard requires near/world rotation-only cache V to match at the existing
tolerance, consistent with the native copy producer. Recovered world P still
must match the direct jitter0 cache P at the unchanged0.0005 relative tolerance.
The near map uses that validated direct P to preserve native structural zeros,
rather than treating tiny inv(V)*VP roundoff as a new off-axis projection.
The game-local helper accepts only finite, invertible native forward perspective
forms. Canonical raster affine postmultiplication, explicit Z/W clamp and all
forward-W bounds, including the one-D24-step expansion and >1e-6 threshold,
remain unchanged. A true negative-W/invalid matrix continues to refuse.

The actual compiled camera fixture now calls the same helper used in production.
Its independent analytic cases use the recorded two origins and synthetic near
projection with stated near1/far1000, the observed world near15/far353468,
nonzero source jitter and view distances20/200/800. Expected target depth comes
from the analytic perspective view-Z equation, independently of matrix inverse
composition. A separate physical-origin transform with the recorded origins
must remain negative-W/refused; for a local distance40 its W is independently
predicted by40+aNear.z-aWorld.z. Singular, non-perspective, nonfinite and absent
inputs refuse and leave the composed map zero. Existing crop/history/jitter,
cache inconsistency and W-threshold regressions remain. These new compiled
cases were authored, not built or run by this worker.

Read-only PE/RTTI checks and an independent analytic origin/view-Z oracle passed
in `near-virtual-convention-proof-20261116.json`; detailed native getter/root
receipts are `near-firstperson-translation-20261116.txt`,
`near-firstperson-vtable-writers-20261116.json` and `near-root-*`.
All35 existing activation guards still match the installed PE, with no changed
expected bytes. Scoped diff checks passed. Source is frozen for root compilation
and review. No worker game/process/UI interaction, attachment, build, GPU run,
deployment or commit occurred. The root owns the live process and its close.
Sustained native admission, visual near-layer depth quality, object/skinning
coverage, later writers and FG geometry quality remain separate live/audit gaps;
one accepted scalar normalization must not be promoted to blanket FG acceptance.

## Recorded trial and composed-view-Z validation disposition

The motivating live refusal is retained in
`native-raster-20261116-140447-020/loader-world-first-refusal.log`: PID54248
passed reader source4732/native5212, then GAMEPLAY source4733/native5213
published one DLSS SR evaluation before aggregate HUDless refusal. The next
source4734/native5214 failed near-W with actual W coefficients/minimum and
small near/large world origins recorded. The aggregate HUDless message can
also represent framebuffer-read failure and proves no comparison defect.
There was no completed capture, sustained SR or near-layer/FG geometry-quality
acceptance. This trial used Runtime90E86B69.../EntryDAC24A... after the complete
scene/input/marker gate passed221/15 in144.40seconds, not the new composed map.

The actual `fallout4_camera_contract` test exercising the composed-view-Z
helper subsequently passed in0.01seconds, focused invocation exit0
(`camera-composed-view-depth-20261116.log`). This upgrades the helper's status
from authored-only to compiled and tested by the coordinator, with the numeric
scope above. It does not validate composed first-person depth in a game frame.
Initial complete `verify-composed-depth-hudless-20261116.log` failed only
`anomaly_overlay_visibility` at its SetCursorPos precondition; focused
`overlay-cursor-refocus-recheck-20261116.log` passed after normal desktop focus
without a source change. Complete `verify-composed-depth-hudless-r2-20261116.log`
is pending at this documentation freeze. The composed map and named HUDless/CPU
attribution remain source-frozen with no new full-pass or live acceptance claim.

The trial's NativeDesktopQuit exitedC0000005. Its30second passive trace expired
before the final UI action and did not capture that fault. Controller/disposition
receipts record20/20 restored entries, unchanged custom INI, prior user settings
SHA13D80689... preserved and no forced game kill. This close limitation remains
separate from the near-W math evidence; no new fault cause is assigned here.

## Later composed-depth execution scope, PID61128

Complete `verify-composed-depth-hudless-r2-20261116.log` subsequently passed:
exit0,221passed15skipped,171.65seconds, fmt/Clippy clean. This supersedes the
earlier pending r2 status in this note, not the separate latest padding batch.
The new padding repair is source-frozen, not deployed in PID61128;
`verify-reader-padding-20261116.log` is running/pending at this documentation
freeze and has no result assigned here.

Report `native-raster-20261116-143511-988` used Runtime90E86B69... and
EntryB403DF6A.... Root manual quality0..4 apply/status and evaluation receipts
show sustained DLSS SR execution at native2560x1600 and1707x1067/1485x928/
1280x800/853x533, all to2560x1600, followed by return to native. This improves
on PID54248's single evaluation but proves no composed first-person depth,
motion, visual or FG geometry quality. No complete capture exists; the manual
manifest remained capturing. Base FPS approximately4to6 even with SR Off has
no performance attribution.

HUDless owner-changed blocked FG trials. Exact deployed-object/image evidence
proves NativeState equality compared uninitialized padding; its semantic-field
repair is source-frozen in the next pending gate, not deployed here. Removed-MSG
acceptance and marker-order0 are not prospective reservation/pacing or latency
acceptance. Controller600scene/120quit timeouts and late root manual quality
trials/scene-continue provide no controller acceptance.

After native-Quit input did not complete, ordinary AltF4 produced the close
failure captured by fresh durable trace-close-3:4CB273 read0x50 with exact
baseline stack and NtTerminateProcessC0000005. This is not a normal/native-Quit
exit or evidence of a depth fault. Later exact-run recovery ending
84c7a0a118e143d6b24c41db1e459b4d supersedes post-close-disposition restored=false:
20/20 entries verified restored, custom INI unchanged, original user settings
SHA13D80689... restored after sealing the API-test mutation. Native borderless
prefs/custom INI were preserved. No new experiment or source edit was performed
by this documentation update.

## Native-scale matched capture after padding repair

The complete reader-padding r2 gate passed223/14 in119.87seconds with Rust
fmt/Clippy clean. Retain its first221/15/1 XeSS activity/generated_windows
failure and unchanged focused6.90second pass before the full r2 pass. Prior
pending padding status above is superseded by this bounded build/live evidence.

Report `native-raster-20261116-151505-988`, PID70400, Runtime90E86B69... and
Entry463B1EED..., has manifest complete/matchedPresent source4121/native4598,
16 captured images among30 entries, and an accepted native framebuffer reader.
Native depth and SR scene depth were captured at2560x1600. This closes the
missing native-scale capture fact, not the depth-domain/first-person geometry,
motion, broad quality or visual audit. The FG-Off HUDless image remains
unavailable; later live HUDless1 does not substitute a captured image.

Explicit FG-on foreground1/base54/output54/active0/tags0 remained refused.
Same-source inputs/HUDless/identity/matched/scene-supported/continuous and
render-complete were1, while CPU mask1/flags0/paced0/prospective0 and
owner_generation0 versus provider_generation2 showed absent bound/paced input
ownership. No resource/depth failure is inferred. Removed-MSG acceptance and
marker-order0 are not prospective input, FG or latency acceptance. World
observation around55fps carries no performance attribution.

The completed capture observation was followed by120second controller quit
cleanup timeout. Actual action was ordinary AltF4 after Escape did not open a
menu, despite observer filename trace-native-quit. It captured baseline4CB273
read0x50 stack and NtTerminateProcessC0000005, not normal/native Quit or a new
depth-fault attribution. Later exact recovery ending3103b81a21864e34ac33e15f1721a1b1
verified20/20 restored, custom INI unchanged and user settings SHA13D80689...
restored; native prefs/custom were preserved. This documentation update ran
no new experiment and changed no production source.
