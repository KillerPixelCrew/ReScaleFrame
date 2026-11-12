# Shared post-upscale sharpening

## Milestone scope

D3D11 production sharpening is connected for the Runtime pipeline used by
Anomaly and AC7. D3D12 sharpening is connected through `rsf_sr12`, with live
RDR2 and Unity Mono native-SR host controls. Milestone 3 adds Vulkan, GL option
forwarding and Godot, Unity URP and Minecraft settings surfaces. This supersedes the isolated prototype
status recorded on 2026-11-12; the shader foundation is retained.

Off is the default. It adds no dispatch or copy. Sharpness zero also skips the
pass unless MAS can raise it. The pipeline snapshots settings once per frame,
after which all filtering uses that snapshot. A failed optional pass logs once
and leaves the completed vendor output available for reinsertion.

## Modes and controls

The shared SR panel contains one Sharpening section:

- Off preserves the stock reconstructed image.
- RCAS uses OptiScaler RCAS with optional Motion Adaptive Sharpening and Contrast.
- Depth RCAS and Depth DAS use the distinct depth-aware OptiScaler algorithms.
  Missing depth falls back to RCAS; depth controls are disabled with a reason
  until depth is available. Depth is sampled using its supplied reversed-depth
  convention. Unknown near/far use nonlinear depth with the nonlinear defaults.
- Vendor native is offered for FFX backends (FSR2/3/4) only. It sets the FFX
  dispatch descriptor's enableSharpening/sharpness fields. Custom modes always
  set both vendor fields to zero. DLSS native sharpness is deprecated and is
  never written; XeSS has no native sharpness control.

All values apply live and save. Sliders apply at drag completion, with the
in-progress value held in the panel independently of the host snapshot. The
panel scrolls when expanded options exceed the available display area. Mode
Off hides inactive options. Vendor mode hides custom controls. RCAS alone
shows Contrast; depth modes alone show the depth controls. RCAS debug tint is
disabled until MAS is enabled. DA debug includes depth/edge tinting.

The keys are shared across Runtime hosts and are stored in the adapter's own
INI section. AC7 forwards these same values through its existing carrier
settings path and saves them in `[Rendering]` in its AC7 preferences file.

| Key | Default | Range / meaning |
| --- | --- | --- |
| SharpeningMode | 0 | 0 Off, 1 RCAS, 2 Depth RCAS, 3 Depth DAS, 4 Vendor native |
| Sharpness | 0.4 | 0..1 |
| MAS | 0 | Toggle |
| MotionSharpness | 0.2 | -1..1 |
| MotionThreshold | 0 | 0..100, motion texture pixels |
| MotionScaleLimit | 10 | 0.01..100, motion texture pixels |
| ContrastEnabled | 0 | Toggle, RCAS only |
| Contrast | 0 | 0..2 |
| DADepthScale | 35 | 0..600 |
| DADepthBias | 0.001 | 0..1 |
| DAClampOutput | 0 | Clamp decoded output to 0..1 when enabled, depth modes only |
| SharpeningDebug | 0 | MAS/depth tint toggle |

These are OptiScaler code defaults at `2fff542`, with Off explicitly selected
for RSF. D3D11 depth inputs are device depth, so the nonlinear defaults apply.
OptiScaler's 250/0.0015 defaults for already-linear depth do not apply to this
D3D11 input contract. Clamp output uses its direct meaning, without the upstream
inverted DAClampOutput semantics. Nonfinite values fall back to defaults;
finite numeric values clamp to the documented ranges and invalid modes/flags
fall back to defaults. Float persistence uses nine significant digits.

## Execution and ownership

`rsf_dlss_pipeline_on_frame` applies the pass after color-transport decode and
color fidelity, before output dumps and return. The pass reads the runtime's
completed linear HDR reconstruction and writes it back through an owned
scratch texture. Full D3D11 state is restored. Motion comes from the current
frame's resolved inputs, with explicit scaling to pixels. Alternate backends
provide their dense motion/depth through `sr_legacy_adapter`; DLSS's canonical
motion scale is converted to pixels. Exposure is the frame's original guide.

