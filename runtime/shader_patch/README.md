# Runtime shader patch service

The generic C API is `include/rescaleframe/shader_patch.h`. The Runtime exports
`rsf_get_shader_patch_api`; callers borrow returned metadata/bytes until the stated
session lifetime and release returned D3D11 COM objects themselves. Devices,
contexts and compiler entry points are borrowed. Calls use the graphics owner
thread; prepare serializes one session. Root tags and intended bindings are pack
data, never game switches in core.

`tools/rsf_shader_pack.py build <source-dir> <output.rsfpatch>` builds the raw v1
container. `inspect <pack>` prints metadata and content hash. Metadata may reference
shared authored payload sources relative to the source directory, avoiding copied
header trees. No source corpus is needed to build the Anomaly release pack.

Synthetic checks (includes real WARP readback):

```powershell
cmake --build --preset windows-release-vs18 --target rsf_shader_patch_device_test
ctest --preset windows-release-vs18 -R shader_patch_synthetic_device
```

Corpus parity is opt-in and reads only an ignored local corpus. Choose a fresh
oracle destination, because the development tool refuses to overwrite evidence:

```powershell
python tools/anomaly_motion_deploy.py --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/motion-deploy-parity-new
python tools/rsf_shader_pack.py build games/anomaly/patches/motion .local/anomaly/motion.rsfpatch
cmake --build --preset windows-release-vs18 --target rsf_shader_patch_test
build/windows-x64/bin/Release/rsf_shader_patch_test.exe .local/anomaly/motion.rsfpatch .local/anomaly/vanilla/shaders/r3 .local/anomaly/motion-deploy-parity-new compile
```

The final `compile` argument also tests all 73 roots through System32
D3DCompiler 43 and the core source service, with synthetic SM_5/model SKIN_1
inputs. Without it, only byte parity and refusal tests run. It is not a game run.

DXBC rewriting currently supports one whole, same-length instruction replacement
with exact input hash, unique token pattern and checksum repair. It is exercised
only by synthetic tests and is not advertised as a supported bytecode game client.
New interfaces/resources and arbitrary reassembly remain unsupported.

Anomaly's adapter registers `ReScaleFrame/patches/motion.rsfpatch` and supplies the
engine include handler and draw eligibility. Generic sessions reject roots already
claimed by a different pack; the same pack id may overlap during transactional
reload. No pack dependency/order engine exists. The first client has one pack
on/off/status row, reload action and shader-dump action. No per-rule UI or signing.

See [research](../../docs/research/runtime-shader-patching.md) for exact source
acquisition evidence, parity and the outstanding game acceptance checks.
