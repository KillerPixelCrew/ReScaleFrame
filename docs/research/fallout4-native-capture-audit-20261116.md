# Fallout 4 matched native capture numeric audit

Question: are the retained depth, motion and colour products populated and
consistent with their declared formats and transformations, or does the nearly
white depth preview reveal a numeric defect? This CPU-only audit reads the
retained files, not the live process or GPU. It decodes original storage using
each image's `raw_metadata` row pitch, dimensions, format and subresource,
checks finite/range/quantile counts, and compares only compatible products.
The ignored analyzer and bounded JSON are
`.local/fallout4-research/audit-native-capture-20261116.py` and
`.local/fallout4-research/native-capture-audit-20261116.json`.

## Identity and scope

Capture directory:
`D:/SteamLibrary/steamapps/common/Fallout 4/ReScaleFrame/captures/website/scene-20261116-151647-391-100523078-70400-1`.
Manifest schema1 reports complete, session1, source/frame4121, native4598,
scope/submission1514, view/family2108834008320, viewport/window857388,
matched_present=true, present_result0, sr_result0 and backend1.
All captured images are2560x1600. `frame.json` and `engine_pass.json` agree
on source/session and render/output extents; the engine pass declares camera
valid, flags22, camera motion included and motion_to_uv[-1,-1].

Executable1.11.240.0 SHA256:
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
Coordinator attributes this run to Entry
`463B1EED47090204DBB301B0332CA5CB9E5938907A67274994D59980E5AED0C7`
and Runtime
`90E86B69D96B29D5D74F42E1A5DF61289FC0C4B3CD72EC4B1EC9C793F9F13178`.
Those DLL hashes are run provenance supplied by the coordinator, not inferred
from textures. The observed stationary shower interior at about55FPS is also
coordinator observation, not an independently measured frame-rate result here.

Selected retained SHA256 hashes follow; the JSON records every capture file:

| File | SHA256 |
| --- | --- |
| manifest.json | 1194f83ce902a186fc4368c05d4453fbaa1933b29b74ee997abd9fad426fd1fc |
| frame.json | 6e68fb20ebe4da67f4aea70f209eb18182120984b732c26b30d8f62be394a780 |
| sr_constants.json | 875737b4dd6d4368dc724e44cd56539f2e423b57d815a428523e5ca88c0c7c8c |
| native_depth.bin | 70703700d16f7f185240886d2fb3024539dda5549aea9d4b1f846bbfaa222ddd |
| sr_scene_depth.bin / fg_depth.bin | 503a5877ebd77e068aef29caa21f7f4e655e13978f6d3d9a475d5eb1b345b994 |
| native_motion.bin | 5d020a9a4e42ca92166c3ed0930130ee325e8c00759477d99991a4a34c590402 |
| sr_motion_decoded.bin | b2eea05d927a5a66f3c54bbc0c445437a89107b71a999b3dbdb48f26578b3c11 |
| sr_motion.bin | 407b43464dfc7de087cdd5f8ec3a8767eddb6f2b0e6dbe5d6d08506c948e0f1f |
| sr_reinserted.bin | 3953f9c0dd6808b891b12dfaecdd9728497cd350b25eb2ae07c8a433180f83ed |
| final_color.bin | bc1fe5b5f1a3de959e9e595d3f360e7602dede86b039e65eec1e5baf0250d41c |

## Depth

Native format44 is packed R24G8 typeless storage. The audit extracts low24
bits as UNORM24 depth and high8 as stencil, rather than interpreting the whole
word as float. Native and R32 normalized rows are10240 bytes; each raw file is
16,384,000 bytes, exactly row_pitch*1600. All4,096,000 samples in each depth
product are finite, positive and strictly below1, with no zero or clear samples.

| Product | Minimum | 1% | Median | 99% | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Native D24 | 0.8767715023 | 0.8774338888 | 0.9296010691 | 0.9765855656 | 0.9773488031 |
| Normalized R32 | 0.8755268455 | 0.8761959672 | 0.9288900495 | 0.9763491154 | 0.9771201015 |

`sr_scene_depth`, `sr_depth` and `fg_depth` are bit-identical R32 data. This
establishes consistent captured depth handoff for this source, not native FG
generation. Using the reference/source split float32(0.01), all4,096,000 native
samples fall above the near interval and below clear1; none occupies either
quantized shared endpoint code167772/167773. All stencil bytes are255. Constant
stencil supplies no near/world primitive ownership distinction in this capture.

As a conditional world-domain diagnostic, `(nativeDepth-s)/(1-s)` at
s=float32(0.01) differs from normalized R32 by at most1.22888796e-7, mean
absolute8.79342869e-8. This is consistent with the intended world interval
normalization and float transform rounding. It is not a fully replayed native
descriptor proof: `frame.json`/`engine_pass.json` do not serialize domain
intervals, transforms, actual fractional producer viewport or the near inverse.
The exact split/matrix cannot be recovered merely from a close affine fit.
The target world projection, jitter, render rectangle and near/far are serialized;
near15/far353467.562 and depth_inverted0 agree with the current camera contract.

