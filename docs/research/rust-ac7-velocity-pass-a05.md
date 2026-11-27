# AC7 velocity pass and history port

Chunk A05 continues the Rust ownership described in [history](rust-ac7-velocity-history.md),
[cache](rust-ac7-velocity-pass-cache.md), stream and
commit. The existing AC7 renderer evidence and build scope are in
[the velocity investigation](ac7-velocity-20261013.md). The port preserves the native producer,
pipeline bindings, motion format and output integration described there.

| PORT.md finding | Source evidence |
| --- | --- |
| Commit model record capacity after native allocation succeeds. | `velocity_history.rs` returns a capacity proposal while rotating history independently. `velocity_pass.cpp` charges the draw after native allocation and queued hash dispatch. The history fixture retries a discarded proposal before charging it. |
| Apply the same view identity to velocity accumulation and commit. | `velocity_history.rs` shares frame, view and family matching for same-frame accumulation and commit. Replays additionally match depth resource and extent. The fixture refuses alternate view and family identities that look up the same history. |
| Retain one shader bytecode allocation for AC7 capture and velocity. | `dlss_bridge.c` delivers one creation callback through the F9 compiler leaf into `motion_capture_observations.rs`. The two caches hold references to the same fallibly reserved byte vector while keeping separate retention budgets, hashes and artifact order. The unlocked builder retains the original payload. Rust's standard `Arc` still allocates its small control block separately. Fixtures inspect the retained payload after source eviction and check capture ordering. |
| Include beam-layout classification in velocity variant identity. | `velocity_pass_cache.rs` calculates the layout classification before checking the variant key. The fixture changes stride on the same layout after a cached refusal. |
| Preserve the reason for variant build failure before caching it. | `velocity_pass.cpp` returns retryable outcomes for D3D shader, layout and stream-output failures. Definitive structural refusals still use the refusal cache. The shader-transform API currently combines unsupported shaders with compiler and allocation failures, so opaque transform failures remain retryable. This can repeat transform work on unsupported draws. |
| Reject a built slot when its renderer generation has retired. | `velocity_pass_cache.rs` checks generation before returning a late build; the reset fixture expects `NO_VARIANT`. The native replay lock spans draw, commit and frame-end uses of renderer state and serializes renderer replacement and release. Frame tap suppresses nested draw observation during replay, and commit suppresses velocity observation of its own merge draw. Device creation notifications only enter the source and capture caches. |
| Invalidate the old shader identity even when its replacement is too large to capture. | `motion_capture_observations.rs` invalidates F9 shader facts before admission. Velocity invalidates its source even when the callback has no retainable bytecode. Existing separate test entry points still support the velocity-only fixture. |

`velocity_commit.rs` owns the merge and fullscreen HLSL, and `velocity_stream.rs` owns the
indirect bounds shader. The C++ leaves still compile them and own COM objects, state leases, GPU
resource preparation and submission. A04 owns the shared hash writer and linked tessellation
transform. The A05 history and cache headers are private C ABI seams with paired Rust definitions.
The shared generated game and observer records used by A02, A03 and G02 retain their existing layout.

Validation was limited to source checks and formatting with the pinned Rust 1.94.1 toolchain.
Build, lint, fixtures and device validation remain user-owned.
