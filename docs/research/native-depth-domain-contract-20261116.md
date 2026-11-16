# Copied depth domains for native input normalization, 16 November 2026

Built and device-tested by the coordinator as recorded below; ABI23 game
deployment and visual acceptance remain pending. No worker build, device or game
run, deployment, attachment or commit was performed.

## Question and decisions

The native audit in [fallout4-depth-domains-20261116.md](fallout4-depth-domains-20261116.md)
identified multiple stored depth intervals. The adapter owns their native
producer, classification, same-source freshness, clear rules and complete view /
origin / projection / jitter transforms. The runtime must not know the engine's
thresholds, symbols, cameras or policy. Reference processing is evidence about a
contract, not code copied into this implementation.

The coordinator rejected the initial proposal's per-frame GPU failure readback
and target-XY agreement requirement. The implemented operation is explicitly
**depth-only normalization at existing composed raster samples**, not spatial
reprojection or reconstruction of a world position. It writes clipped target
Z/W at the same texel. Different-FOV foreground layers remain a composed-depth
approximation for target-camera geometry; native motion coverage and actual
quality need independent validation. No whole-depth physical validity is claimed.

The initial supported mapped numeric domain is single-sample UNORM16/24 depth.
Finite format values, exhaustive disjoint classification, CPU projective bounds
and explicit clipping replace the abandoned unknown-sample readback design.
Descriptor-enabled float depth is refused. All-zero descriptors retain legacy
copying, including R32_FLOAT. No new setting, export, SDK owner or normalization
framework was introduced. Native engine texture contents remain unchanged.

## Exact adapter contract

`game_renderer.h` appends `rsf_game_depth_domains depth_domains` after the ABI22
`temporal_mask_weights`. Matrix data and all other facts are copied inline.
No adapter matrix pointer or camera-state pointer survives the synchronous lease.

| Structure | Exact fields in order |
| --- | --- |
| `rsf_game_depth_domain`,104 bytes | float classify_min/max; uint32 endpoint_flags/reserved; float viewport_xywh[4]; float viewport_min_depth/max_depth; float source_clip_to_target_clip[16] |
| `rsf_game_depth_domains`,240 bytes | uint32 struct_size/version/domain_count/clear_enabled; float clear_value; uint32 reserved[3]; domains[2] |

Version is `RSF_DEPTH_DOMAINS_VERSION=1`; endpoint inclusion bits are MIN1, MAX2.
All-zero means no mapping. Any nonzero byte requests mapping, including malformed
zero-header packets. Requested mapping requires exact240-byte size, version1,
count1..2, clear_enabled0/1, zero reserved and unused-domain bytes. Unknown,
partial or malformed descriptions refuse rather than silently use raw depth.
When clear is disabled clear_value must be0. Clear, when enabled, is an exact
comparison against the decoded UNORM sample and takes precedence over domains;
it writes target far depth, camera.depth_inverted ?0:1. A clear assertion must
exclude real geometry at that value. The runtime cannot verify that engine fact.

The enclosing render_rect supplies the active source-allocation crop. The camera
supplies the target projection and jitter convention, so neither is duplicated
in the descriptor. Actual resource descriptors establish dimensions and formats.
Viewport XYWH uses source-allocation pixels and must cover all active pixel
centers, with inclusive left/top and exclusive right/bottom. Fractional viewports
need not cover the integer rectangle edges. Each depth viewport is finite, normalized and strictly increasing.

Classification intervals are finite, ordered, inside [0,1] and strictly increasing.
The first starts0, the last ends1, and adjacent endpoints must be exactly equal.
Exactly one adjacent interval includes a shared endpoint, unless explicit clear
owns that single excluded endpoint. Double inclusion refuses even with clear.
Outer endpoints must be included or explicitly clear. No interval-width gap or
ambiguous domain is accepted. This is stronger than testing a few likely pixels
and does not manufacture a validity mask or discard unmatched geometry.

Each classify interval must lie within its sampled viewport depth interval,
allowing only one format-derived quantization step at an endpoint:1/65535 for
UNORM16,1/16777215 for UNORM24. This is tolerance for stored-depth quantization,
not a hardcoded engine threshold. The adapter still must establish semantic
ownership of quantized boundary samples. The runtime cannot distinguish two
native producers that wrote the same code with different projection semantics.

## Shader and arithmetic

Accepted mapped source views are R16_UNORM and R24_UNORM_X8_TYPELESS, including
their compatible D16/D24 and typeless allocation aliases. Require mip0 only,
one mip/slice/sample. No integer, float, filtered, sRGB or MSAA mapping path is
inferred. A compatible readable copy handles engine depth without SRV binding.