Vendor mode takes the separate `sr_legacy_adapter` -> `sr_bridge` ->
`rsf_sr_frame.sharpness` path into the actual FFX descriptor. Bridge translation
copies the entire frame, preserving sharpness. No adapter-specific sharpening
logic was added to Anomaly. The shared overlay configures the runtime pipeline
both on startup and after each saved intent; AC7 forwards through its carrier.

The independent C options contract is size/version checked. The frontend
settings append fields in ABI 3 (96 bytes), overlay stats/intent append fields
in ABI 15 (344/188 bytes), and the D3D11 SR table appends a setter in API 2.
Overlay service API 3 is required because its setup embeds the extended frontend
settings. Backend ABI 4 appends the native FFX sharpness field. Deploy the
matching rebuilt runtime, adapter and overlay binaries together.

## Shader provenance and color fixture

The three shader ports derive from OptiScaler revision
`2fff542e78e257a1e4ff5a1e21a69a166c78c335`,
`OptiScaler/shaders/rcas/precompile/rcas.hlsl`, `da_rcas_sharpen.hlsl` and
`da_das_sharpen.hlsl`. Source review found only these three sharpeners, matching
the supplied map. Each port retains attribution; see
`runtime/graphics/src/sharpening/THIRD_PARTY.md` for GPL and MIT notices.

HDR inputs use `pow(x*E/(x*E+4),1/2.2)` before filtering and its inverse after
filtering. Bounded encoding removes the need for RCAS HDR localScale
normalization; DAS also skips its normalization for encoded HDR. Before
inversion, HDR filtering is limited to the local encoded color range, except
in debug views. Flat HDR neighborhoods return the original pixel. Alpha comes
from the original center sample. The Contrast blend spans the entire 0..2
range. The RCAS upper limiter uses a negative epsilon with `min`, correcting
the upstream `max` that could leave a zero denominator at white.

Run `python eng/sharpening/fixture.py` to reproduce an analytic CPU fixture in
`.local/sharpening/fixture/`: `numbers.csv`, `comparison.png` and `comparison.ppm`.
The image's strips are original, linear-filtered and encoded-filtered, first
for a dark edge and then a bright edge. At exposure 1 and sharpness 0.8, the
selected dark sample 0.16 becomes 0.163421053 with linear filtering and
0.164759806 with encoding. The bright sample 80 becomes 80 versus 83.4427054.
Thus normalization suppresses sharpening at that bright sample while encoded
filtering still operates. This is analytic evidence, not game acceptance or
proof of matching perceptual strength across exposures.

`python eng/sharpening/spirv.py` compiles and validates embedded SPIR-V with
Vulkan SDK DXC and spirv-val. It uses the same raw-string HLSL compiled as
`cs_5_0` by D3D; DXC uses its required `cs_6_0` SPIR-V frontend target. Embedded
SPIR-V is embedded in the Vulkan compute helper; builds do not need DXC or the SDK.

## Acceptance boundaries

The D3D11 WARP fixture checks all three shader modes, HDR flat preservation,
bounded HDR edges, alpha, absent-depth fallback, SDR white, and disabled-path
bit identity/dispatch count. Shared settings tests cover every key's round
trip, invalid-value sanitization, ABI offsets and actual FFX descriptor field
assignment per mode/backend. Rust tests cover defaults, inbound/outbound FFI
mapping, visibility and drag completion. The hardware SR pipeline fixture
checks sharpening through production `on_frame`, including Off versus zero
strength byte identity. Full verification and shared-host fixture receipts are
recorded in the milestone handoff report. No game deployment or visual
acceptance is claimed by these synthetic tests.

Milestone 1 validation on 2026-11-12: MSVC Release verification passed with 199
executed CTest passes and 11 default opt-in skips (210 discovered), 66 Rust tests,
formatting and Clippy clean. Both required shared-host bridge fixture variants
passed with the release overlay. The FSR3 production pipeline fixture changed
an edge in all four non-Off modes and matched Off/zero strength byte for byte.
The switch fixture did not establish foreground-game DLSS activity, because
foreground ownership was denied. No game installation was modified.

