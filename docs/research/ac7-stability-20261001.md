# AC7 reconstruction stability, 1 October 2026

## Follow-up retest and producer ordering, 23:19

The user confirms both background and clouds are stable after the 22:52 build. Residual
reports are moving bloom, dark aircraft colour steps at Performance/Ultra Performance,
edge glistening and cloud colour smearing onto front steering surfaces when they overlap.
Three new captures at 22:57, 22:58 and 22:59 contain hangar DLSS Ultra, native comparison
and flight DLSS respectively. The flight successful input is 928x522; it must not be
described as an Ultra capture. Native comparison has no successful vendor-frame dump.
The hangar Ultra input is 533x300 with dense motion enabled, history reset0. This run
confirms the earlier generic claim that dense motion alone would solve smearing is unproven.

Source and capture topology show bloom generated from jittered SceneColorHalfRes before
the late SR node. Stock UE4.18.3 places temporal scene AA before this producer. The new
graph correction finds the native downsample descriptor method at RVA0xfb7a00, verifies
its constructor DebugName at node+0xc8 is SceneColorHalfRes, and requires its input ref
to equal the tonemapper's scene ref. It then feeds both that producer and tonemap from
the same owned SR node. Bloom and histogram/exposure descriptors derive their new sizes
from SR output through native ComputeOutputDesc; native view/scene state switches at
SR execution before those consumers run. Existing upstream scene/depth/reflection
dependencies remain unchanged. This is not a bloom removal or final image translation.

The earlier ordering correction finished all tonemap auxiliaries before SR to preserve
their low-resolution geometry. That was correct for a late insertion, but preserved the
unstable bloom source. In the verified shared-source path, those auxiliary dependencies
now belong after reconstruction. Current exposure cannot be an SR dependency because
it would form a graph cycle. The plugin uses the engine-owned exposure buffer already
available before this update, leased through the SR execution. Effective view state is
obtained with native getter0x1126090; its eye manager at +0xbd8 selects owned pool at
+0xbe0 or +0xbe8. No exposure buffer is fabricated or allocated by reading these fields.
The current native exposure update still feeds the game's tonemapper. If the common
source cannot be verified, the existing late insertion remains; it is not credited with
a bloom fix. F9 now preserves the actual supplied 1x1 exposure guide as raw data.

Eight distinct Gaussian jitter offsets recur in the captures despite the old console
setter's reported32. PreVisibilityFrameSetup reads the render-thread sample setting
and invokes the native view-state getter through virtual slot+0x60. The getter at
RVA0x10ecdd0 returns byte+0xc1c; count is +0xc1d. Ghidra disassembly/source match the
native TemporalAASampleIndex/Count pair; FrameIndexMod8 at +0xc20 is separate.
Hook46 replaces only this producer consumption in the selected primary pre-visibility
scope, sets phase/index from native family frame identity and
round(8*outputArea/renderArea), clamped8..255. Engine Halton/Gaussian sampling,
projection/derived matrices and pending previous-view storage remain native. Typical
Performance/Ultra cycles are32/72. No driver preset is forced. The legacy console
sample-setting route is skipped under native ownership. Getter/body was created,
named and saved in Ghidra; its return byte is zero-extended in the real instruction.

MSVC Release and actual-overlay Insert/pixels/resize preflight passed. Installed backup:
`.local/deploy-backups/ac7-pair-20261001-231912-182`. No autonomous game launch or new
tests. Bloom, sampling and exposure behavior need a game retest. The older frozen-input
precision experiment already reproduced colour steps independently of input/output
storage formats and exposure toggles; this change does not claim to fix that result.
Missing-object coverage and cloud-overlap boundary improvements remain after glistening
under the user's priority order. The shutdown-only vendor fault remains separate.

The user reports the hangar camera-pan crash no longer occurs after the 22:34 recording
ownership correction. They report severe shimmer, sky vertical bounce and sky smearing over
the aircraft, especially at Ultra Performance. Other upscalers appear to show the fallback.
This investigation separates producer sampling, motion validity and backend acceptance using
the seven 22:37 to 22:39 F9 sessions from the same run, with consecutive route frames.

