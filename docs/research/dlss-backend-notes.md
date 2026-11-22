# DLSS backend notes: motion data, Streamline and Wine

Moved from `runtime/backends/README.md`. Research history for the Streamline adapter; the adapter itself now lives in the Rust orchestrator (`runtime/orchestrator/src/streamline_sr/`, `streamline_host/`) and the Streamline SDK headers are read only by test fixtures.

## Motion and frame data

AC7's velocity texture is sparse: moving objects write values, while unwritten pixels carry a clear-value sentinel. UE4 stores velocity as `v * (0.499 * 0.5) + 32767/65535`. It needs decoding before submission; a scale alone cannot remove the bias.

The native AC7 path decodes sparse velocity and resolves a dense motion field using device depth,
`ClipToPrevClip` and available cloud depth. Written-vector camera/jitter conventions and missing
independent object coverage are documented in [motion research](ac7-motion-depth-20261004.md).
Earlier Streamline-only camera reconstruction below belongs to the diagnostic/compatibility path.
Camera reprojection cannot supply missing independent object motion.

## Streamline DLSS

The adapter loads `sl.interposer.dll` from the configured directory, uses manual hooking, accepts the existing D3D11 device, and queries support and render sizes. It supplies resource tags and per-frame constants before evaluation. NGX initialization uses the caller-supplied engine/project identity.

The recorded 7 September mission run evaluated 7,917 frames without refusal, rendering at 1024×576 and producing 2048×1152. Flight footage was reported free of obvious smearing. This exercises the path but does not establish motion conventions for every camera, object, or effect. See [capture evidence](ac7-frame-capture.md).

The accepted AC7 integration evaluates at the native reconstruction gate and reinserts before
post-processing. The optimized bridge and FSR4 INT8 SR correction later received user acceptance,
recorded in [interop research](sr-interop-performance-20261002.md).
[Shared FG research](shared-fg-20261004.md) records independent generation,
provider changes and final Unity acceptance. Those results do not establish AC7 FSR/XeSS FG,
FSR4 FG on the tested NVIDIA device, higher MFG counts, or measured latency reduction.
[Current status](../current-status.md) summarizes the limits.

## Wine development notes

The recorded support test used an RTX 4070 Laptop, driver 610.57, and the game's Proton prefix. It reported DLSS support and a 1024×576 Performance input for 2048×1152 output; the reported minimum driver was 512.15.

That setup needed DXVK, DXVK-NVAPI, vkd3d-proton, and the driver's NGX path. Even this D3D11 integration initialized a Streamline DX11-on-12 compute path. Missing D3D12 overrides caused a misleading unsupported result.

For a plain Wine test, adapt the executable, prefix, GPU selection, and SDK path to the machine:

```bash
WINEPREFIX=/path/to/prefix \
WINEDLLOVERRIDES="d3d11,d3d12,d3d12core,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n" \
RSF_STREAMLINE_BIN='Z:\path\to\streamline\bin\x64' \
wine build/linux-cross-x64/bin/rsf_dlss_backend.exe
```

The recorded NGX cubin `Ex`/`ExV2` warnings were followed by a working fallback. Two `waitForIdle: Operation failed` messages during synthetic teardown remain unexplained. Do not treat them as a clean shutdown pass.
