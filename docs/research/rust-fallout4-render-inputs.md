# Fallout 4 render-input admission in Rust

This note covers moving the decisions of `games/fallout4/src/render_inputs.cpp` into the
`rsf-fallout4` crate (`src/render_inputs.rs`). It is a source port from base `31a06a94`. Nothing
was built, compiled, linked or tested for it, and no game, device or window run was made. The
native evidence for every address, guard, layout and admission rule is unchanged and stays in
[fallout4-native-inputs-20261116.md](fallout4-native-inputs-20261116.md) and
[fallout4-render-integration-20261116.md](fallout4-render-integration-20261116.md). The camera
arithmetic it calls is described in [rust-fallout4-camera.md](rust-fallout4-camera.md), and the
producer snapshot it checks in [rust-fallout4-depth-domains.md](rust-fallout4-depth-domains.md).

## What moved

| Former C++ | Rust | Owns |
| --- | --- | --- |
| `verify_render_inputs` | `verify` | the executable check and all 21 expected-byte guards, the process `verified` flag |
| `owner`, `state` | same names | renderer owner, device, context, window, chain, native frame and size; timing, world camera, TAA cache and flags, viewport metadata, ratios, colour/motion/depth slot maps, with the same refusal stage per field |
| `detail::same_native_owner`, `same_native_state` | same names | the exact-field comparisons, float bits, timing excluded |
| `camera`, cache scan | `read_camera`, `read_inputs` | the jitter-1 world camera entry, duplicate and missing refusals, scan counters for the receipt |
| `view_texture`, `extent`, `auxiliary`, `active_full_viewport` | same names | SRV/RTV resource, device, format, dimension, mip and extent admission; the optional allocations 14, 30 and 15 with their reread; the one-viewport rule |
| `thread_local History` | `HISTORY` | steady history, the eight reset terms, the previous raster and jitter |
| `read_native_inputs`, `read_native_raster_inputs`, `read_native_framebuffer` | `read_inputs`, `read_raster`, `read_framebuffer` | the same sequencing, producer agreement, double reads and confirmations, and every pass field |
| receipt bitmasks and budgets | `REPORTED`, `FRAMEBUFFER_REPORTED`, `SEEN_CHANGES`/`CHANGE_BUDGET`, `SEEN_REASONS`/`REASON_BUDGET` | once-per-stage receipts, sixteen mismatch texts, 32 history texts |

`render_inputs.h` is unchanged: `NativeInputs`, `NativeFramebuffer` and the four C++ functions keep
their signatures, defaults and comments. `render_inputs.cpp` keeps the four definitions as thin
wrappers over `rsf_fallout4_inputs_*` exports. `render_hooks.cpp`, `resolution.cpp`,
`depth_domains.cpp` and the C++ callers see the same results, leases and receipts.

The full reader still calls the camera owner (`camera::camera_frame`, `camera::motion_to_uv`)
and the depth owner (`depth_domains::snapshot_current`, `raster_current`) exactly where the C++
called their wrappers, now directly. The screen owner is not involved: the full reader leaves the
screen to its caller and the raster reader writes `RSF_SCREEN_UNKNOWN`, as before.

## The leaves

`render_inputs.cpp` now holds only what needs C++ or COM, passed per call as `NativeInputLeaves`
(`render_inputs_platform.h`), so the crate imports no native symbol and the camera fixture still
links it alone:

- `NativeInputLease`: the context, chain and six texture slots as `ComPtr`s. The `owner` leaf
  takes the context and chain references where the C++ assigned `result.context` and
  `result.chain`; the `resource` leaf stores the `ID3D11Texture2D` query result where the C++
  stored it. The wrapper moves the lease into its output only on admission, so a refused read
  releases everything, and Rust calls `release` for a refused optional allocation as the C++
  `output.Reset()` did. The pass pointers Rust writes are the leased pointers.
- `identity`: the canonical `IUnknown` of a live object, compared as in the depth owner.
- `owner`, `viewport`, `swap`, `resource`: context type and devices, the two `RSGetViewports`
  calls (the second only when the first count is one), `IDXGISwapChain::GetDesc`, and one view's
  description, resource, texture allocation and device.
- `reader`, `mismatch`, `history`, `framebuffer`: the former `snprintf` texts unchanged, under
  the former `try`. Rust chooses the stage, applies the masks and budgets, and fills the receipt
  records at the step the C++ filled the printed local.

The exception receipt stays in the C++ wrappers; `rsf_fallout4_inputs_claim_exception` spends
the shared reader bit, as the C++ `fetch_or` did.

## Deliberate differences

- Call order inside one stage. The `owner` leaf takes the context type, its device and the
  chain's device together, and the `resource` leaf describes the texture and queries its device
  when it obtains it. A refusal can therefore follow a few `Get*` calls the C++ would have
  skipped. They have no side effect beyond reference counts, and the refusal stage and receipt
  text are unchanged. The framebuffer receipt fills its allocation and view fields at the stage
  the C++ queried them, not when the leaf returned them.
- The colour render-target check and the auxiliary leases query `ID3D11Texture2D` on the view's
  resource. Identity comparison is on the canonical `IUnknown`, as `same_object` was.
- A failed partial `ReadProcessMemory` no longer leaves partial bytes in a field. Only receipt text
  for an already refused stage could have shown them.
- The native padding fixture initially used `native_input_state.h`. The later input cutover
  moved that fixture into Rust, where it poisons the six trailing bytes in place and borrows
  both records directly into the production owner and state predicates.
  `render_inputs_platform.h` keeps the layout only for the native diagnostic receipt.

## Preserved details

Stage numbers and names (33 reader, 19 framebuffer), the order of every check, which fields a
receipt shows at each stage, `double(x)+w <= width` bounds in f64 and the full-viewport equalities
in f32, the 0.001 `close_matrix` tolerance, bytewise `memcmp` comparisons as bit comparisons, the
unsigned frame wrap in the history reset term, the producer raster copy (`actual_raster[0..4]`,
`render_rect`), all pass keys and flags, and the raster pass key `image + 0x21fa283`.

`camera.rs` gained `camera_frame` and `motion_to_uv` helpers with the export's clearing behaviour,
and `NativeCamera` fields became `pub(crate)`. `depth_domains.rs` changed only in visibility (the
snapshot fields the readers compare, `DepthCamera`, `complete`, `raster_current`,
`snapshot_current`).

## Checks and open gaps

Checks performed: line-by-line comparison of the C++ at `31a06a94` against the Rust and the leaves;
a scripted comparison showing all 21 guard sites in the same order with the same bytes and every
hex offset unchanged; layout assertions on both sides for every shared record; `cargo fmt` with
the installed Rust 1.100.0 rustfmt (the pinned 1.94.1 toolchain is not installed here);
`cargo metadata --locked --offline`; and `git diff --check`. New Rust unit tests cover stage
numbering, the state comparisons from the C++ fixture, bytewise camera and raster comparison and
the matrix tolerance.

Open: build, link, Clippy, the plugin contract, the camera and input-state fixtures and the new
unit tests are not run, and no game run exercised the leaves or the history. A native run with
reader receipts enabled would confirm the D3D11 facts, the receipt text and the leased pointers.
