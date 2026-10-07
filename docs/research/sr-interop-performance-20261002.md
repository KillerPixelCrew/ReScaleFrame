# XeSS/FSR transfer cost and FSR4 compatibility, 2 October 2026

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

The user confirms the wing/cloud build is clean, then reports DLSS Ultra Performance running
about60 FPS faster than XeSS and FSR. The question is whether those backends execute at all,
whether the transfer path adds stalls, and why selecting FSR4 refuses.

## Evidence and reference comparison

The21:28 AC7 log selects XeSS2.0.2 at534x300 for1600x900, while DLSS and FSR use533x300.
Selection is not itself proof of evaluation. An explicit standalone run of the existing full
pipeline fixture switches DLSS, FSR2, FSR3, XeSS and DLSS again on the NVIDIA adapter; all
15 evaluations succeed. A separate frozen-input probe executes80 Ultra frames per backend,
reads the1600x900 output, checks finite/nonzero pixels and computes an output hash.

The local reference comparison used:

- OptiScaler `92337ebfbf07d533eabe3731fd8de1bb19437aea`,
  `OptiScaler/with_dx12/dx11_with_dx12.cpp:211-264` and
  `OptiScaler/upscalers/IFeature_Dx11wDx12.cpp:111-205`: GPU Signal/Wait handoff,
  three command allocator/list slots and CPU waiting before busy allocator reuse.
- Skyrim Community Shaders `dd2677fc4020db1da91b39cebef3dbfbe8913983`,
  `src/Features/Upscaling/DX12SwapChain.cpp:217-277`: shared GPU-fence handoff around
  a D3D12 presentation path. Its swapchain ownership differs from our mid-frame SR operation.

Our old bridge waited on the CPU after D3D12 reconstruction and again after the D3D11 output
copy. That serialized the render thread with earlier scene rendering as well as reconstruction.
The correction uses three command slots, waiting only before reusing a slot still in flight.
Inputs, reconstruction, output copy and the next frame's uploads remain ordered by the same
monotonic shared fence and validated immediate context. Current-frame completion waits are
removed. Explicit flushes remain because this is a mid-frame operation and cannot assume an
imminent game Present. No reference implementation was copied.

Dimensions/format changes, backend or quality switches, exposure-policy context rebuilds and
teardown drain the latest transfer fence before retiring resources. A failed SDK dispatch still
orders D3D11 after any submitted D3D12 work. Success means output is queued before subsequent
draws on the same context, not that the CPU has observed GPU completion. Logs now report the
first successful submission after backend/input-size changes.

## Standalone measurements

RTX4070 Laptop GPU, captured FP16 colour,1600x900 output, Ultra input533/534x300, fixed depth,
zero motion,20 warm-up iterations and60 timed iterations. These are single standalone runs with
unlocked GPU clocks and no AC7 scene workload. They are not an estimate of the user's FPS gain.

| Backend | Old CPU call mean | New CPU call mean | Old/new output hash |
| --- | --- | --- | --- |
| XeSS2.0.2 | 1.5682 ms | 1.3141 ms | `47c119414d7650bc`, identical |
| FSR3.1.5 | 0.9006 ms | 0.6338 ms | `5746e3c8e7775960`, identical |
| FSR2.3.4 | 0.8309 ms | 0.6150 ms | `b607e67b2e885715`, identical |

All240 evaluations succeeded, with no nonfinite output. GPU elapsed intervals include transfers
and submission gaps, rather than timing SDK kernels in isolation. Replay source/logs are ignored
under `.local/probe-sr-cost-20261002.cpp`, `sr-cost-baseline-20261002.log` and
`sr-cost-gpu-ordered-20261002.log`. The existing hardware bridge fixture additionally exercises
48 frames, command-slot reuse, quality/backend changes, exposure-policy rebuilds and output
readback on RTX4070. Earlier basic switching/readback also passed on Intel UHD Graphics.

## FSR4 refusal and the cross-vendor route

