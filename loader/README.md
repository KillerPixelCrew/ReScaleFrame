# Loader

The standalone launcher or WSGM will arrange early loading of this bootstrap. The bootstrap will load the orchestrator; the orchestrator selects and prepares the game plugin before activating the pipeline.

The current DLL exports its version only. Injection, startup interception, the runtime handshake, and DXGI shim fallback are pending. Keep heavy initialization and waits outside `DllMain`.

`diagnostics/` holds the module capture used to read a protected executable that cannot be analysed from disk. `proxy/` builds the research carrier `dinput8.dll` that loads it into the game. Both are diagnostics: they intercept nothing and touch no graphics object. See [ghidra-tooling.md](../docs/research/ghidra-tooling.md) for the procedure and results.
