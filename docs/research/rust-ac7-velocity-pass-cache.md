# AC7 velocity-pass sources and variant admission in Rust

Base: `43754c8` on `NightHammer1000/rust-ac7-velocity-pass-cache`. This is a source comparison
of [velocity_pass.cpp](../../games/ac7/src/velocity_pass.cpp) before and after the port. Nothing
was built, linked or run for it.

## Question

Which part of the translucent velocity pass can move into `rsf-ac7` first while the pass keeps its
typed D3D11 calls, HLSL sources, public C entry points and carrier wiring unchanged?

## Entry points and lifetime

[The bridge](../../loader/proxy/src/dlss_bridge.c) is the only production caller. Both AC7
carriers link it.

| Public call | Caller |
| --- | --- |
| `rsf_ac7_motion_capture_shader` | `on_shader_created`, every successful D3D11 shader creation; it feeds the F9 observer and the velocity cache |
| `rsf_ac7_velocity_layout_seen` | `on_layout_created`, every successful input-layout creation, after the F9 observer |
| `rsf_ac7_velocity_scope` | `on_native_scope`, every native render scope begin and end, after the F9 scope observer |
| `rsf_ac7_velocity_geometry` | `on_geometry` for every frame-tap draw; only immediate-context draws proceed |
| `rsf_ac7_velocity_commit` | `on_native_pass` at `RSF_GAME_RENDER_VELOCITY_FINALIZE` begin while started |
| `rsf_ac7_velocity_frame_end` | `on_native_pass` at `RSF_GAME_RENDER_FRAME` end |
| `rsf_ac7_velocity_release` | renderer stop, after the frame tap is uninstalled |
| `rsf_ac7_velocity_set_log` | `rsf_bridge_set_log` |