The unmodified installed AMD runtime enumerates only3.1.5 and2.3.4 on RTX4070. Explicit FSR4
therefore returns NOT_SUPPORTED and preserves the active backend. Its SHA256 is
`d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46`;
XeSS SHA256 is `251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7`.
Refusal logging now prints the provider list, and the overlay explains GPU/runtime unavailability.

This does not prove FSR4 cannot execute on NVIDIA. AMD officially limits4.1.1 to RX9000/RX7000
discrete GPUs, while the shared API uses FSR3.1.5 on other hardware.
[AMD requirements](https://gpuopen.com/amd-fsr-upscaling/).
[OptiScaler0.9.4](https://github.com/optiscaler/OptiScaler/releases/tag/v0.9.4) documents forced
INT8 support for NVIDIA/Intel and warns that internal FSR3 fallback must be distinguished from
genuine FSR4-i8 execution. Local OptiScaler source confirms runtime-caller-scoped adapter and
driver-capability substitutions plus an AMD extension-interface shim. Its model-quality hook
alone does not enable that route. ReScaleFrame does not yet implement this compatibility path.
It requires a bounded probe of the installed runtime's actual checks, effective provider/model,
GPU output and teardown before deployment. Do not relabel a fallback as FSR4.

Release gate:33 executed native tests passed, five vendor/environment skips; Rust format/clippy
passed. Full-pipeline and explicit hardware checks above are separate from those skipped CTest
entries. AC7 was not launched. Actual in-game performance and transitions require user retest.

Installed21:52 with backup `.local/deploy-backups/ac7-pair-20261002-215240-752`.
Proxy/plugin/overlay hashes verified; actual overlay Insert, pixels and resize passed.
The AC7 plugin hash is unchanged from the accepted wing/cloud build. No game launch.
The forced FSR4 INT8 path remains research only; this delivery improves XeSS/FSR transfer
synchronization and reports the default FSR4 refusal clearly.

## FSR4 INT8 compatibility implementation

The user subsequently requested implementation of the OptiScaler-style D3D12 transfer and FSR
compatibility handling. The shared GPU-fence bridge above is retained. For the installed AMD
binary, tracing OptiScaler's substitutions to their consumer found a narrower intervention:
the SDK's own INT8 capability predicate. This avoids changing global adapter/driver identity.

`amd_fidelityfx_upscaler_dx12.dll` is4.1.1.2740,28761864 bytes, with the SHA256 recorded above.
At RVA `0x8d70`, expected first16 bytes are
`48 83 ec 48 48 8b c2 48 85 d2 74 54 48 8d 54 24`.
`FFX_CheckInt8UpscaleDeviceSupport` takes an unused first argument and the D3D12 device as its
second, returning a Boolean in AL. It calls `FFX_QueryAmdAdapterAsicFamily` at0xed90, which gets
the real device LUID, checks adapter vendor and queries driver-private ASIC data. The predicate
accepts gfx11 family145 with matching revisions, including1. This is the same decision affected
by OptiScaler's caller-scoped vendor/GDI substitutions. Both functions are named/saved in Ghidra.

The separate `FFX_CheckFp8WaveMatrixSupport` at0x99460 remains unchanged and is also named/saved.
Missing AMD extension interfaces return false cleanly. The INT8 predicate does not require that
extension probe, so no fake AMD COM interfaces or broad successful extension stubs are needed.

The runtime enables compatibility only for explicit FSR4 requests on NVIDIA/Intel devices with
Shader Model6.6 and wave operations. Full file SHA256, size and the in-memory entry bytes must
match before installing a MinHook detour. It returns true only for the exact leased D3D12 device;
other calls forward to the original predicate. Radeon selection stays native. Unknown binaries,
failed preparation or other active SDK/device ownership refuse the compatibility route.
The provider enumeration and effective-version check still require an actual4.x provider.

The lease holds the D3D12 device through FFX context destruction. Dropping the last lease clears
the allowed device under an SRW lock. The SDK, detour and trampoline remain resident and forward
native capability answers without a lease. This deliberate process-lifetime code ownership avoids
freeing a callback/trampoline beneath an unrelated concurrent SDK query. Review found and removed
an earlier raw-lease callback race and a recursive cold-path mutex lock during last-reference
destruction. No system DXGI/GDI/kernel hooks or on-disk vendor DLL changes are installed.

The first RTX4070 probe enumerates/selects4.1.1 and evaluates successfully. A1600x900 Ultra probe
then ran80 frames with no refusals/nonfinite output. AMD's diagnostic watermark, enabled only in
that standalone process through `MLSR-WATERMARK=1`, explicitly reads **FSR4-I8 UPSCALE4.1.1,
DRS3.00X, SOURCE:LOCAL, COLORSPACE:LINEAR**. This establishes real INT8 execution rather than
an internal FSR3 fallback. The diagnostic PNG is `.local/fsr4-watermark-proof-20261002.png`;
its source is an older frozen scene capture and is not a new in-game visual test.

In that standalone run, FSR4 INT8 cost about2.52 ms per frame including transfer/submission gaps,
versus XeSS about1.34 ms and FSR3 about0.63 ms. These unlocked-clock measurements cannot predict
AC7 FPS, but INT8 FSR4 is not expected to be free on this hardware. First-use shader compilation
took substantially longer than warmed evaluations. No watermark is enabled in the game setup.

The final existing hardware bridge fixture passes84 frames including FSR4 native/Ultra,
FSR4->XeSS->FSR4 switching, repeated exposure-policy context rebuilds, slot reuse and output
readback. The full MSVC Release gate passes33 executed tests with five environment/vendor skips;
Rust format/clippy pass. The focused hardware runs are separate from those skipped CTest entries.
Actual moving-game FSR4 quality/performance and the first-use experience still require user testing.

Installed22:17, backup `.local/deploy-backups/ac7-pair-20261002-221700-274`.
Matching proxy/plugin/overlay hashes verified and actual overlay Insert/pixel/resize passed.
The full pipeline additionally passes18 evaluations across DLSS, FSR2, FSR3, XeSS, FSR4 and
DLSS again, followed by normal shutdown. AC7 was not launched. Restart normally and select
FSR4 in the overlay for moving-scene validation; the accepted wing/cloud plugin is unchanged.

## User acceptance and release replacement

After testing the 22:17 deployment, the user confirms it works and requests replacing the
broken v0.1.0 release with all required SR runtimes and the overlay. This resolves the latest
moving-game acceptance request. The accepted TrueSky/wing/cloud plugin remains unchanged.
No new numeric game FPS measurement was supplied. Non-NVIDIA AC7 visual coverage remains open.
The agent did not launch or stop AC7.

The release gate was repeated for packaging: 33 executed native tests pass, five vendor/environment
entries skip, Rust formatting/clippy and workspace tests pass. The packager now requires/copies
the AC7 game plugin as well as the proxy and overlay, includes the exact tested FidelityFX/XeSS
runtimes and notices, and supports validated hashes for all three first-party DLLs.
Release contents, source revision and per-file hashes are recorded in the package manifest.
FG, Reflex and unproven missing-object motion-vector improvements are not advertised.

The release follow-up adds the requested startup Insert hint through overlay ABI5. The host
owns an eight-second wall-clock lifetime, fades during the last second and dismisses it when
the panel opens. Rust paints text only, without pointer events, controls or a cursor. After expiry,
the host skips all hint GPU work. It uses the existing post-frame overlay renderer, outside SR.
The actual egui/D3D11 host produces1515 nonblack hint pixels with the panel closed, then passes
Insert open/close, settings rendering and800x600->960x720 resize. MSVC Release gate and53 Rust
workspace tests pass. No new AC7 launch: the accepted game plugin is byte-identical, while the
proxy/overlay change only for this requested hint and its input contract.
