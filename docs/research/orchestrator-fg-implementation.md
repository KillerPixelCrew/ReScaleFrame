# Frame-generation runtime implementation

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

30 September 2026. Work in progress against `0e2df170a9f276e24c76b75039065393b760ccd7`.
This records the implemented portion of the [FG plan](../frame-generation-plan.md), not completion
of that plan or working frame generation in AC7.

## Question and approach

Can ReScaleFrame expose one reusable FG contract and switch physical presentation ownership without
creating two flip chains for one window, inventing CPU event identity, or freeing live vendor inputs?
The runtime now has a C ABI provider contract, a bounded CPU frame ledger and a presentation-owner
controller. No game identifier, hook address or velocity encoding enters this controller.

The host supplies graphics quiescence, release/adoption and plain fallback callbacks. Selection
probes capabilities with no HWND while all graphics callers are stopped. A refusal preserves the
current owner. A supported candidate retires the old SDK's inputs and releases the old chain before
creating its replacement. Creation/configuration/adoption failure restores plain presentation.
Adoption must be transactional. A pending retirement fence refuses the transition and retains the
current context. The controller refuses selection while CPU frames remain outstanding.

The ledger carries an input-boundary identity and explicit event timestamps through input,
simulation start/end, render submission start/end and application Present start/end. It rejects
duplicates, missing/order-inverted events, decreasing timestamps and live slot reuse. Preparing
an interpolated frame also requires its input QPC to match the input event. Abort clears CPU
bookkeeping and vendor token ownership, without claiming GPU input retirement.

This is an integration API. The D3D11 swapchain facade, asynchronous interop leases and actual AC7
CPU hook sites and matching postprocessed HUDless/UI export are not connected yet. Existing root
UI promotion/recomposition remains completed work; it does not alone supply FG input leases.
The separately running motion agent's shader/capture work is preserved.

The [CPU boundary investigation](ac7-fg-cpu-boundaries.md) identifies static AC7 outer-loop/input
leads. Those sites are not installed and the renderer identity handoff still needs validation.
The asynchronous input lease pool retains COM resources through separate submission and vendor
fences. Its WARP test uses CPU-signaled fences to prove lifetime gates; it does not claim GPU or
vendor asynchronous scheduling. A submitted lease cannot be cancelled or overwritten.

## Vendor code and source evidence

- AMD SDK 2.3.0, `60f4ea81909200d8542eca14dccb2628b763a9a3`: direct signed
  `amd_fidelityfx_framegeneration_dx12.dll`, version enumeration and explicit family/provider
  override; FG API 4.0.1 and swapchain API 3.1.7. Analytical FG selected as 3.1.6 on this machine.
  FSR2 is SR only. FSR3/4 initially expose one generated frame, independently of SR selection.
  Configure precedes PrepareV2, which includes caller-supplied world-unit scaling and camera pose.
  Depth, jitter and HDR conventions are explicit setup fields. The SDK's default present callback
  handles HUDless extraction/composition; a copy callback is used only for declared no-UI input.
  Input resources remain borrowed until both caller GPU work and SDK wait-for-presents retire.
- Intel XeSS 3.0.2, `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`: XeFG 1.3.1 dynamically
  loaded with XeLL. Capability limits clamp the reserved interpolation count. Depth/jitter flags
  are supplied at initialization. Every prepared application Present has an ID, including
  disabled frames. RV_ONLY_NOW resources are copied on the supplied command list, so the caller's
  submission fence still governs reuse. Resume/gaps reset history. Positive warm-up warnings
  do not masquerade as fatal failures or active generation.
- Streamline 2.14.1 headers and retail package; matching inspected source tag commit
  `2122257e0fce486f91b385aa63b9a09b0a34b363` under ignored `.local/streamline/source`.
  `slGetNewFrameToken` uses six mutable slots; retrieving an old frame ID again advances its ring
  unless it matches the most recently minted ID. The host therefore mints once, retains the real
  SDK token, and refuses reuse of the next ring slot until that frame ends/aborts. Neutral 64-bit
  identities map to a monotonic native 32-bit sequence; exhaustion requires restart, never a
  truncation or unproved wrap. Independent callers must not mint tokens on this host.

The early Streamline host initializes before graphics activation, creates and immediately upgrades
its D3D12 device/factory, and creates the queue through the proxy. It refuses an already loaded
interposer. Native aliases are borrowed for same-device interop. The DLSS provider owns its upgraded
swapchain; its Present route supplies Streamline's common-present bookkeeping. Fixed, Auto and
Dynamic options are distinct and limits come from DLSSGState. Reflex Off/On/Boost and the independent
frame limiter are initialized explicitly; FG requires available Reflex and promotes Off to On.
Sleep occurs once at begin, including Off. Ordinary input sampling stays in the neutral ledger;
only real controller samples emit PCL marker 13. Deprecated numeric marker 6 is never sent.

