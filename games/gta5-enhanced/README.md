# Grand Theft Auto V Enhanced integration

This is an opt-in entry and game-plugin scaffold. It does not yet recognize an executable
fingerprint or provide SR, FG, Reflex, rendering hooks, UI, or output reinsertion.
The completed Steam download and runtime evidence are needed before enabling a renderer.

The build produces `ReScaleFrame.GTA5Enhanced.asi` and
`ReScaleFrame.Game.GTA5Enhanced.dll`. A compatible external ASI loader must load the former
from the game directory. The ASI expects the shared runtime and plugin in its `ReScaleFrame`
subdirectory, and is disabled unless `ReScaleFrame.ini` contains:

```ini
[GTA5Enhanced]
Enable=1
```

Even when enabled, the current empty fingerprint allowlist refuses preparation. The entry checks
the process name, hashes that executable, and asks the shared runtime to prepare the plugin.
It does not load the RDR2 plugin or use RDR2 addresses. Its log is `ReScaleFrame/gta5-enhanced.log`.
No ASI loader or Script Hook binary is bundled.

The game supports DX12 and Vulkan and has native DLSS and FSR. DX12 is the first research target
because the RDR2 integration and shared runtime provide useful starting points. Vulkan remains
separate and unverified. GTA V Enhanced has its own FG and color/HUD behavior, so a shared engine
family does not establish matching hooks or inputs. RTSS compatibility is required.

Script Hook V's developer says it does not work in GTA Online. Compatibility with the downloaded
Enhanced build is unverified. The scaffold does not change anti-cheat or launcher settings.

See [engine evidence](engine.json) and [renderer research](../../docs/research/gta5-enhanced-renderer.md).