At source texel `(sx,sy)`, load once without filtering. After clear classification,
unmap sampled depth using `(stored-minDepth)/(maxDepth-minDepth)`. Reciprocal is
computed in float32 from the copied endpoints, not approximate decimal constants.
Quantization tolerance may place this result slightly outside0..1; it remains
part of the checked source-clip box rather than being silently remapped again.

Build a producer clip row vector:

```text
x = 2*(sx+0.5-viewport.x)/viewport.width - 1
y = 1 - 2*(sy+0.5-viewport.y)/viewport.height
z = (stored-viewport.minDepth)/(viewport.maxDepth-viewport.minDepth)
clip = [x,y,z,1] * source_clip_to_target_clip
output = saturate(clip.z / clip.w)
```

The matrix is row-major and HLSL uses row-vector `mul`. Producer clip includes
its actual jitter. The full supplied matrix maps it into the enclosing camera's
unjittered target clip, using D3D device NDC Z0..1 and visible positive W. It may
include source inverse projection, view/origin change and target projection.
Do not transpose both native matrices indiscriminately or assume identical views.
The shader does not use transformed XY for a coordinate write or agreement test.
Clipping target Z/W to0..1, including geometry outside target near/far, is the
explicit operation's semantic, not an invalid-sample error fallback.

CPU-only preflight runs before caller binding changes or backend resizing.
All elements must be finite and bounded; double Gaussian elimination rejects
singular or numerically unresolved pivots. It computes the source clip box from
all crop pixel-center extrema and the classify interval endpoints, using the
same float32 reciprocal constants as the shader. Conservative absolute float32
error envelopes enlarge the box, including subtractive cancellation during depth
unmapping. Each homogeneous component is affine in that box and reaches extrema
at its eight corners. Validation requires:

- finite absolute dot-product magnitude bounds no larger than1e20;
- minimum W minus64 float32 epsilons times its magnitude strictly above1e-6;
- bounded division magnitude no larger than1e30;
- finite constants/reciprocals, with the same conservative1e20 magnitude ceiling.

These restrictions prove finite arithmetic and safe positive division for every
supported input value, independently of actual GPU contents. They may refuse
ill-conditioned but theoretically invertible facts. They do not prove native
producer identity, motion coverage or physical meaning. No production UAV
validation flag, staging readback, Map, frame-delayed validity or CPU/GPU wait
was added. Test readbacks are separate.

## Existing path, ownership and provenance

`native_regions_prepare` extends its existing depth shader/constants and reuses
render-local R32 storage. Colour and packed motion keep their current treatment.
A CPU-only internal preflight helper is not exported from Runtime. The SR local
fast path is disabled for any requested descriptor. Both native SR and pure FG
capture pass the same copied facts into regions; FG maps before motion resolve.

All leased region textures are checked against canonical context-device identity
before cache mutation, copies or commands. Native SR/FG preflight likewise checks
its supplied texture devices before changing bindings. Immediate contexts only.
A non-SRV source is copied into a checked compatible readable allocation on that
device. Original resources are never written. Caller binding guards still restore
state on success and refusal. Cached allocations/views may be reused, but every
invocation updates all mapping constants; resource-address reuse is not a
same-frame content cache.

Existing FG capture copies normalized depth/motion into the immutable source
slot before region scratch can be reused. The full copied pass retains depth
facts. Later SR metadata cannot replace an explicit FG producer's pass; repeated
explicit leases with different facts become NO_FG rather than reuse stale facts.
A failed repeated explicit normalization marks any matching retained source
NO_FG through existing scene ownership, so an earlier successful capture cannot
silently authorize a later refused lease. No new public refusal API was added.
Later HUDless composition consumes that slot rather than rereading native depth.

History retains the descriptor. Changes in domain count, classification, sampled
viewport/depth interval or clear convention reset continuity. Per-frame matrix
changes update constants without forcing every frame to reset. The adapter's
existing camera/history identity still owns projection discontinuities and resets.
No pointer to an adapter descriptor or native Context is retained across source scope; only the copied facts survive.

## ABI and consumer audit

| Boundary | Source change / negotiation |
| --- | --- |
| GameABI |22 ->23, append-only pass tail; plugin entries and services negotiate the shared constant and full pass size. |
| D3D11 SR table |3 ->4, query rejects old table3 despite unchanged function-table size; Anomaly binding follows the macro. |
| D3D11 FG table |2 ->3, query rejects old table2; AC7 clients follow the macro. |
| Godot engine/native ABI |2 ->3 because vk_packet embeds the growing pass before image fields; old packet layouts must refuse. SettingsABI2 remains unchanged. |
| D3D11 host / render host / callback host | Setup layouts unchanged; Host8 and host setup1 stay. They already negotiate current GameABI through services/plugin_session. |
| Camera / frame | CameraABI1 and FrameABI2 unchanged. |

