# Orchestrator

The reusable runtime owns plugin sessions, backend selection, history continuity, graphics interop,
latency services and presentation. Game plugins own detection, hooks, engine conventions and
reinsertion. Shared SR/FG APIs accept no game signature or private engine address.

The Rust crate `rsf-orchestrator` has one module per service under `src/`, and `include/rescaleframe/` holds
its public C headers. Start from `src/lib.rs`; the names say what a module owns:

- `plugin_session`: exact-build selection and prepare/start/quiesce/stop/status (the game ABI is the one in `sdk/game/include/rescaleframe/game_api.h`).
- `native_scene`, `native_window`, `native_cpu`, `render_links`, `frame_sequencer`: associate engine CPU, view, submission and window events and reject mismatched or ambiguous frames.
- `native_sr`, `sr_session`, `sr_bridge`, `dlss_pipeline` (and the `d3d11_*`, `d3d12_*`, `vulkan_*` hosts): prepare inputs and evaluate SR through the D3D11, D3D12 and Vulkan routes; same-adapter shared transfers.
- `native_fg`, `fg_session`, `fg_choice` (and the provider modules): independent FG providers, drained live replacement, input retirement and saved provider choice.
- `native_composition`, `native_translucency`: native composition and translucency inputs.
- `input_dump`, `input_dump_vulkan`: shared F10/API frame capture, named opaque PNG/raw images, camera metadata and completion tickets.
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

F10 captures a named frame bundle through the owning runtime. D3D11, D3D12,
Vulkan and the OpenGL/D3D12 bridge share the same storage and metadata service.
New integrations can submit additional named resources through the C API without
changing vendor backends. Unproduced HUD/guide resources remain explicit in the
manifest. See the [capture contract and evidence](../../docs/research/core-frame-dump.md).

## Test fixtures

The `test-fixtures` feature compiles the fixture exports the ctest executables call (`tests/graphics_fixture.rs` and
the `fixture` modules next to their owners). CMake builds that second archive only under `BUILD_TESTING` and links
it into the executables that set `RSF_ORCHESTRATOR_FIXTURES` (end of `tests/CMakeLists.txt`); the Runtime DLL and
the carriers always link the archive without it. `cargo check -p rsf-orchestrator --features test-fixtures`
compiles the fixtures.
