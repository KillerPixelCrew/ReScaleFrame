# Loader

The standalone launcher or WSGM will arrange early loading of this bootstrap. The bootstrap will load the orchestrator; the orchestrator selects and prepares the game plugin before activating the pipeline.

The current DLL exports its version only. Injection, startup interception, the runtime handshake, and DXGI shim fallback are pending. Keep heavy initialization and waits outside `DllMain`.