The executable SHA256 remains
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`; dump SHA256
`fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`.
UE4.18.3 reference revision is `0a14a8d537a31ecc77488ced41dbaa0166612ef8`.
Local captures and analysis scripts stay ignored. Ghidra selected Ace7Game.exe.dump,
base `0x7ff741350000`, before analysis and naming.

## Packed colour rejected by FSR and XeSS

Raw input metadata identifies DXGI format26, R11G11B10_FLOAT. The native region copy
preserved that format. The D3D11/D3D12 SR bridge explicitly requires RGBA16_FLOAT for
colour and output, returning invalid argument before vendor evaluation. This matches repeated
result -1 and the 120-failure circuit breaker in the run. Sessions3/4 have final images but
no successful backend dumps. The fallback-looking appearance therefore has a concrete
acceptance failure; it is not evidence that the vendor algorithm itself failed visually.

The runtime now converts scene RGB to linear RGBA16_FLOAT, alpha1, while cropping the
active render rectangle. Depth becomes R32_FLOAT and packed motion remains unchanged.
The conversion runs before backend evaluation under the existing graphics-state scope,
including exact-size packed inputs that previously bypassed region preparation.
No tone mapping or exposure transform is applied. This is a runtime interoperability
responsibility; the plugin still supplies the engine scene colour.

## Cloud projection handedness

Paused session7 contains seven consecutive reconstruction route images. A gradient-based
small-displacement fit in the upper cloud region estimates backend vertical changes from
-2.48 to +2.37 output pixels. The lower aircraft region instead remains approximately stable
(largest fitted vertical change0.21 pixel). The input cloud and aircraft regions move in
opposite vertical directions. These are approximate regional fits with lighting and image
quantization, not calibrated motion ground truth. They reject a blanket global jitter-sign fix.
History resets are zero in the sampled successful frames except the first frame after a
backend transition in session5. Repeated history resets do not explain these sampled frames.

Tracing StaticRenderFrame's resolved function pointer led to native TrueSky RenderFrame,
RVA `0x222afa0`. It copies post-opaque projection at parameters+0x50 and invokes projection
adapter `0x2225470` before the DLL boundary. Ghidra shows that adapter negates projection
M20 and M23, but not M21. Dividing by the negated perspective W reverses vertical projection
jitter while the explicit X negation preserves horizontal jitter. Stock non-jittered symmetric
projection has M21=0 and does not expose this omission.

Expected-entry hooks now scope this correction to native TrueSky RenderFrame's primary view:
Uid at parameters+0xb0 must pass the existing view filter; projection M21 must match cached
TemporalAAJitter.Y at +0x724 and M23 must equal1. After native adaptation, M21 is negated.
The copy belongs to the sky producer, so no shared opaque-view matrix is modified. Original
world/unit conversion, time, textures and cloud animation remain owned by TrueSky. The
adapter has one direct caller in the selected binary. Both functions were named/saved in
Ghidra. No cloud depth or wind vector implementation is claimed by this correction.

## Preserve valid zero aircraft motion

Session6's decoded half-float vectors have valid(0,0) values over the camera-tracked aircraft.
In the sampled lower central ROI, 59.94% are zero and 40.06% are the unwritten -1000 sentinel.
The ROI includes surrounding pixels and is not an aircraft segmentation mask. The deployed
Streamline motion resolve treats any all-zero vector as missing and substitutes camera
reprojection. Earlier notes already identified this zero/sentinel ambiguity; these captures
make it relevant to the user's sky-over-aircraft smear report. Its precise share of the visual
defect still needs controlled runtime comparison.

The runtime now uses its existing explicit-sentinel resolve for DLSS sparse inputs. Unwritten
pixels use the current frame's depth and unjittered ClipToPrevClip; written pixels, including
valid zero, retain total object/camera motion. Current-minus-previous UV becomes previous-
minus-current pixels using negative render extent times the canonical scale. DLSS receives
the dense texture with cameraMotionIncluded=true and reciprocal render dimensions as scale.
FSR/XeSS already use this explicit resolve. Owned resources rebuild on size changes and
release with the pipeline. F9 adds motion_submitted alongside raw and decoded vector dumps.
No object-selection gate, skeletal history or depth dilation change is included here.

## Validation and outstanding work

MSVC Release proxy/plugin builds passed. Both new region shaders compiled with the installed
D3DCompile cs_5_0 compiler. The existing real-overlay deployment preflight passed Insert,
visible pixels and resize. Installed backup:
`.local/deploy-backups/ac7-pair-20261001-225225-566`. No game launch or new tests.
The three corrections are built and installed, not yet game-validated. Required comparisons
are DLSS Ultra Performance aircraft edges/cloud bounce and FSR/XeSS evaluation acceptance.
Remaining work includes object/weapon coverage, custom velocity projection, boundary dilation
and effective cloud depth/motion. The vendor exit fault recorded in sl.common during
LdrShutdownProcess is separate from the corrected hangar draw crash and remains unresolved.
