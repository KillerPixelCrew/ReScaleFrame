# Game SDK

A small MIT-licensed C ABI between the orchestrator and game plugins. The current interface provides plugin metadata and executable recognition; renderer preparation and frame delivery are planned.

Use fixed-width fields, `struct_size`, and ABI versions. Keep STL/Rust types and allocator ownership inside their DLLs. The header must compile as C and C++.

Recognition is separate from rendering support: AC7 currently reports `rendering_ready = 0` because the research proxy has not been integrated into this plugin lifecycle.