## Milestone 2: D3D12 execution and hosts

`rsf_sr12_evaluate` applies sharpening only after a successful vendor evaluation
and before its existing copy to the game's output. It keeps normalized motion,
depth and any configured external exposure guide in NON_PIXEL_SHADER_RESOURCE
until the optional pass completes. Output enters and leaves the helper in UAV
state; scratch/output transitions order compute reads/writes and the scratch
copy back. Normalized guide states then return to their original UAV states.
The image remains unsharpened and is still copied back if preparation fails;
the supplied runtime logger receives one warning for that context.

The D3D12 helper uses the same `cs_5_0` raw-string shaders as D3D11. It compiles
lazily when a custom mode actually runs. There is one immutable root signature
and one PSO per algorithm, three shader-visible descriptor heaps and three
scratch textures. Root constants are copied into each command-list recording,
so a later CPU settings change cannot overwrite an in-flight frame's constants.
Only a retired slot's scratch allocation can be replaced on size/format change.
The existing caller contract drains slots before reuse and submitted work
before destruction. Off, zero strength without positive MAS and vendor mode
perform no custom dispatch, copy, barrier or allocation.

HDR filtering uses the frame's configured exposure guide and perceptual
transport. If no guide is configured, exposure is one. `input_sdr` sets the
helper's HDR flag to zero, so SDR inputs bypass the HDR encoding and inverse.
Native FFX mode goes through the existing common descriptor helper with the
new frame snapshot's sharpness; custom modes always supply zero. Vendor native
mode adds no custom post pass. DLSS/XeSS never receive native sharpness.

`rsf_sr12_set_sharpening` reuses the size/version checked options contract from
milestone 1. It copies a validated snapshot under a mutex and does not rebuild
the upscaler. No existing setup struct or ABI version changed in milestone 2.
The RDR2 host loads/sanitizes shared keys on its existing INI poll, fills panel
stats, saves intents through its frontend transaction, and applies the setter
to existing and newly created contexts. The Unity Mono callback host loads its
`[Rendering]` preferences, polls while evaluating, and saves the same keys
without rewriting engine-owned base settings. It applies changes directly to
its existing context and does not mark the context dirty. Both hosts expose the
existing ABI 15 Sharpening section. Engine-owned spatial/FSR1 rendering bypasses
`rsf_sr12`; the callback host hides the unavailable sharpening controls with a
reason until that engine path is connected in the later engine work.

The D3D12 WARP fixture enables debug validation and reports no warnings/errors.
It checks Off and zero-strength byte identity without a dispatch, flat HDR,
SDR white and edges, bounded HDR edges, alpha, missing-depth RCAS fallback,
vendor mode adding no custom pass, and three recordings before any submission.
With sharpness 0.2/0.6/1.0, aggregate edge deltas are
15.953113 / 21.837702 / 24.504733; they are distinct and monotonic, protecting
both inline constants and per-slot descriptor ownership. Common descriptor
tests use the installed FFX header and cover every mode/backend assignment.
The RDR2 frontend and callback persistence helpers round-trip all options and
preserve unrelated keys, including a depth bias below six-decimal precision.
The hardware host fixture changes to DA RCAS/DAS with earlier recordings still
unsubmitted, then checks backpressure, submission, slot retirement and Present.
These are synthetic/device results, not new moving-game visual acceptance.

A verification rerun exposed an unrelated thin-loader test fixture collision:
Windows reused a PID whose loaded-DLL fixture directory still existed. The
fixture now includes the run's monotonic uptime in its root and outside-target
names. This changes test isolation only; it does not change the loader or
remove any earlier fixture directories.

