# Grand Theft Auto V Enhanced integration

This is an opt-in entry and game-plugin scaffold. It recognizes the fingerprinted Steam
1.0.1158.16 executable, but does not provide SR, FG, Reflex, rendering hooks, UI, or output
reinsertion. The entry/probe has Story gameplay evidence with Steam/RTSS; native camera,
DLSS resources and candidate scene/UI tags are recorded in the linked research. Pixel boundaries
and RSF rendering are still unverified.

The build produces `ReScaleFrame.GTA5Enhanced.asi` and
`ReScaleFrame.Game.GTA5Enhanced.dll`. A compatible external ASI loader must load the former
from the game directory. The ASI expects the shared runtime and plugin in its `ReScaleFrame`
subdirectory, and is disabled unless `ReScaleFrame.ini` contains:

```ini
[GTA5Enhanced]
Enable=1
Census=0
```

Even when enabled, rendering preparation remains refused. The entry checks
the process name, hashes that executable, and asks the shared runtime to prepare the plugin.
It does not load the RDR2 plugin or use RDR2 addresses. Its log is `ReScaleFrame/gta5-enhanced.log`.
No ASI loader or Script Hook binary is bundled.

For an explicit diagnostic run, set `Census=1`. Only the known executable can install this probe.
It forwards native vendor calls and records bounded samples in
`ReScaleFrame/gta5-enhanced-census/<pid>-<tick>/`, including Streamline camera matrices, camera axes, jitter,
frame/viewport identity, resource tags, feature options, latency and DX12 presentation calls.
It does not execute RSF backends or modify game-owned settings. Initialization is on the ASI
worker after hashing; earlier calls can be missed, so startup coverage must be checked in the log.
Diagnostics default off and the first-run installation temporarily enables them. The shared
probe is capped at64 MiB and samples after32 full records per call site.

The installed build ships DX12 vendor libraries and native DLSS and FSR. DX12 is the first
research target because the RDR2 integration and shared runtime provide useful starting points.
Vulkan remains unverified, including whether this game exposes such a path. GTA V Enhanced has
its own FG and color/HUD behavior, so a shared engine family does not establish matching hooks
or inputs. RTSS compatibility is required.

Script Hook V's developer says it does not work in GTA Online. Compatibility with the downloaded
Enhanced build is unverified: the developer currently lists1158.13 and the installed build is
1158.16. The ASI probe does not require Script Hook's scripting functions. The pre-existing
ASI loader and ScriptHookV.dll are preserved. Use Story Mode with BattlEye disabled through the
supported launcher setting; the scaffold does not change anti-cheat or launcher settings.

See [engine evidence](engine.json) and [renderer research](../../docs/research/gta5-enhanced-renderer.md).
