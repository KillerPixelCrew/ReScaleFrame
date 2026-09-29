# Backends

| Component | Purpose | Status |
| --- | --- | --- |
| `rsf-upscaler` | Rust quality, motion, and input-validation model | Unit-tested; no GPU calls |
| `dlss` | C++ Streamline adapter behind `rescaleframe/dlss.h` | Evaluates live AC7 frames |
| `fsr` | FFX DX12 SR provider with explicit FSR2/FSR3/FSR4 version selection | FSR2/FSR3 synthetic device-tested; FSR4 support queried and refusal tested; FG unavailable |
| `xess` | XeSS-SR DX12 provider | Synthetic device-tested; native D3D11, FG and latency unavailable |

Without SDK headers, providers return `NOT_COMPILED`. With headers, `open` loads the runtime
from an absolute configured directory, resolves entry points, creates the vendor context and checks
support. Probe reports build capability without loading. Missing DLLs, unsupported versions and
failed evaluations remain different outcomes. Current AMD SDK 2.3.0 supplies actual FSR 2.3.4,
3.1.5 and hardware-dependent FSR4 providers; the family is never silently substituted.

The orchestrator owns shared resources and frame sequencing. Each backend owns its vendor context, capability queries, input requirements, and evaluation. Streamline stays in C++ so its versioned vendor types come from the official headers. No frame generation SDK runs on D3D11; the plan's presentation bridge is where every FG backend and the D3D12-only SR backends execute ([vendor contracts](../../docs/research/vendor-fg-contracts.md)).

## Motion and frame data

AC7's velocity texture is sparse: moving objects write values, while unwritten pixels carry a clear-value sentinel. UE4 stores velocity as `v * (0.499 * 0.5) + 32767/65535`. It needs decoding before submission; a scale alone cannot remove the bias.

The current DLSS path decodes this field and asks Streamline to resolve camera motion using depth and the engine's `ClipToPrevClip`. The engine already supplies the camera transform. Whether a reusable full-motion texture exists, and whether written vectors already include camera movement, still need controlled captures.

The Rust model defines motion-to-pixel and jitter conversions, plus an eight-sample base jitter sequence scaled by the output/input area ratio. The live C/C++ path does not use all those conversions. Reconciling their units, signs, and jitter treatment is an open [motion task](../../docs/implementation.md#ac7-motion-and-velocity-improvements).

```bash
cargo test -p rsf-upscaler --locked
```

## Streamline DLSS

The adapter loads `sl.interposer.dll` from the configured directory, uses manual hooking, accepts the existing D3D11 device, and queries support and render sizes. It supplies resource tags and per-frame constants before evaluation. NGX initialization uses the caller-supplied engine/project identity.

The recorded 7 September mission run evaluated 7,917 frames without refusal, rendering at 1024Ã—576 and producing 2048Ã—1152. Flight footage was reported free of obvious smearing. This exercises the path but does not establish motion conventions for every camera, object, or effect. See [capture evidence](../../docs/research/ac7-frame-capture.md).

The accepted AC7 integration evaluates at the reconstruction gate and reinserts into the game pipeline; the Present path remains diagnostic. That DLSS acceptance does not validate FSR or XeSS in AC7. See [the backend switching evidence](../../docs/research/orchestrator-sr-switching.md). [The review](../../docs/review.md) records known code issues.

## Wine development notes

The recorded support test used an RTX 4070 Laptop, driver 610.57, and the game's Proton prefix. It reported DLSS support and a 1024Ã—576 Performance input for 2048Ã—1152 output; the reported minimum driver was 512.15.

That setup needed DXVK, DXVK-NVAPI, vkd3d-proton, and the driver's NGX path. Even this D3D11 integration initialized a Streamline DX11-on-12 compute path. Missing D3D12 overrides caused a misleading unsupported result.

For a plain Wine test, adapt the executable, prefix, GPU selection, and SDK path to the machine:

```bash
WINEPREFIX=/path/to/prefix \
WINEDLLOVERRIDES="d3d11,d3d12,d3d12core,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n" \
RSF_STREAMLINE_BIN='Z:\path\to\streamline\bin\x64' \
wine build/linux-cross-x64/bin/rsf_dlss_backend.exe
```

The recorded NGX cubin `Ex`/`ExV2` warnings were followed by a working fallback. Two `waitForIdle: Operation failed` messages during synthetic teardown remain unexplained. Do not treat them as a clean shutdown pass.