Milestone 2 validation on 2026-11-12: the final MSVC Release gate passed with
200 executed CTest passes and 11 opt-in skips (211 discovered), and all 66 Rust
tests, formatting and Clippy passed. The D3D12 device fixture reports zero
validation messages and zero failures; the strengthened per-pixel local-range
check also passes. RDR2 host fixtures pass on backends 1/3/4/5 while changing
sharpening with older recordings still unsubmitted. Both required D3D11 bridge
variants pass against the shared Runtime; foreground-game DLSS activity remains
outside that fixture's acceptance. No game installation was modified.

## Milestone 3: Vulkan and engine settings

The Vulkan helper uses the same three HLSL ports and embedded SPIR-V. Each
unretired recording owns its UBO, descriptor pool/set, scratch image and borrowed
image views. CPU frame IDs never authorize reuse. The owner calls
`rsf_srvk_retire_commands` only after GPU completion and command-buffer reset;
Godot forwards this for every buffer after its completed frame pool resets,
before deferred textures are freed. The callback is outside vendor evaluation;
putting it in command-buffer begin would re-enter the FSR4 split callback. Context
destruction requires drained work and releases views before vendor images.
A bounded 64-recording pool falls back to the completed vendor image if the
owner fails to retire recordings. Failure logs once through the supplied sink.

Vendor output enters and leaves the helper in GENERAL. Compute samples it in
SHADER_READ_ONLY_OPTIMAL, writes RGBA16F scratch in GENERAL, copies scratch back
through transfer layouts, then restores GENERAL for the existing engine output
copy. Borrowed motion, depth and optional exposure remain shader-readable.
Off, zero without positive MAS, and vendor mode bypass initialization, dispatch
and copy. FFX descriptor fields are overwritten each frame using the shared
mapping. FSR4 applies the helper on its continuation, after the D3D12 producer's
timeline wait and before the Vulkan output copy.

SRVK ABI 2 appends optional exposure, sharpening and a log sink to the frame.
Setup retains its original size, preserving offsets in embedding service structs. The split handoff ABI remains 1. Godot and Unity settings ABI 2 append
the 56-byte sharpening contract after the original 44 bytes. Changing sharpening
does not increment either host's SR-context generation. The GL options append
the same contract and update their D3D12 context without rebuilding it.
Minecraft uses integer thousandths in JSON/sliders, and millionths for depth
bias, then passes all 12 values through a JNI float array. Its native SDR flag
keeps HDR encoding disabled. Unity URP's engine-owned FSR1 control remains
separate because that spatial path does not dispatch the native temporal pass.

The Vulkan fixture exposed two details that the float D3D fixtures did not:
half-float quantization can flatten a weak linear-ramp response, so monotonicity
uses a curved HDR edge; upstream depth weights also have a soft floor that can
still alter a hard depth boundary. The shared DA ports now preserve the center
when gradient-compensated relative depth residual, after bias and scale, reaches
one. Debug views retain upstream tinting. On the RTX 4070 fixture the depth
boundary deltas fell from 53.125 and 60.5625 to zero for DA RCAS and DA DAS.
This is an intentional correction to meet the no-cross-depth-edge requirement,
not a claim that upstream uses strict rejection.

User controls and meanings are documented in [Sharpening](../sharpening.md).

Milestone 3 validation on 2026-11-12: the MSVC Release gate passed with 202
executed CTest passes and 11 default opt-in skips (213 discovered), plus 66 Rust
tests, formatting and Clippy. The three Vulkan CTest opt-ins then passed 3/3 with
runtime paths supplied. RTX 4070 device fixtures passed FFX2/3, DLSS and XeSS
pixel checks, plus six FSR4 continuation frames on the INT8 compatibility path.
The custom sharpening fixture passed with Khronos synchronization validation,
including three recordings submitted together with distinct UBO/descriptor
slots. Godot's custom editor built; Resource serialization and the Godot/Unity
native settings tests passed. Unity's managed/native layout fixture passed;
Minecraft passed 44 tests and built its JAR with the current native payload.
Both D3D11 bridge variants and RDR2 D3D12 host backends 1/3/4/5 passed with the
release overlay. Foreground-game DLSS activity acceptance in the switch fixture
was not run because Windows denied foreground ownership. No game deployment or
new visual game acceptance is claimed.
