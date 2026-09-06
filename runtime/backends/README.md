# Backends

Super resolution and frame generation backends, and the vendor-neutral model they implement.

The orchestrator owns the graphics resources and the presentation path. A backend owns one vendor's
reconstruction: how it wants its inputs, what it can derive for itself, and what it refuses. The
line between them is the same one `AGENTS.md` draws between orchestrator and game plugin, applied a
level down.

## Why Rust here

The rest of the runtime is C and C++ because it lives inside the game process and speaks COM. These
crates do not. They hold rules, unit conversions and refusal conditions, none of which need a GPU or
Windows to test, and all of which are easier to get right with sum types and exhaustive matches.

When a backend does need to reach a vendor SDK, the boundary is a C ABI in the shape
`sdk/game/include/rescaleframe/game_api.h` already uses: `#[repr(C)]` structs leading with
`struct_size`, opaque handles, `catch_unwind` at every entry point so a panic never unwinds into
foreign code, and a thread-local last error rather than a returned string. No STL, no Rust types,
no ambiguous ownership across the line.

| Crate | Contents |
| --- | --- |
| `rsf-upscaler` | Vendor-neutral model: quality levels, motion vector conventions, viability checks |

## `rsf-upscaler`

Two questions, answered without any vendor SDK present.

**What are this game's motion vectors, really?** `motion.rs` carries the answer as data rather than
as an assumption. Ace Combat 7 writes object motion only, biased into a sixteen bit unsigned target
by `In * (0.499 * 0.5) + 32767/65535`, with zero reserved to mean "nothing wrote this pixel". The
scale constant comes from engine source, not from fitting captured data: in a frame where little
moves, the samples cannot determine it, and a fitted value came out wrong by four times while
looking entirely plausible. There is a test pinning that.

**Can this backend be driven from those inputs?** `check` says so, and returns every reason it
cannot rather than the first. A backend that reconstructs camera motion from depth (Streamline,
given `cameraMotionIncluded` and an invalid value) accepts AC7's object-only field directly. One
that wants a complete field (XeSS, FSR) needs a composition pass that does not exist yet, and
`check` says exactly that instead of producing a smeared image at runtime.

The honesty rule from `AGENTS.md` applies to this crate the same way it applies to
`rsf_game_info.rendering_ready`: a backend reports what it can do, and a pairing that is not viable
is refused with a reason rather than attempted.

Nothing here talks to a GPU yet. `cargo test -p rsf-upscaler` runs anywhere.
