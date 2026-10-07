# Orchestrator

The reusable runtime owns backend sessions, plugin lifecycle, history continuity, graphics
interoperability, latency services and presentation. Game plugins own detection, hooks, engine
conventions and output reinsertion. Shared SR/FG APIs accept no game signature or private engine
address.

## Source map

| Module | Purpose |
| --- | --- |
| `runtime` | Immutable DLL version entry point; no initialization; other services are exported by their own modules |
| `plugin_session` | Exact-build selection and load/detect/prepare/start/quiesce/stop/status with game ABI 13; retains busy owners until work retires |
| `render_links` | Bounded copied CPU-to-queued-render tickets with exact key/generation retirement |
| `sr_session` | C ABI over prepared D3D12 frames; transactional FSR2/FSR3/FSR4/XeSS selection and status |
| `sr_bridge` | Same-adapter D3D11/D3D12 copies, shared fences, resource states and GPU-ordered SR output |
| `sr_legacy_adapter` | Private conversion from the existing pipeline into SDK records and prepared inputs |
| `dlss_pipeline` | Process-wide D3D11 decode/evaluate/output pipeline, alternate SR selection, layer history, colour policies and paired diagnostics |
| `frame_assembly` | Legacy decoded-camera/resource validation and conversion to a DLSS frame |
| `native_sr`, `native_regions` | Native D3D11 pass validation, region conversion, exposure normalization, output copy and history |
| `native_sr_d3d12` | Native D3D12 input normalization and optional SR; three caller-retired descriptor slots |
| `native_translucency` | Opaque/translucent snapshots, reactive/coverage/bias hints, optional cloud depth and material mip bias |
| `native_cpu` | Bounded real CPU event records and an optional vendor pacing/marker sink |
| `native_scene`, `native_window` | Exact final-scene/window ownership, scoped COM identity matching and paired real Present observations |
| `native_composition` | Demand-driven D3D11 scene/UI/composed snapshots from one engine UI boundary |
| `frame_sequencer` | Thread-safe seven-stage input/simulation/render/Present CPU ledger |
| `fg_session`, `fg_providers` | Provider routing and quiescent physical-chain replacement with plain presentation recovery |
| `fg_leases` | Retain D3D12 inputs through separate application and vendor retirement fences |
| `native_fg` | D3D11 integration of real CPU/RHI ownership, shared D3D12 transfers, FG eligibility and latency markers |
| `native_fg_d3d12` | Native D3D12 capture/submission/backbuffer association and provider retirement |
| `fg_choice`, `gpu_policy` | Persisted FG intent and actual-adapter SR startup preference; neither proves device support |
| `unity_sr_host`, `overlay_d3d12` | Explicit Unity plugin/SR lifecycle, settings requests and shared D3D11On12 overlay drawing |

## SR contracts

Direct SR consumes dense previous-minus-current render-pixel motion, device depth, pre-tonemap
scene colour, render-pixel jitter, explicit exposure convention, and matching SDK frame/view
records. The caller owns the resources and graphics thread and completes submitted D3D12 work
before switching or destroying a direct session. The D3D11 bridge orders transfers on shared GPU
fences and uses three command slots, with CPU waits at slot reuse or resource/backend retirement.
Failure paths preserve ordering. A successful bridge evaluation queues output on the D3D11
immediate context; it does not mean the CPU observed GPU completion.

SR replacement opens and plans the candidate before releasing the active backend. Refusal retains
the old backend and is reported separately from evaluation failures. The next accepted frame
resets history after selection, gaps, refusals, view/session changes and resource-generation
changes. Separate paths and optional provider IDs let FSR SDK releases coexist; requested family
and actual provider version must agree. XeSS in the D3D11 compatibility path uses the D3D12 bridge;
there is no native XeSS D3D11 SR provider here.

Public preset selector/getter declarations in `dlss_pipeline.h` currently have no implementation
in this source tree. The compatibility pipeline uses its internal Auto preset request.

## FG, pacing and lifetime

DLSS-G, FSR3/4 and XeSS FG are selected independently of SR. Unity Off/FSR1 can normalize FG inputs
without reconstructing colour. DLSS SR/FG share one Streamline owner with distinct Unity viewports.
Pacing follows the FG provider: Reflex for DLSS-G, XeLL for XeSS, and no NVIDIA SR sleep for
FSR/XeSS. Requested settings, effective settings, SDK activity and presentation counters remain
distinct.

FG and latency use source identities from real CPU events and queued renderer/window ownership.
They require complete matching input/submission evidence; an SR evaluation counter or native view
number alone cannot establish a simulation/input-to-Present association. Native D3D12 SR currently
uses `pass.native_frame` in its captured record, so a producer using its automatic FG capture must
match that ID to the source ID used by CPU/window events. Native D3D12 capture records commands;
`rsf_fg12_submitted` separately confirms they reached the owning queue.

The FG facade preserves engine buffers across provider replacement after a drained Present.
Physical-chain switching requires host quiescence, vendor retirement, and release of all old
physical buffer/view references. A capability-probe refusal preserves the old provider. Failure
after detachment attempts plain recovery and may leave presentation unavailable if recovery also
fails. CPU abort does not retire GPU resources. Lease reuse requires both application and vendor
fences; a failed retirement query deliberately prevents reuse. Plugin stop failures retain their
module and host callback storage for retry. A live SR bridge that cannot confirm GPU completion
may keep its resources until process exit.

## Hosting and validation

AC7 links shared runtime objects into its DirectInput proxy. Its compatibility pipeline starts on
DLSS before alternate SR selection; the carrier supplies engine identity, world-unit scale, hooks
and output reinsertion. Insert requests are applied on the render thread. Direct SR APIs can start
on FSR or XeSS independently of DLSS.

Unity loads the runtime DLL through the version shim. Its explicit startup prepares the plugin
outside `DllMain` and initializes graphics from the actual render callback. Auto selects SR from
the owning D3D12 adapter. UI requests become replacements at the plugin's completed GPU boundary.
Native FG interception is installed before graphics creation and has process-lived callbacks.
Product bootstrap, standalone launching and WSGM IPC remain planned.

[Current status](../../docs/current-status.md) separates released SR, current source features and
recorded game/device acceptance. The published v0.1.0 AC7 package contains SR and excludes FG and
Reflex. Corrected AC7 SR and DLSS-G have recorded user acceptance; Unity XeSS/DLSS-G and the final
FSR correction also have recorded acceptance. New AC7 FSR/XeSS FG acceptance, broader Unity
scene/resize/teardown coverage, higher MFG counts and Claw operation remain unproven. These results
do not establish general plugin readiness or measured latency reduction.

See [SR switching evidence](../../docs/research/orchestrator-sr-switching.md),
[bridge/FSR4 correction and acceptance](../../docs/research/sr-interop-performance-20261002.md),
[FG controller evidence](../../docs/research/orchestrator-fg-implementation.md),
[independent latency services](../../docs/research/orchestrator-latency-services.md),
[shared FG corrections and acceptance](../../docs/research/shared-fg-20261004.md), and
[Unity D3D12 evidence](../../docs/research/unity-dx12-runtime-20261004.md).
Earlier pending statements apply to their dated experiments; follow later corrections and current
status for present claims. Documentation updates do not change those validation results.
