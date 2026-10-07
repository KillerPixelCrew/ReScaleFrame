# Game SDK

The MIT-licensed native C ABI joins the orchestrator and game plugins. Current game ABI is **13**,
defined by `RSF_GAME_ABI_VERSION`. It includes executable recognition and `prepare`, `start`,
`quiesce`, `stop` and `status` hooks. Frame/camera records have their own ABI version **1**.

Use fixed-width fields, `struct_size`, and ABI versions. Keep STL/Rust types and allocator ownership inside their DLLs. The header must compile as C and C++.

| Header | Contract |
| --- | --- |
| [`game_api.h`](include/rescaleframe/game_api.h) | Plugin entry point, executable probe, metadata, results and lifecycle function table |
| [`game_renderer.h`](include/rescaleframe/game_renderer.h) | Host services, producer settings, leased native pass resources, CPU events and status |
| [`game_frame.h`](include/rescaleframe/game_frame.h) | Frame identity, camera metadata, screen/phase/latency enums and eligibility helpers |

Call structures start with `struct_size` and use the version required by their contract. Embedded
metadata belongs to its enclosing ABI. Plugin strings are immutable and valid until DLL unload;
probe strings are borrowed for the call.

Resolve `rsf_get_game_plugin_api`, initialize the output table's `struct_size`, and request the
declared ABI. The host probes executable identity, calls `prepare` with host services, then calls
`start` to enable producers. `quiesce` prevents new producer work; `stop` can return `BUSY` while
queued commands still reference callbacks/resources. Keep the plugin DLL, host functions and
`user` storage alive until stop succeeds. Append fields and bump the game ABI when extending it.

The host settings callback runs on the engine producer thread. CPU events describe native CPU
boundaries and prohibit graphics-context work. Render callbacks run before/after native pass
commands on the graphics execution stream; the host restores context state before returning.
Pass structures are borrowed during the call. Native texture pointers have the explicit leases
documented by each role; copying a pointer does not extend its lease. Retaining a COM resource
also does not establish its frame identity or engine pool role. Frame identity alone does not prove
Present ownership. Frame eligibility helpers check screen/flags only; they do not prove temporal
input, simulation or Present ownership.

AC7 and Unity implement lifecycle/rendering paths; Project Wingman refuses preparation/start.
All three currently return `rendering_ready = 0`, even where individual deployed paths have user
acceptance. Recognition is separate from capability and validation.

See [current status](../../docs/current-status.md) and [contract fixtures](../../tests/plugin_contract.cpp).
The ABI 2 sketches in the [representation plan](../../docs/representation-plan.md) are historical
design, superseded by these headers.