One owner polls DLSSGState after Present. Its total-present delta is not relabeled generated-only.
FFX image-finalization callbacks have their own counters/validity flag; they are not claimed as
confirmed/displayed presents. SDK activity availability is explicit, including the HUDless path
whose SDK default callback does not supply first-party callback counts.
Cached input-processing fences govern retirement, including non-presenting producer queues.
Current shared D3D12 DLSS SR, PCL independent of FG and hot Reflex-to-XeLL ownership transitions
remain to be connected; loading the host is not proof these transitions are ready.
Subsequent [independent latency services](orchestrator-latency-services.md) add cold PCL/Reflex
profiles and standalone device API checks; they do not establish hot pacing transitions or game hooks.
FSR and XeFG currently refuse a loaded Streamline interposer with NEEDS_RESTART, preserving the
previous owner during preflight. Their standalone lifecycle probes do not establish safe
DLSS-to-FFX/XeFG hot transitions or independent PCL presentation instrumentation.

## Experiments and corrections

The device fixture creates a hidden test HWND, selects the adapter by vendor, uses 1920x1080
RGBA8 with three buffers, creates a provider-owned chain, configures Off, acquires the backbuffer,
presents, drains the application queue and SDK retirement, and destroys it. It never injects or
changes a game install. It does not invent depth, motion, simulation or input events. A higher
present count is not measured latency.

On NVIDIA vendor `10de`, device `2860` (RTX 4070 Laptop GPU):

| Provider | Device lifecycle result | Generated image / latency result |
| --- | --- | --- |
| DLSS-G | Passed early host, chain, Off Present, drain, teardown. NGX 310.9.1 / SL 2.14.1; max generated 1, dynamic unsupported, minimum dimension 100 | Not run |
| FSR3 FG | Passed chain, Off Present, drain, teardown; selected 3.1.6 | Not run |
| XeFG | Passed chain, Off Present, drain, teardown; 1.3.1, max generated 1 | Not run |
| FSR4 FG | Explicit request refused as unsupported on this adapter | Not run |

The same adapter also passed a controller-owned Off-only FSR3 -> XeFG -> FSR3 -> plain transition
fixture (`tests/fg_switching.cpp`). Each old physical chain was released before replacement and
each selected provider presented a disabled frame. This proves that ownership path, not enabled
interpolation, XeLL/Reflex timing or Streamline cross-vendor transitions.

The first FSR attempt used the general loader DLL, which did not enumerate the desired provider;
using the explicit framegeneration DLL fixes that. An early fixture then presented without
acquiring a proxy backbuffer. D3D12 validation reported a null barrier and corrupt CopyResource
source; GetBuffer before Present fixes the lifecycle test. This was a fixture correction, not
evidence of successful interpolation.

An invalid non-UUID test project ID prevented NGX initialization. The fixture now uses the same
valid synthetic project UUID as the SR fixture. The first DLSS chain attempt also tried to upgrade
an already upgraded factory-created chain; inspecting its native alias prevents that double
upgrade. These failures and their SDK output are retained in local development logs.

`tests/fg_session.cpp` passes synthetic ownership, capability refusal, plain recovery, retirement
refusal, CPU sequence failure/abort, duplicate preparation and bounded slot/timestamp tests.
`tests/fg_provider.cpp` is opt-in and skips in ordinary verification. Native Release verification
with VS2026 passed, including the C header compilation, 30 tests passing and four opt-in fixtures
skipped. SDK-free FG controller/lease tests also passed; the provider fixture skipped. That build
found a missing legacy no-SDK `rsf_dlss_release_viewport` stub, which is now present. The C compiler
also caught a provider-getter name colliding with an older typedef; the getter is `rsf_fg_get_provider`.
Game visuals, enabled FG, MFG selection, Reflex/XeLL timing, PCL ETW/Reflex tools,
resize/device loss, SDR/HDR/UI equivalence and long asynchronous retirement are still unvalidated.

Local logs: `.local/fg-dlss-lifecycle.log`, `.local/fg-xess-lifecycle.log`,
`.local/fg-fsr3-lifecycle.log`, `.local/fg-fsr4-lifecycle.log`, `.local/fg-release-verify.log`. Downloaded sources and runtimes remain
ignored. No reference implementation was copied.
