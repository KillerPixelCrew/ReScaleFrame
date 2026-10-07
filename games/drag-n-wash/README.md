# Drag'n Wash

This is the third research case and the first Unity game. It is intended to validate the shared
[Unity Mono plugin](../unity-mono/README.md), rather than receive a duplicated game renderer DLL.

The examined Steam build is `25286774`, app `4739660`: Windows x64, Unity `6000.3.14f1`, Mono,
URP Forward+ and RenderGraph. The observed default renderer is native D3D12.

The shared Unity Mono adapter loads through an added `version.dll` shim and the `ReScaleFrame/`
folder. It provides DLSS, FSR1/2/3/4 and XeSS with the shared Insert overlay and quality controls.
Temporal SR runs before DoF/blur, bloom and tonemapping; HUD remains afterward. The user confirms
that correcting this order substantially improved static skyline stability. Broader image,
scene-transition and MSI Claw hardware acceptance are still pending. These are source/local test
builds; no Unity release is listed on GitHub as of the documentation audit on 7 October 2026.

Extract the test ZIP beside `DragNWash.exe`, preserving every original game file. Start normally
through Steam on DX12 and press Insert. The Claw package defaults to Auto Quality: NVIDIA
selects DLSS, Intel selects XeSS, and AMD selects FSR3 using the game's actual adapter. Unsupported
providers report refusal and retain spatial fallback. DLSS needs supported NVIDIA hardware;
the experimental FSR4 INT8 compatibility route is device-tested separately from the Claw.
Current source also implements independent DLSS-G, FSR3/4 and XeSS FG selection and live switches.
The user accepted XeSS/DLSS-G and the final FSR correction in the recorded 4 October session.
Reflex follows DLSS-G (effective mode at least On while active); XeSS uses XeLL. The pinned FSR
path is 2x. Higher DLSS/Intel MFG counts, FSR4 FG hardware, general scene/resize coverage and
Claw operation are not established by that acceptance.

Select FG separately in Insert. Unity stores the provider choice in `ReScaleFrame/preferences.ini`.
Deployment defaults use `[UnitySR] FrameGeneration=0/1/3/4/5` (Off/DLSS/FSR3/FSR4/XeSS) and
`GeneratedFrames=1` for 2x. Off/FSR1 SR can still supply normalized FG inputs. Install matched
runtime/helper/overlay and vendor files; old SR-only ZIPs do not gain FG by changing their INI.
See [shared FG evidence](../../docs/research/shared-fg-20261004.md) and
[current status](../../docs/current-status.md).

For builds and package generation, see [the plugin guide](../unity-mono/README.md).
[Runtime evidence and corrections](../../docs/research/unity-dx12-runtime-20261004.md) separate
source review, synthetic/device fixtures, live execution and user visual observations.

[Engine evidence](engine.json) records identity and capability limits.
[Renderer research](../../docs/research/drag-n-wash-renderer.md) records the file/source findings,
temporal/UI boundaries and parser failures from the initial static investigation. Its earlier
unimplemented/pending statements describe that investigation, before the subsequent runtime work.
