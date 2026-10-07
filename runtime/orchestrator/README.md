# Orchestrator

The reusable runtime owns plugin sessions, backend selection, history continuity, graphics interop,
latency services and presentation. Game plugins own detection, hooks, engine conventions and
reinsertion. Shared SR/FG APIs accept no game signature or private engine address.

| Module | Purpose |
| --- | --- |
| `plugin_session` | Exact-build selection and prepare/start/quiesce/stop/status with game ABI 13 |
| `native_scene`, `native_window`, `native_cpu`, `render_links`, `frame_sequencer` | Associate engine CPU/view/submission/window events and reject mismatched or ambiguous frames |
| `native_sr`, `native_sr_d3d12`, `sr_session` | Prepare inputs and evaluate SR through D3D11/D3D12 routes |
| `sr_bridge`, `sr_legacy_adapter`, `dlss_pipeline` | Same-adapter shared transfers and compatibility SR entry points |
| `native_fg`, `native_fg_d3d12`, `fg_session`, `fg_providers`, `fg_leases`, `fg_choice` | Independent FG providers, drained live replacement, input retirement and saved provider choice |
| `native_composition`, `native_translucency` | Native composition and translucency inputs |
| `gpu_policy`, `unity_sr_host`, `overlay_d3d12` | Actual-adapter Auto policy, Unity hosting and shared overlay drawing |
| `runtime` | DLL version entry point; other services are exported by their own modules |

Direct SR sessions consume matching frame/view records, pre-tonemap colour, device depth, dense
previous-minus-current render-pixel motion, jitter and exposure conventions. Callers own resources
and the graphics thread, and must drain submitted GPU work before switching/destroying sessions.
The D3D11 bridge uses ordered shared fences and three command slots, with CPU waits at slot reuse
or retirement rather than a wait after every evaluation. Failure paths still preserve ordering.

SR replacement is planned before releasing the active backend; refusal retains it. Accepted frames
reset history after switches, gaps, refusals and identity/resource changes. FSR family selection
must match the actual provider. Native XeSS D3D11 SR remains unimplemented; current XeSS SR uses DX12.

The FG facade keeps engine buffers stable across provider replacement after a drained Present.
DLSS-G, FSR3/4 and XeSS are independent from SR. Unity Off/FSR1 can normalize FG inputs without
reconstructing colour. DLSS SR/FG share one Streamline owner with distinct Unity viewports.
Pacing follows the FG provider: Reflex for DLSS-G, XeLL for XeSS, no NVIDIA SR sleep for FSR/XeSS.
Requested settings, effective settings, SDK activity and presentation counters remain distinct.

AC7 links shared runtime objects into its DirectInput proxy; Unity loads the runtime DLL through
the version shim. Product bootstrap, standalone launching and WSGM IPC remain planned.
[Current status](../../docs/current-status.md) separates released SR, current source features and
recorded game/device acceptance. See [SR evidence](../../docs/research/orchestrator-sr-switching.md),
[bridge/FSR4 correction](../../docs/research/sr-interop-performance-20261002.md) and
[shared FG corrections](../../docs/research/shared-fg-20261004.md).
