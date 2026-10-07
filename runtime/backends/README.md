# Backends

| Component | Purpose | Status |
| --- | --- | --- |
| `rsf-upscaler` | Rust quality, motion, and input-validation model | Unit-tested; no GPU calls |
| `dlss` | C++ Streamline adapter behind `rescaleframe/dlss.h` | D3D11/D3D12 SR, DLSS-G and Reflex/PCL implemented; recorded live AC7/Unity paths |
| `fsr` | FFX DX12 SR provider with explicit FSR2/FSR3/FSR4 version selection | FSR2/3/4 SR device evidence including guarded FSR4 INT8; FSR3/4 FG implemented with 2x maximum, exact hardware support queried |
| `xess` | XeSS-SR DX12 provider | DX12 SR and XeSS FG/XeLL implemented; native D3D11 SR unavailable; queried MFG limits |

Without SDK headers, providers return `NOT_COMPILED`. With headers, `open` loads the runtime
from an absolute configured directory, resolves entry points, creates the vendor context and checks
support. Probe reports build capability without loading. Missing DLLs, unsupported versions and
failed evaluations remain different outcomes. Current AMD SDK 2.3.0 supplies actual FSR 2.3.4,
3.1.5 and hardware-dependent FSR4 providers; the family is never silently substituted.

The orchestrator owns shared resources and frame sequencing. Each backend owns its vendor context, capability queries, input requirements, and evaluation. Streamline stays in C++ so its versioned vendor types come from the official headers. No frame generation SDK runs on D3D11; the implemented D3D12 presentation/interop services host those paths ([vendor contracts](../../docs/research/vendor-fg-contracts.md)).

## Source map and call contracts

| Location | Responsibility |
| --- | --- |
| `../contract/include/rescaleframe/backend.h` | Common C ABI, capability records, resource state/generation/lifetime, SR and legacy FG tables |
| `../contract/include/rescaleframe/backend_registry.h`, `../contract/src/negotiate.cpp` | Pure selection of providers/routes, with caller-ordered fallback and accumulated reasons |
| `../contract/include/rescaleframe/frame_generation.h` | Live generation table, options, statistics validity, source markers, abort and input retirement |
| `common/sr_helpers.h`, `common/fg_helpers.h` | Absolute-path runtime loading, argument/ABI checks, native-device/state/rectangle validation |
| `dlss/include/rescaleframe/` | SR adapter, native DX12 context and shared Streamline host contracts |
| `dlss/src/dlss_streamline.cpp`, `dlss_native12.cpp` | SR registration, SDK render-size queries, camera constants, tags and evaluation |
| `dlss/src/dlss_generation.cpp`, `streamline_frame.h` | Shared host/proxy lifetime, source-token ring, Reflex/PCL markers, FG policy and retirement fences |
| `dlss/src/reflex_audit.*` | Optional bounded NVAPI observation; forwards original calls and preserves resident hook chains |
| `fsr/src/fsr_backend.cpp`, `fsr_sr.inl` | Compile capability, exact-family/version selection and DX12 SR dispatch |
| `fsr/src/fsr4_compat.*` | Fingerprinted, device-scoped INT8 capability lease; unrelated SDK queries retain native answers |
| `fsr/src/fsr_generation.cpp` | Exact FG family/version, SDK proxy callbacks, prepare/history, separate activity and Present counters |
| `xess/src/xess_backend.cpp`, `xess_sr.inl` | Compile capability, versioned DX12 SR and fallback responsive mask |
| `xess/src/xess_generation.cpp` | XeFG proxy/resource tags, reserved interpolation count and serialized XeLL pacing/markers |
| `rsf-upscaler/src/lib.rs`, `frame.rs`, `motion.rs` | Pure quality/input checks and unit/encoding math; model ratios do not replace live SDK sizing |
| Backend `CMakeLists.txt`, `rsf-upscaler/Cargo.toml` | Optional SDK header gates and workspace build policy; vendor DLLs are loaded at runtime |

