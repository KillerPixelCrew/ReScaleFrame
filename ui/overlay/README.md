# In-game overlay

Rust/egui behind the C ABI in [overlay.h](include/rescaleframe/overlay.h). It produces meshes, texture updates, and settings intents. The C++ D3D11 renderer in `runtime/graphics` handles GPU work.

The DLL, renderer, and input hook are loaded and connected by `loader/proxy/overlay_host`, which builds the renderer against the presenting device and draws one frame per Present; F5 opens the panel and it is game-tested in AC7 with a mouse cursor. The planned Compare, Inspect, Capture, Performance, and Status views are in [the todo](../../docs/implementation.md#egui-development-and-validation-workflow). ABI 3, appended per the rule in the header, is specified in the [representation plan](../../docs/representation-plan.md): effective vendor, route and multiplier, UI layer and bridge state, and the intents to change them.

| Layer | Files | Responsibility |
| --- | --- | --- |
| Safe core | `model.rs`, `panel.rs`, `overlay.rs` | Status, layout, input, intents, meshes |
| C boundary | `abi.rs`, `ffi.rs` | ABI layouts, argument checks, panic handling, five exports |

The current panel shows enable/quality controls, actual input/output dimensions, input availability, motion/jitter status, evaluation/refusal counts, and backend errors. Unsupported controls show a reason. It does not measure latency or quality gains.

## Renderer contract

1. Use one thread for all calls on a handle. Pass collected window input to that thread.
2. Initialize `struct_size` on input and output structures. Set the ABI version as required by the header.
3. Run a frame, then drain texture updates until none remain. Apply whole-texture uploads and partial patches before drawing; process frees after the draws that need them.
4. Draw only after `RSF_OVERLAY_OK`. Empty output has null pointers and zero counts. Follow the header's borrowed-data lifetimes.
5. After `RSF_OVERLAY_ERROR_PANICKED`, destroy the handle. Panic containment requires unwinding; an aborting Rust panic profile cannot provide it.

Vertices use premultiplied RGBA with red in the low byte (`R8G8B8A8_UNORM`). Indices are local to each draw call: add `vertex_offset`, or use `BaseVertexLocation`. Positions and clip rectangles are already physical pixels.

The first frame may only measure layout and return no draws. Supply elapsed time; zero falls back to 1/60 second. The atlas limit is 2048 pixels. Display scale follows height: 1× through 1080 lines, 2× at 2160, capped at 3×.

## Verification and gaps

```bash
cargo test -p rescaleframe-overlay --locked
cargo clippy -p rescaleframe-overlay --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Tests exercise controls, mesh bounds, texture updates, and FFI argument/layout rules. Recorded builds include Linux and the Windows GNU DLL. Live game rendering, C/C++ calls into the DLL, and MSVC ABI verification remain unproven by these tests. The panic path has no deliberate panic test.

ABI v1 has pointer position, button state, and wheel input, with no keyboard or controller events. Short clicks can be lost between frame samples; see [the review](../../docs/review.md). Real input, scrolling, focus recovery, and display scaling need in-game validation.
