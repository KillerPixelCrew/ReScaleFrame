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
