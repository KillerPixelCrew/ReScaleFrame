# XeSS multi-frame capability audit, 25 October 2026

This is a source audit and implementation plan, not a new MFG experiment. Work
stopped at the user's wrap-up request before any proposed probe, analyzer,
capture-runner or negative-count fixture changes began. The existing count-one
implementation and its completed device evidence remain intact.

The question is whether XeSS FG counts 2 and 3 can be admitted and proved with
actual SDK output resources, independently rendered reference poses and explicit
time hypotheses. A higher status/present counter alone would not establish this.

The inspected SDK package is XeSS 3.0.2, revision
`8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`. Its local
`doc/xess_fg_developer_guide_english.md` explicitly limits non-Intel GPUs to one
generated frame. On Intel devices the maximum can also be overridden through
Intel Graphics Software. The guide recommends querying the actual context's
properties rather than guessing from vendor or an advertised multiplier.

The current NVIDIA device reports `max_generated_frames=1` in the completed
Godot receipts. In particular, `visual-20261025-210703-517.json` and its result
and runtime manifest record the SDK and binary identities, actual count-one
output callbacks and SDK maximum. That case is documented with its initial
guide-extent settling in
[the FG4 interop note](godot-fsr4-fg-interop-20261025.md). It is not positive
count-two or count-three evidence. No Intel MFG-capable adapter was available
for this audit, and no capability spoof or SDK override was attempted.

## Existing implementation

`runtime/vulkan/xess-fg/bridge.cpp` initializes
`maxInterpolatedFrames=XEFG_SWAPCHAIN_USE_MAX_SUPPORTED_INTERPOLATED_FRAMES`,
then obtains `maxSupportedInterpolations` from the SDK. The private
`RsfXessBridgeSetGenerated` path refuses a requested count above this maximum
and forwards supported changes through `xefgSwapChainSetNumInterpolatedFrames`.
The older `VulkanStoryXessFgSetGeneratedFrames` helper clamps values, but the
current Godot presenter does not use that helper.

The pinned diagnostic callback capture already records the requested count,
actual SDK output-array count, each output ordinal, copied source identity,
token, resource and command-list identities, and pixel readbacks after the owned
DX12 queue drains. It supports multiple ordinals within its existing budgets.
This is stronger evidence than a status counter, but its MFG behavior has not
been device-tested here.

`tools/analyze-godot-capture.py` currently accepts only a strict count-one XeSS
input/output producer pair. Its per-frame role map would overwrite multiple
XeSS outputs with the same role. It therefore cannot honestly accept counts 2
or 3 without changes. Existing count-one proof must remain the default.

## Unresolved admission edge

Source review of `integrations/godot/native/vulkan.cpp` found that the current
maximum-count guard runs when the requested provider already owns presentation.
A request for XeSS count 2 or 3 from another working provider can pass that guard
before the new XeSS context checks its actual maximum. This can retire the
working owner before discovering that the count is unsupported. This is a
source finding, not a reproduced runtime failure.

The proposed correction is to query actual XeSS properties before any cross-
provider HWND/WSI takeover, preserving failed probe-cleanup ownership explicitly.
The SDK documents `xefgSwapChainGetProperties` as reporting the maximum even
before proxy-swapchain initialization. No public ABI 1 structure needs growth.
This correction was approved for follow-up but was **not implemented** before
wrap-up.

## Proposed follow-up, not executed

- Add a size-checked private capability query and preflight the requested count
  against the actual matching device before settings, latency or owner changes.
- On current NVIDIA hardware, test refusal of counts 2 and 3 from both a working
  FSR3 owner and a working count-one XeSS owner. Preserve the working settings,
  session and WSI owner, then retain actual count-one callback/readback proof.
- Generalize analysis only with complete, unique SDK ordinals `0..N-1`, exact
  requested/observed counts, matching source/token/provenance and completed pixel
  readbacks. Add CPU metadata fixtures for missing, duplicate and mismatched
  records while preserving the count-one path.
- Permit independent reference substeps 3 for count-two hypotheses and retain
  substeps 4 for count-three hypotheses. Require explicit reference fractions
  for each selected ordinal. An SDK ordinal is not a temporal fraction; even a
  successful comparison would remain a declared hypothesis until SDK timing is
  independently established.

No new GPU captures, experiments or builds were run for this MFG audit. Positive
Intel count-two/count-three execution, image quality, timing and lifecycle
acceptance remain pending. The completed count-one camera case, strict
standard/.NET functional regressions and FSR4 negative-admission results are
separate evidence and are not relabeled as MFG support.