The preview is device-depth log display, `log2(1+65535*z)/16`, so this populated
high-depth interval appears close to white. It is not blank, all-clear, zero,
NaN or an unexplained copied native-depth image. With no near-domain pixels,
this capture does not validate the composed first-person view-Z convention,
near absence detection in other modes, endpoint ownership or clear handling.
Those remain the limits recorded in the dedicated depth-domain research.

## Motion and coverage limits

Native format34 is R16G16_FLOAT,10240 bytes/row. Decoded format16 is R32G32_FLOAT,
20480 bytes/row. All8,192,000 components in each product are finite; there is no
invalid float sentinel. Native X is within +/-1.19209290e-7, with3,992,817 zero
X components. Native Y ranges from-4.76837158e-7 to-2.38418579e-7 and is nonzero
at every pixel. Thus no vector is exactly [0,0], although all are tiny.

`native_motion` and `sr_game_motion` match numerically exactly. Decoded R32G32
preserves every native half value exactly, which is the expected identity
storage decode, not a conversion to pixels at that stage. `native_sr.cpp`
sets resolve scale to `-renderDimensions*pass.motion_to_uv`; the serialized
[-1,-1] yields [2560,1600]. The resolved `sr_motion` equals native motion times
that scale exactly at all8,192,000 components, and `fg_motion` is bit-identical
to resolved SR motion. Resolved X is within +/-0.00030517578125 pixels;
Y spans-0.000762939453125..-0.0003814697265625 pixels. The SR constants then
declare [1/2560,1/1600] pixel-to-UV scale, camera motion included and no jitter
contribution. The two stage units/signs agree; comparing native UV directly
to resolved pixel values would be an invalid numeric-defect test.

The existing shader audit remains applicable: studied DFPrepass convention is
current-minus-previous NDC encoded through(-0.5,0.5), with adapter[-1,-1]
conversion to current-minus-previous UV. This stationary sample establishes the
declared processing arithmetic and tiny nonzero native writes, not the active
shipped shader's sign/scale under motion. It does not independently validate
camera or object motion, skinning/bone history, particles/translucency, near
first-person motion, jitter exclusion or disocclusion coverage. No zero-filled
motion substitution, unwritten-sentinel repair or full shader-coverage claim
is justified by these data. `frame.json` has flags4 and SR constants reset1;
this is not a sustained temporal-history validation.

## Colour, output and remaining gaps

Native R11G11B10 is decoded as unsigned floating R/G11 and B10, not UNORM.
All12,288,000 native RGB components are finite and positive, with channel
maxima3.1875/4.625/5.25. Native colour, the RGBA16F jittered source and SR colour
match exactly in RGB. Backend output and `sr_output` match exactly in RGBA16F;
their RGB maxima are3.06640625/3.86328125/4.63671875 and alpha is1 throughout.
This rules out a numerically empty or constant source/backend result.

Reinserted R11G11B10 matches floor quantization of the backend RGB at every
component:10,358,216 components decrease,1,929,784 are exact and none increase.
Maximum difference is0.12109375, at most0.96875 of a destination-format step;
mean absolute RGB difference is0.00219113609. This is a measured precision-loss
pattern, not evidence of a colour-space bug or an API rounding guarantee.
The final RGBA8 image spans0..1 with varying RGB means near0.571/0.590/0.559.
It follows native imagespace/UI processing, so direct HDR-to-final equality is
not an expected invariant and its differences do not identify a defect.

The manifest does not capture HUDless colour, FG scene colour or UI layers
in this particular matched handoff; it explicitly marks them unavailable,
despite the separately reported passing live HUDless-input callbacks.
Opaque/transparency layers, reactive/transparency/bias masks, cloud depth and
engine exposure are likewise unavailable. Captured normalized FG depth/motion
are not proof that these missing colour/layer resources or generated output
were audited. No movement sequence or temporal reconstruction comparison exists.

No concrete numeric defect is established by this one stationary frame.
Depth is populated and consistent with conditional world mapping, motion
decode/resolve is exact in its declared units, colour conversion/reinsertion
is accounted for numerically, and matched Present/source identity is retained.
First-person composed-depth semantics and the wider shader/motion/visual audit
remain unaccepted. The analyzer checked all declared file layouts and wrote
strict finite JSON with per-channel quantiles and hashes. No production edits,
builds, GPU execution, process queries/attachment, game/UI input, deployment or
target writes occurred. The capture was only read and remains retained outside
the trial's20-file restoration whitelist; the coordinator owns recovery.
