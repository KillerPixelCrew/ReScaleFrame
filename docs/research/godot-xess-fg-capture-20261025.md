# Actual XeSS FG output capture

## Question and approach

The Vulkan/DX12 XeSS provider initially exposed SDK present counts, which cannot
establish that the generated pixels are correct. Capture must observe the actual
SDK-produced resources with source identity and GPU completion, preserve the
existing depth/motion/HUD-less gates, and avoid assuming that array order provides
a temporal fraction.

The public Intel debug header supports presentation filters/corner tags, but no
resource callback. The installed `libxess_fg.dll` exports
`xefgSwapChainD3D12SetDiagnosticCallbacks` without a public declaration. A bounded
reverse-engineering pass examined that export before any call through an inferred
interface. No vendor binary was patched. The result is a private diagnostic path
for one exact binary, not a public Intel support claim.

## Static evidence

Target: XeSS SDK `v3.0.2`, revision
`8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`; FG DLL `1.3.1.78`, SHA-256
`EC5E0C65E075570C6EDE72618BB666D0BE0C2E10B2EA9762C0FE8CB8E375AB27`.
REA used Ghidra 12.1.4, analysis-profile digest
`90e13888e18b7d587414bb259f5e3dc44a7c9293bdf290ad8c459be524747d9f`.
Addresses below are RVAs relative to the inspected image base `0x180000000`.

- Export RVA `0xDDB0` forwards through context vtable slot `0xA8` to the setter
  at `0x20E1C0`. The setter accepts record versions 2 through 4, copying fixed
  sizes `0x60`, `0x68` and `0x70` bytes. Version 2 is sufficient. Evidence:
  `ev_063a06708ba00a8866cd9824884e23f082167d752fd05800a1cdfeedcd45f2cd`,
  `ev_c1105219c1c192b83d7fd95062f4f009fb480b39090af0983ce397932f77caa9`.
- Record offset `+0x08` is the caller cookie. Resource notifier `0x20AB10`
  invokes record `+0x38` for each populated input role with cookie, frame token,
  role index, cached resource descriptor and recording command list. The cached
  descriptor is `0x28` bytes, resource pointer at `+0x18`, state at `+0x20`.
  Evidence: `ev_51d705a5f4459bff084c245f062b3d2894bd7192ef171f1af79d8b2e414a9412`.
- Notifier `0x20AC40` invokes `+0x48` with output count and recording list, then
  `+0x50` with cookie, frame token, raw output kind, actual resource, recording
  list and zero-based output-array ordinal. The interpolation path at
  `0x20B290` passes the same output-resource array to the presentation scheduler.
  Outputs have transitioned from UAV to COMMON before this callback. Default
  debug features are off; the SDK also has a separate debug-forced failed-output
  presentation branch, so the notifier alone is not a universal success flag.
  Evidence: `ev_0f8c19df84dc6c34888fb924b54c4cb0cb9d5066c6e3d0eeb3c276660b7fc102`,
  `ev_01f2e6ba1dcc01d25d048213ed4868ed9f0816883e75731d1ef2adba64411cf0`.
- Token low 32 bits are the public SetPresentId value, stored at context
  `+0x138`. The high 32 bits are an SDK counter and remain opaque. The output
  ordinal is an array index, not a typed temporal factor. Evidence:
  `ev_49fef31ef6efb33f6893d2581f59283f5d208fc4f534fce3874f6a8ccf8429a4`.

Selected inline Evidence is retained in
`.local/godot/xess-fg-diagnostic-evidence.json` (approximately 97 KB). The REA
export writer returned an IO error, so normalized inline records and their
identities were retained through the filesystem tool. Ghidra Windows P0 refused
analyst-name mutation. No binary/database mutation was claimed.

## Implementation and lifetime

`runtime/vulkan/xess-fg/diagnostic-capture.h` installs a zeroed 96-byte version-2
record with only the proved input/output-count/output callbacks populated. It
requires the exact on-disk SHA-256 before registration. The observer is installed
only when `RSF_VULKAN_FRAME_CAPTURE_DIR` enables bounded capture. Normal use does
not invoke this private Intel interface.

The provider copies the complete source frame/camera record before SDK Present.
Callbacks bind a token only to that exact source ID. The SDK command list and
resource are borrowed for recording; the callback records a readback copy and
restores the original resource state, without closing, resetting or submitting
the SDK list. Readback allocations remain owned until the bridge's exact DX12
queue fence completes during checked retirement. There is a hard bound of 64
readbacks and 256 MiB. Capture failure produces a diagnostic error, never a
substitute image.

Generated images are named `fg-xess-generated-before-present`. They are the
actual SDK interpolation outputs passed to presentation, captured before their
copy to DXGI. `fg-xess-sdk-backbuffer-input` names the actual SDK backbuffer input.
The latter is not falsely described as an interception of native DXGI Present.
Sidecars retain source/session/view/generation, raw token/kind/ordinal, requested
and observed output counts, resource/list identity, provider hash and completion
boundary. Every sidecar keeps `fraction_established:false`: no typed SDK factor
was recovered. Independent half-step comparisons remain a test hypothesis.

Vulkan prepare also records `fg-depth` and `fg-motion-uv`; HUD-less records
`fg-hudless-color`. These use the existing FrameCaptureVk reader on shader-read
inputs, the same source/camera identity, and flush only after Vulkan and DX12
drain. The shared capture constructor's try/catch now lives inside the constructor
body, avoiding access to a member already destroyed during a function-try handler.

