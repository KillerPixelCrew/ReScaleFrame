# Game SDK

A small MIT-licensed C ABI between the orchestrator and game plugins. The current interface provides plugin metadata and executable recognition; renderer preparation and frame delivery are planned.

Use fixed-width fields, `struct_size`, and ABI versions. Keep STL/Rust types and allocator ownership inside their DLLs. The header must compile as C and C++.

Recognition is separate from rendering support: AC7 currently reports `rendering_ready = 0` because the research proxy has not been integrated into this plugin lifecycle.

ABI 2 is specified in the [representation plan](../../docs/representation-plan.md): a `game_frame.h` beside this header with the frame record and the draw description, a hooks table the plugin fills (prepare, start, quiesce, stop, classify a draw, screen policy, apply render scale, fill camera), and host services the runtime provides. Textures never cross the SDK; the plugin says which draw is interface and what the camera is. `rendering_ready` flips only with the game run that proves the path.
