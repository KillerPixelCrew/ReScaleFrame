# Unity Mono

The planned shared plugin uses reflection and Harmony to adapt Unity Mono rendering pipelines.
Drag'n Wash is the first URP RenderGraph validation case. There is no Unity Mono native DLL,
managed helper, loader, Harmony dependency or rendering implementation yet.

The plugin will own Unity discovery, engine hooks, camera/frame conventions and SR/UI reinsertion.
The shared orchestrator retains vendor SDKs, GPU interoperability and presentation. URP, HDRP
and built-in rendering require separate adapters. IL2CPP is outside the initial Mono adapter.

[Plugin design](../../docs/research/unity-mono-plugin.md) records the inspected methods, proposed
bootstrap, semantic patch guards, native execution and lifecycle requirements.
[First game evidence](../drag-n-wash/engine.json) keeps build identity separate from capability.