The status callback counter counts observed genuine output-resource callbacks
when this observer is installed. It remains zero without the opt-in observer and
is never derived from SDK present counts.

## Controlled runtime evidence

Windows standalone fixtures used the installed LLVM-MinGW compiler with warnings
as errors, the common named GPU lease, process-only extension-point mitigation
and Khronos core/synchronization validation. No SDK/cache DLL was modified or
staged by this experiment.

1. A metadata-only output probe established the callback's real BGRA8 1280x720
   resource, recording list, raw kind 0 and ordinal 0. An input probe established
   public role indices 0/1/2/4 and cached state 64 (NON_PIXEL_SHADER_RESOURCE).
   Initial filtering on the cached descriptor's `type` field suppressed backbuffer
   observation. That field is not a reliable cached-role discriminator; the
   separately supplied role index is used instead.
2. Static-color readback returned the exact expected RGB `[25,64,127]` for both
   SDK output and backbuffer input. This proved actual mapped GPU bytes, but a
   static scene could not distinguish interpolation from endpoint copying.
3. A moving 80x120 cyan rectangle advanced eight output pixels per source frame.
   Its plane motion was current-to-previous UV `[-8/1280,0]`, supplied at 640x360
   with render-pixel scale. Source frames 6/7 had current centroids
   `379.5/387.5`; previous positions were `371.5/379.5`. Actual SDK output centroids
   were `375.5/383.5`, preserving 9,600 foreground pixels, empirically matching the
   half-step in this control. Generated/current raw hashes differ. This is an
   observed result for the control, not an SDK-provided fraction.
4. The first rectangle fixture omitted COLOR_ATTACHMENT_READ access before a
   render pass with loadOp LOAD. Synchronization validation rejected it. The
   fixture barrier was corrected; the failed receipt remains preserved.
5. The final complete fixture captures both source frames with all three Vulkan
   guides plus genuine generated/backbuffer images. Depth is uniformly 0.5 and
   motion is the expected finite half-float `[-0.00624847412109375,0]`. Engine
   HUD-less bytes equal the SDK backbuffer after channel decoding. All 12 source
   presents, 11 SDK interpolation statuses, readback retirement and releases
   succeeded with zero validation errors.

Final compact evidence is
`.local/godot/xess-fg-complete-readback-20261025/receipt.json`,
`pixel-summary.json`, `stdout.log`, `stderr.log` and three representative frame-6
PNGs. Two complete source frames of raw inputs/outputs remain for the active
comparison. Earlier duplicate raw dumps can be removed while retaining their
compact receipts and failure logs.

This proves actual interpolation in a bounded native control and provides the
inputs needed for unchanged Godot image gates. It does not establish Godot
object/camera/UI acceptance, exported-player coverage or broad provider readiness.

## First actual Godot image gate

The first standard-editor object control passed the existing image and geometry
gates on Native SHA-256
`19BC537D1FF40A863C91690B2CAE39D005FD3760B65699DEECDF336A2B34F301` and engine
`DE9BA015EDC32412380BE6A57B59118BAC604517538C957371ADF6D8946145CB`.
It selected SR 0, XeSS FG 5, one generated frame and latency On at 1280x720.
The independent reference used FG 3 Off and 8x MSAA at the same output extent and
matching binaries. Both runs used pinned Khronos core/synchronization validation
and process-only legacy-hook mitigation. The exact SDK fingerprint guard admitted
the same Intel DLL inspected above.

Actual generated frames 12 through 15 and all matching depth/motion/HUD-less
readbacks were validated against independent source poses and rendered half-step
references. Material centroid errors were `0.2524`, `0.2162`, `0.2480`, `0.2019`
output pixels, below the unchanged `0.35` gate. Object motion geometric error p95
was at most `0.002594` pixels. Sidecars still report no SDK-provided temporal
fraction; the rendered 0.5 comparison is explicitly a test hypothesis.

Manifest: `.local/godot/visual-20261025-201542-693.json`. The generated case is
`visual/visual-20261025-201542-693-standard-sr0-object`, with `decoded/analysis.json`
and representative comparison PNGs; the paired reference has the same prefix and
`standard-reference-object` suffix.

The preceding reference attempt `visual-20261025-201412-631` failed the harness's
validation-DLL presence requirement before any FG 5 candidate ran. Its invocation
omitted the explicit layer environment; the log requested zero instance layers.
Rerunning with the pinned layer path, `VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation`,
`VK_VALIDATION_VALIDATE_SYNC=1` and implicit-layer filtering passed. This was an
invocation failure, not evidence of a XeSS rendering defect.

The standard-editor XeSS SR 5 plus FG 5 object control also passed on the same
Native/engine pair, with a fresh matching MSAA reference. Material centroid
errors were `0.2463`, `0.1401`, `0.2062`, `0.0819` output pixels. Motion error p95
was at most `0.002397` render pixels and raster-jitter metadata error at most
`0.000785` pixels. Its manifest is
`.local/godot/visual-20261025-201711-999.json`, with the matching generated and
reference directories under `visual/` using that prefix.

This establishes standard-editor SR0/SR5 object/count1/latency-On paths. Camera,
combined motion, C# and exported-player acceptance remain separate checks. No
gate was relaxed and no broader readiness flag was changed.
