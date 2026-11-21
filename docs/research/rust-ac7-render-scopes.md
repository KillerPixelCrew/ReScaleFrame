# AC7 queued render scopes in Rust

Date: 2026-11-21. Base: `rust/orchestrator-anomaly` at d27ebb4 (branch
`NightHammer1000/rust-ac7-render-scopes`), after the TrueSky port
([rust-ac7-truesky-relays.md](rust-ac7-truesky-relays.md)).

## Question

Can the queued render-scope and ticket owner in `games/ac7/src/render_scope.cpp` move into the
`rsf-ac7` crate without changing the `ac7_render_scope.h` C ABI, the verified native RHI list and
command layouts, marker order, deferred output resolution, or the link closure of the game DLL,
the fixtures and the two DirectInput carriers?

## Inventory at d27ebb4

`render_scope.cpp` (198 lines) held one owner:

| Part | Content |
| --- | --- |
| `rsf_ac7_render_scopes` | `std::mutex`, session, serial, capacity (1..4096), outstanding, accepting, state and pass callbacks, user, the executed-scope stack reserved for `capacity` |
| `rsf_ac7_render_ticket` | owner, copied scope, lease, begin and end `ScopeCommand`s, `closed` |
| `valid_identity` | FRAME needs a source frame; WINDOW and TEXTURE_BINDING need source, game viewport, window and pass (WINDOW also the RHI viewport); every other role needs family and view |
| `append` | the private list layout: root `+0`, tail link `+8`, executing flag `+0x10`, NumCommands `+0x14`, UID `+0x18` untouched; refuses an executing list, a null or occupied tail link and a full count |
| `execute_scope` | the engine-called command function (`Next +0`, execute `+8`): resolve the lease, push on begin or pop the matching innermost scope on end, pass callback with the ticket scope, state callback with the selected (enclosing or zeroed) scope, then on end release the lease, decrement outstanding and delete the ticket |
| C ABI | create, create_passes, open, open_leased, close, current, quiesce, idle, destroy |

The command-list prefix and command layout are the ones recorded in
[ac7-renderer-roots-20260930.md](ac7-renderer-roots-20260930.md) (root, tail link, UID, Next and
execute thunk) and in the C++ comment for the executing flag and count. The identity rules come
from the two regressions in [ac7-dlss-fg-20261003.md](ac7-dlss-fg-20261003.md).

Callers of the header, found by searching every source and CMake file:

- `native_renderer.cpp` (game DLL; included by `tests/ac7_native_graph.cpp`): create_passes
  through the renderer leaves, open/open_leased/close in the hooks, quiesce/idle/destroy through
  the renderer transaction.
- `tests/ac7_render_scope.cpp`, `tests/d3d11_present_bridge.cpp`, `tests/ac7_native_graph.cpp`.
- No carrier source (`loader/proxy`), `velocity_pass.cpp`, `velocity_shader.cpp` or other AC7
  library calls it. The carriers reach `rsf_ac7_render_scope` only through `AC7VelocityPass`.

## Moved

`games/ac7/src/render_scope.rs` owns all of the state and sequencing above: admission and the
capacity bound, identity rules, serials, ticket allocation, the list append, the engine-called
`execute` function, the begin/end stack, both callbacks and their order, lease resolve at both
markers and release after end, quiesce, idle and destroy. It exports, under their existing
names and signatures, `rsf_ac7_render_scope_open`, `_open_leased`, `_close`, `_current` and
`rsf_ac7_render_scopes_quiesce`, `_idle`, `_destroy`.

`render_scope.cpp` keeps only `rsf_ac7_render_scopes_create` and `_create_passes`, which call
`rsf_ac7_scope_owner_create` (`src/ac7_scope_owner.h`, bindings `src/scope_abi.rs`) with a static
`rsf_ac7_scope_calls` table. Those three leaves call the consumer's callback, the lease resolve
and the lease release inside `try`/`catch (...)` and report a throw as false. They are needed
because a C++ exception cannot unwind through Rust or the engine's iterator; the Rust owner
cannot catch it. No other leaf is kept: the list append is plain memory access over the verified
layout and needs no SEH, and fallible allocation is done in Rust (`std::alloc::alloc` and
`Vec::try_reserve*`), so open and create still refuse on exhaustion instead of ending the game.

The dependency only runs C++ to Rust. Rust holds the leaf table by pointer and imports no C++
symbol, so there is no archive cycle.

## Preserved

- The public header, every exported name, signature and result, the opaque handle types and
  every caller. `native_renderer.cpp`, `plugin.cpp`, the renderer transaction and the fixtures'
  sources are unchanged.
- List and command layout and the append checks in the same order; markers are appended only by
  open (begin) and close (end), so command order is unchanged.
- Deferred output: resolve runs on the RHI stream before each marker's stack change, so an end
  sees outputs published after begin was queued; a throwing resolve still skips the stack and
  both callbacks for that marker.
