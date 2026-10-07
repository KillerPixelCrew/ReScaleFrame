# Game SDK

The MIT-licensed native C ABI joins the orchestrator and game plugins. Current game ABI is **13**,
defined in [game_api.h](include/rescaleframe/game_api.h). It includes executable recognition and
`prepare`, `start`, `quiesce`, `stop` and `status` hooks. [game_renderer.h](include/rescaleframe/game_renderer.h)
defines host services, CPU events and rendering callbacks; [game_frame.h](include/rescaleframe/game_frame.h)
defines frame/camera records with its own ABI version 1.

Use fixed-width fields, `struct_size`, and ABI versions. Keep STL/Rust types and allocator ownership inside their DLLs. The header must compile as C and C++.

Call structures start with `struct_size` and use the version required by their contract. Embedded
metadata belongs to its enclosing ABI. Plugin strings are immutable and valid until DLL unload;
probe strings are borrowed for the call. Renderer callbacks carry opaque native resource pointers
with leases through the matching end callback. Frame identity alone does not prove Present ownership.

The runtime selects and prepares the plugin before graphics activation. Quiesce stops producers;
queued work may still need to drain. Keep the DLL and host services alive when stop returns busy.
AC7 and Unity implement lifecycle/rendering paths; Project Wingman refuses preparation/start.
All three currently return `rendering_ready = 0`, even where individual deployed paths have user
acceptance. Recognition is separate from capability and validation.

See [current status](../../docs/current-status.md) and [contract fixtures](../../tests/plugin_contract.cpp).
The ABI 2 sketches in the [representation plan](../../docs/representation-plan.md) are historical
design, superseded by these headers.
