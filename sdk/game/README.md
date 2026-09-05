# Game SDK

This directory is licensed under MIT. The surrounding runtime uses GPL-3.0-only.

The SDK is built and versioned in this repository. Its C ABI keeps DLL boundaries explicit while the loader, orchestrator, and game plugins evolve together.

The initial contract exposes metadata and read-only executable recognition. Callers set `struct_size` before requesting the exact ABI version. Returned strings belong to the loaded plugin. A match must not be treated as permission to activate rendering: `rendering_ready` remains false until the game integration exists.

Frame/resource descriptors, hook lifecycle, and output callbacks will be added with the first runtime experiments. See the architecture research for the intended data contract. A separately published SDK or package is not needed for initial development.