- Pass callback before state callback; the pass callback gets the ticket scope, the state
  callback the selected scope (the scope on begin, the enclosing one or a zeroed scope on end).
  Callbacks run without the lock, so a callback calling destroy is refused, as the fixture
  checks.
- Serials start at 1 and are spent even when the append then refuses. Open validates before it
  writes `*out`; a lease is owned only on success; a refused close leaves the ticket open.
- Capacity bound, quiesce, idle (`!accepting && !outstanding && stack empty`, null is idle) and
  destroy (null succeeds, otherwise refused unless idle).

## Fixes, from source review

- The end marker decremented `outstanding` and then deleted the ticket. Between the two, stop
  could see the owner idle, destroy it and let the module unload while the RHI thread still ran
  `operator delete` and the rest of `execute_scope` in it. The ticket is now freed first and the
  decrement is the last action; only the unlock and return remain after idle. That window is
  narrowed, not closed: closing it needs a drain on the engine side and is not attempted.
- `execute_scope` was `noexcept` but called the lease release outside its `try`, so a throwing
  release called `std::terminate` on the engine's RHI thread. Release now goes through the
  exception leaf; the ticket still retires. No current lease release throws, so this was latent.

## Differences without intended behaviour change

- Open copies the caller's scope before taking the owner lock rather than under it; the copy is of
  caller memory on the calling thread.
- Begin grows the stack with `try_reserve(1)`. Within the reserved capacity it never allocates,
  as before; beyond it (see the stranded scopes below) a failed growth skips that begin's stack
  change and callbacks, as a C++ `bad_alloc` did.
- A null or incomplete leaf table refuses create; `render_scope.cpp` always passes a complete one.
- `std::sync::Mutex` replaces `std::mutex`; nothing panics under it and poisoning is ignored.

## Link closure

The Rust archive now defines symbols that consumers of `ac7_render_scope.h` call, so each such
target links `rsf_ac7_rust` after `rsf_ac7_render_scope`:

| Target | Before | After |
| --- | --- | --- |
| `rsf_game_ac7` | already linked `rsf_ac7_rust` after the scope library | unchanged |
| `rsf_ac7_native_graph_test` | already linked it | comment only |
| `rsf_ac7_render_scope_test` | scope library only | adds `rsf_ac7_rust` |
| `rsf_d3d11_present_bridge_test` | orchestrator core and scope library | adds `rsf_ac7_rust` |
| `rsf_proxy_dinput8`, `rsf_proxy_ac7` | scope library through `AC7VelocityPass` | unchanged |
| `rsf_ac7_velocity_shader_test` | scope library | unchanged |

`rsf_ac7_render_scope`'s own link interface is unchanged, so the Rust archive stays off the
carrier link lines.

Source-only uncertainty, none of it checked by a link:

- Static archive member selection. The carriers and the velocity shader fixture must keep
  referencing no symbol defined in `render_scope.cpp` or `native_renderer.cpp`; otherwise they
  would now need the Rust archive. Every source was searched, but no link map was produced.
- GNU order (mingw cross build). `render_scope.cpp`'s object references the Rust create symbol
  and the Rust archive references no C++ symbol, so "scope library, then Rust archive" is
  sufficient. CMake places a target's own link items after it, which keeps that order on the
  four lines above.
- Two Rust static libraries in one image. The present bridge fixture now links the orchestrator
  and AC7 Rust archives, each carrying `std`. The same pairing already exists in
  `rsf_unity_package`, `rsf_minecraft`, `rsf_godot`, `rsf_gl_shared_surface_test` and
  `rsf_ac7_native_graph_test`.

## Remaining risks

- An end that is not innermost does not pop or notify, as before. Its scope stays on the stack:
  `current` keeps reporting it and idle and destroy refuse, so the module is retained. The pass
  consumer also gets no end for it although its lease is released. No such interleaving is known
  in AC7's hooks, which open and close in nested pairs; changing it would assume engine ordering
  that has not been observed, so it is unchanged.
- A ticket whose end is never queued or executed (refused close, a list discarded without
  execution) keeps the owner busy and the module loaded, by design.
- `execute` is a Rust `extern "C"` function on the engine's RHI thread. A panic there would abort
  the game; the code uses no indexing, wrapping counters, a poison-tolerant lock and fallible
  growth so that it has no panicking path.
- Nothing here was compiled, linked or run.

## Checks

Source review against the removed C++; `rustfmt --edition 2024 --check` on the new Rust files;
`py_compile` of the binding generator; JSON parse of `engine.json`; `git diff --check`.
bindgen-cli is not installed, so `scope_abi.rs` is hand-mirrored with layout assertions and the
generator entry was not run. New Rust unit tests (`render_scope/tests.rs`, covering the C++
fixture's two scenarios, callback order, leases, refusals and argument checks) and a new section
of `tests/ac7_render_scope.cpp` that throws real C++ exceptions from a pass callback, a lease
resolve and a lease release through the leaves were written but not run. Not compiled, linked, Cargo-tested, Clippy-checked, CTest-run, deployed or game-tested.
