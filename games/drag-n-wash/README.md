# Drag'n Wash

This is the third research case and the first Unity game. It is intended to validate the shared
[Unity Mono plugin](../unity-mono/README.md), rather than receive a duplicated game renderer DLL.

The examined Steam build is `25286774`, app `4739660`: Windows x64, Unity `6000.3.14f1`, Mono,
URP Forward+ and RenderGraph. The observed default renderer is native D3D12.

The shared Unity Mono adapter loads through an added `version.dll` shim and the `ReScaleFrame/`
folder. It provides DLSS, FSR1/2/3/4 and XeSS with the shared Insert overlay and quality controls.
Temporal SR runs before DoF/blur, bloom and tonemapping; HUD remains afterward. The user confirms
that correcting this order substantially improved static skyline stability. Broader image,
scene-transition and MSI Claw hardware acceptance are still pending.

Extract the test ZIP beside `DragNWash.exe`, preserving every original game file. Start normally
through Steam on DX12 and press Insert. The Claw package defaults to Auto Quality: NVIDIA
selects DLSS, Intel selects XeSS, and AMD selects FSR3 using the game's actual adapter. Unsupported
providers report refusal and retain spatial fallback. DLSS needs supported NVIDIA hardware;
the experimental FSR4 INT8 compatibility route is device-tested separately from the Claw.
No Unity frame generation or Reflex support is claimed.

For builds and package generation, see [the plugin guide](../unity-mono/README.md).
[Runtime evidence and corrections](../../docs/research/unity-dx12-runtime-20261004.md) separate
source review, synthetic/device fixtures, live execution and user visual observations.

[Engine evidence](engine.json) records identity and capability limits.
[Renderer research](../../docs/research/drag-n-wash-renderer.md) records the file/source findings,
temporal/UI boundaries, parser failures and pending live experiments. Rendering, SR, FG and
latency support remain unimplemented and untested for this game.