All rebuilt zero-initialized adapters preserve legacy behavior. Old binary
compatibility is not claimed. Nonzero descriptors explicitly refuse at unsupported
native SR12, D3D12 callback/render hosts, GL, FG12 HUDless/window and Godot
D3D12/Vulkan ingress. Native Vulkan and Unity packets otherwise translate into
native zero-initialized passes or frame records rather than mirror this tail.
The direct native_SR/FG C entry points still require coherent GameABI ownership
and their full-size checks. Scene/window/translucency/dump consumers use the
rebuilt shared header and bounded whole-pass copies; unrelated auxiliary or cloud
depth does not acquire this scene descriptor automatically.

## Focused device checks

The existing auxiliary_sr WARP target now exercises the real region path:

| Check | Verified fixture behavior |
| --- | --- |
|8x6 UNORM16 source, crop[2,1,6,5], complete two-domain split at decoded32768/65535 | Local4x4 R32 values match a numeric oracle; exact inclusion and explicit clear precedence work. |
| Non-affine Z=.5*z, W=1+.5*z, matrix changes X | Output is homogeneous Z/W at unchanged sample coordinates; differing XY is accepted. |
| Other domain Z=2*z-.5 | Explicit lower/upper clipping, not raw copying. |
| D24 codes7ffffe..800002 at decoded split800000/16777215 | Correct domain and value without approximate engine coefficients; source bits unchanged. |
| Fractional viewport covering centers but not outer texel edges | Accepted with numeric XY-to-Z NDC check; right/bottom edges exactly at the last center refuse. |
| One UNORM quantization-step endpoint tolerance | Accepted; larger sampled-range mismatch refused. |
| Bad size/version/count/reserved, gap/double inclusion, singular/nonfinite/overflow matrix, W zero/nearzero/negative, missing viewport coverage | Refused before mapping work. |
| Second WARP-device non-SRV depth | Refused without changing outputs/bindings or issuing a readable copy. |
| Requested mapping for R32 versus all-zero descriptor | Mapping refused; legacy R32 exact copy accepted. |

The existing scoped rendered-FG fixture now carries explicit D24-domain facts and
later SR uses a different depth transform, exercising real retained source slots
through scratch reuse and the existing tag/refusal assertions, plus a malformed repeated descriptor that must suppress tagging an already retained same-source capture. This does not add
a public texture inspection API or claim measured physical scanout. C11 consumer
checks assert104/240-byte layout, append offset, Game23 and FGtable3; real SR/FG
API tests explicitly reject previous tables. Godot consumer checks assert ABI3.

## Coordinator validation receipts, 16 November 2026

The first integrated gate, `verify-depth-domains-20261116.log`, failed during
compilation because the Windows `near` macro collided with three local fixture
variables. Renaming those variables to `first_domain` repaired that compile
failure without changing production behavior or undefining Windows macros. The
failed log remains retained alongside the successful rerun.

`verify-depth-domains-r2-20261116.log` completed Release VS2026 with exit0:
220 passed,15 skipped,107.24 seconds of tests, and clean Cargo fmt/Clippy.
Test152 `auxiliary_sr_device` passed in0.08 seconds. This is actual WARP execution
of the numeric domain checks above, not merely source review.

The explicitly rebuilt scoped fixture also exited0 in
`depth-scoped-fixture-20261116.log`:55 sources comprising1 bootstrap and54 test
sources,41 captures,39 SR evaluations,34 tags and34 generated callbacks.
Latency owner, paced frames and markers all remained0. Its existing native
usage430 rejection versus private usage20 success and application UAV assertions
also passed. These receipts establish synthetic/device behavior and preserved
source ownership; they do not establish physical scanout, latency improvement
or game image quality.

All logs are under the ignored `.local/fallout4-research/` directory. The matched
build hashes are:

| Artifact | SHA256 |
| --- | --- |
| Runtime | `228191101080FDB81D2FAFD83B804F6D698AB6764524C80EC521671166A8C88C` |
| Entry | `022392700C3A0DBEE2B175A728546C5680A51B544A85CF1B0F25783C37DF15FD` |
| Carrier | `E4A809E7A7A7D802C7C83691727575FC288EB515130D54D60B608BDD7CF22C98` |

This ABI23 build has not been deployed. The existing deployed PID37012 remains
the prior coherent ABI22 build, with focus acceptance refused; it supplies no new
game evidence for this change. A bounded trial with a proven adapter descriptor
is still required. Native classification, cache freshness, clear ownership,
later depth writers, native motion coverage and composed-depth quality remain
adapter/in-game evidence requirements.