Creation callbacks can arrive on any creating thread, which is why the caches are locked. The
current scope is thread-local, so admission reads the material blend on the draw's thread.
The pass owns one `Renderer` per D3D11 device. A draw on a different device builds a new one and
destroys the old renderer with its variants, bytecode-keyed shader objects, models and views.
Release destroys the renderer and clears the observed sources and layouts. The source cache keeps
D3D11 interface addresses, which D3D11 reuses, so each observation first forgets the old entry.
The renderer's publication and teardown moved to Rust later; see
[Renderer lifetime](#renderer-lifetime) below.

## Moved

[velocity_pass_cache.rs](../../games/ac7/src/velocity_pass_cache.rs) now owns:

- Shader observations: null shader or bytes, stage above 4, zero size or more than 1 MiB is
  ignored without forgetting. Otherwise the address is forgotten first, a source that would exceed
  64 MiB in total is refused, and the copy keeps its stage and the CRC-32C of the declared DXBC
  container. The hash is the contact-shadow helper's, now `contact_shadow::container_crc`, so it
  still ignores engine trailers and is zero below 28 bytes or past the declared size. There is no
  entry count limit.
- Layout observations: a null layout is ignored. Otherwise the address is forgotten first. Null
  elements, `copied != count`, more than 16 descriptors, or 4096 retained layouts refuse. An
  accepted layout, including an empty one, takes the next creation serial. The serial is never
  reset, as before.
- Variant admission: the vertex and pixel sources must be observed at stages 0 and 1; a bound GS at
  stage 2; when either tessellation stage is bound, both at 3 and 4. Otherwise no variant and
  nothing is cached. The key is the five shader addresses, the layout address and serial, the five
  bytecode hashes (zero when absent) and the material blend. A known key returns its slot or its
  cached refusal. The vertex hash goes back for the model key.
- Beam/trail layout recognition: 80-byte first stream, five or six descriptors, semantics 0 to 4
  once each in slot 0 at offsets 0, 16, 32, 64 and 48, semantic 5 in slot 1 at 0, all per-vertex,
  float4 except float3 semantic 1.
- Generated-geometry history identity: `hull ^ (domain << 1) ^ (domain >> 31)` for tessellation,
  the GS hash for a native GS, zero otherwise.

`velocity_pass.cpp` lends `make_variant` per new key. It receives copies of the admitted bytecode,
the layout descriptors, the beam decision and the history identity, and runs the unchanged
transforms, input-layout widening, bytecode-keyed D3D11 object caches, stream-output declaration
and refusal logging. Its outcomes map to the old ones: a transform, object or stream-output failure
is a cached refusal; a missing position output stays uncached and is retried; success appends the
variant to the renderer's list and records its slot. The renderer list replaces the old map, so
refusals no longer occupy a native entry. Replacing the renderer resets the Rust variant table,
and release clears all of it.

The CMake target links `rsf_ac7_rust` after the render scope. The public header, its exported
names and the bridge are unchanged.

## Deliberate differences

- A hit no longer copies the vertex and pixel bytecode out of the cache on every draw. Copies are
  made only for a new key, under the lock, and the builder runs after the lock is released. The
  D3D11 objects it creates report back through the same observation callbacks.
- The variant table now shares the source lock. It used to be unguarded and thread-bound.
- Allocation failure while observing a shader reserves before counting. The old map insert could
  throw after the byte total was raised.
- An exception inside the builder is caught there and leaves the key uncached. Previously it
  unwound to the draw callback's catch. Either way the draw is skipped. The CPU sample now records
  the shader time for that callback.
- A reset during a build discards that late record through a generation counter. The old code had
  no synchronization for this.
- The F9 observation cache in `motion_capture_observations.rs` stays separate. It keys by address
  and stage, hashes the full span, caps 4096 shaders and records only while capture is enabled.

## Checks

Source comparison against the base. `rustfmt` 1.100 (`cargo fmt --check` over `rsf-ac7`), locked
offline Cargo metadata, and Git whitespace. The new Rust tests in
`velocity_pass_cache/tests.rs` cover replacement, budget, layout serial and capacity,
beam recognition, stage admission, reuse, refusal caching, retries, history identity and reset.
They were not run. Build, link, Cargo tests, Clippy, CTest including `ac7_velocity_shader`,
deployment and game runs are user-owned and not done.

## Remaining in C++

Model and view history moved later; see [rust-ac7-velocity-history.md](rust-ac7-velocity-history.md).
At this base the remainder was model and view history state with their epoch retirement, capture and stream buffer sizing,
indirect argument validation, record counts and packing, per-draw parameter packing (rect, jitter,
opacity cutoff, history mask), the commit identity checks and merge, CPU profiling, the device
renderer lifecycle and every D3D11 call and HLSL source.

## Renderer lifetime

Base: `17904281` on `NightHammer1000/rust-ac7-velocity-renderer-lifetime`, which includes the
commit and profile port `bb538820`. Source comparison only; nothing was built, linked or run.

### Question

After the variant, history, stream and commit ports, `velocity_pass.cpp` still held the
`std::unique_ptr<Renderer>` global and decided alone which renderer was current, when a draw's
device needed a new one, what a replacement reset and what release tore down. Can that ownership
move into `rsf-ac7` while the native renderer object, its D3D11 calls, HLSL, the variant builder
and the slot vectors stay native?

### Traced paths at the base

| Path | Behavior at `17904281` |
| --- | --- |
| Draw admission | After the stream draw and route checks, `GetDevice` on the immediate context. Without a renderer, or when `renderer->device` differs, `make_unique<Renderer>(device)` and `initialize()`. A false `initialize` logs "device-side pass initialization failed" and skips the draw, keeping the old renderer. An exception unwinds to the callback's `catch (...)` without a log. |
| Publication | `renderer=std::move(made)` destroys the old renderer with its variants, bytecode-keyed shader maps, models, views, stream, capture and indirect buffers. Then `rsf_ac7_velocity_reset_variants`, `rsf_ac7_velocity_history_reset`, `rsf_ac7_velocity_profile_reset` in that order. Only after that does the `CpuSample` start. |
| Commit | Returns 0 without a renderer or context. Later, after the target check, the context's device must be `renderer->device`. |
| Frame end | Nothing without a renderer; otherwise the CPU line reads `renderer->shader_objects`, then history retirement with the renderer as the retire context. |
| Release | `renderer.reset()`, `current={}` (the releasing thread's scope copy), then cache release, history reset and profile reset. |
| Shader count | `rsf_ac7_velocity_device_shader_creation_count` reads `shader_objects`, or 0 without a renderer. Only the `ac7_velocity_shader` fixture calls it. |

None of these were synchronized; the draw, commit and frame-end callbacks run on the render
thread and release runs after the frame tap is uninstalled.

### Moved

[velocity_renderer.rs](../../games/ac7/src/velocity_renderer.rs), through
[velocity_renderer.h](../../games/ac7/src/velocity_renderer.h), now owns:

- `admit`: the published renderer for the draw's device, or a new one.
  The create leaf runs without the lock because it compiles HLSL and creates D3D11 objects whose
  creation callbacks report into the source cache. A null result keeps the published renderer and
  returns null, so the native refusal text and the skipped draw are unchanged. On success the new
  renderer is published, the old one is destroyed through its leaf, then the variant table,
  history and profile are reset in the former order.
- `rsf_ac7_velocity_renderer_published` and `rsf_ac7_velocity_renderer_for`: the published
  renderer, and the published renderer only when it was made for a given device. The commit's
  device check uses the second.
- `rsf_ac7_velocity_renderer_release`: destroys the published renderer, then releases the source
  cache and resets history and profile.

The renderer is stored with its destroy leaf. The device is kept as an address for identity only;
the native renderer still holds its `ComPtr<ID3D11Device>`. The resets are crate calls now:
`velocity_pass_cache::reset_variants` and `release`, `velocity_history::reset` and
`velocity_commit::reset_profile` replace the `rsf_ac7_velocity_reset_variants`,
`rsf_ac7_velocity_cache_release`, `rsf_ac7_velocity_history_reset` and
`rsf_ac7_velocity_profile_reset` exports, which nothing native calls any more.

`velocity_pass.cpp` lends `create_renderer` (make, initialize, release ownership or null) and
`destroy_renderer` (delete), both `noexcept`. It keeps the `Renderer` object and every member,
`initialize`, the variant builder, the slot vectors and retirement leaf, `prepare_indirect`'s
buffer-device check, all D3D11 calls, the HLSL, the clock, the log text and the thread-local
scope. The public calls in `ac7_velocity_pass.h` and the bridge are unchanged.

### Deliberate differences

- An exception while creating or initializing a renderer now returns null from the leaf, so the
  draw logs the initialization refusal instead of being skipped silently. The old renderer is kept
  either way.
- A null device is refused before creation. `GetDevice` never returns null on a live context, and
  the old code would have dereferenced it in `initialize`.
- Publication takes a lock. If another callback published a renderer for the same device while
  this one was being created, the new one is destroyed and the published one is used. The old
  code had no synchronization; the AC7 pass only admits immediate-context draws, so this path is
  defensive.
- Release clears the thread-local scope after the resets instead of between the renderer and the
  cache. The scope is independent of all four, so the outcome is the same.
- A renderer pointer returned to the native side is still used after the lock is released, as the
  unique pointer was before. Release must not overlap draws, commits or frame ends, which the
  bridge already ensures by uninstalling the frame tap first.

### Checks

Source comparison against `17904281` of every former use of the `renderer` global and of the four
reset and release calls. `cargo fmt -p rsf-ac7 --check` with the installed rustfmt (1.11.0; the
`rust-toolchain.toml` 1.94.1 pin is not installed through rustup here), locked offline Cargo
metadata and Git whitespace passed. `velocity_renderer/tests.rs` covers null devices, reuse on the
same device, replacement order (create, destroy old, reset), a failed creation keeping the
published renderer, a publication during creation, release order and republication, the
device-only lookup and incomplete leaves. They were not run. Build, link, Cargo tests, Clippy,
CTest including `ac7_velocity_shader`, deployment and game runs are user-owned and not done.

### Remaining in C++

The `Renderer` object and its resources, `initialize`, variant construction with its
bytecode-keyed object caches, the per-slot model and view resources and their retirement leaf,
the indirect buffer's device check, the commit's native depth identity, every D3D11 call and
description, the QPC clock, the log text and the HLSL. These are the native calls and resource
leases the earlier ports kept deliberately; no further lifetime or admission policy remains in
`velocity_pass.cpp`.