SR follows `open -> plan -> evaluate -> close`. Evaluation records work on the supplied context/list;
the caller submits it, restores relevant graphics state, and retires textures after GPU completion.
FSR and XeSS SR currently retain vendor allocations until close. A successful build-capability probe
does not establish runtime, device, or version support.

Live FG uses `rsf_generation_provider`, separately from the older `rsf_fg_provider` capability tables.
FSR and XeSS legacy FG tables refuse work; their live generation tables own the implemented FG paths.
Its usual sequence is `create/configure -> begin_frame/markers -> prepare -> Present -> after_present`
followed by status and retirement queries. Keep frame resources until the source submission fence and
any returned vendor fence complete. Statistics are meaningful only when their `RSF_FG_STAT_*` bit is
set; SDK image-finalization callbacks and generated dispatches do not prove generated Present counts.
Aborted/skipped frames break history. Destroy sessions with marker callers stopped and GPU work drained.

The D3D12 Streamline host is the sole SDK registration for shared DLSS SR/FG and Reflex/PCL. It owns
proxy/native aliases and one bounded source-token ring; source IDs persist through input, simulation,
render submission and Present. Tokens retire after all six normal markers or an explicit abort. The
XeLL adapter instead limits source IDs to 32 bits and requests restart before that range is exceeded.

These source contracts describe implementation behavior. Current integration evidence and unresolved
work remain in [the implementation tracker](../../docs/implementation.md) and the research links below.

## Motion and frame data

AC7's velocity texture is sparse: moving objects write values, while unwritten pixels carry a clear-value sentinel. UE4 stores velocity as `v * (0.499 * 0.5) + 32767/65535`. It needs decoding before submission; a scale alone cannot remove the bias.

The native AC7 path decodes sparse velocity and resolves a dense motion field using device depth,
`ClipToPrevClip` and available cloud depth. Written-vector camera/jitter conventions and missing
independent object coverage are documented in [motion research](../../docs/research/ac7-motion-depth-20261004.md).
Earlier Streamline-only camera reconstruction below belongs to the diagnostic/compatibility path.
Camera reprojection cannot supply missing independent object motion.

The DLSS compatibility adapter can ask Streamline to resolve camera motion using depth and
`ClipToPrevClip` when `camera_motion_included` is false and an invalid-motion sentinel is supplied.
The native DX12 SR adapter instead expects an already decoded, complete pixel-motion field.

The Rust model defines motion-to-pixel and jitter conversions, plus an eight-sample base jitter sequence scaled by the output/input area ratio. The live C/C++ path does not use all those conversions. Reconciling their units, signs, and jitter treatment is an open [motion task](../../docs/implementation.md#ac7-motion-and-velocity-improvements).

```bash
cargo test -p rsf-upscaler --locked
```

## Streamline DLSS

The native D3D11 adapter loads `sl.interposer.dll` from the configured directory with manual hooking,
registers the existing device, and queries support and render sizes. The D3D12 SR adapters can borrow
the shared host described above. Both supply resource tags and per-frame constants before evaluation.
NGX initialization uses the caller-supplied engine/project identity.

The recorded 7 September mission run evaluated 7,917 frames without refusal, rendering at 1024×576 and producing 2048×1152. Flight footage was reported free of obvious smearing. This exercises the path but does not establish motion conventions for every camera, object, or effect. See [capture evidence](../../docs/research/ac7-frame-capture.md).

The accepted AC7 integration evaluates at the native reconstruction gate and reinserts before
post-processing. The optimized bridge and FSR4 INT8 SR correction later received user acceptance,
recorded in [interop research](../../docs/research/sr-interop-performance-20261002.md).
[Shared FG research](../../docs/research/shared-fg-20261004.md) records independent generation,
provider changes and final Unity acceptance. Those results do not establish AC7 FSR/XeSS FG,
FSR4 FG on the tested NVIDIA device, higher MFG counts, or measured latency reduction.
[Current status](../../docs/current-status.md) summarizes the limits;
[the September review](../../docs/review.md) retains historical findings.

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
