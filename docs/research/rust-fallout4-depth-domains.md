# Fallout 4 depth-domain admission in Rust

This note covers moving the decisions of `games/fallout4/src/depth_domains.cpp` into the
`rsf-fallout4` crate (`src/depth_domains.rs`). It is a source port from base `1eda132`. Nothing
was built, compiled, linked or tested for it, and no game, device or window run was made. The
native evidence for every address, guard, layout and admission rule is unchanged and stays in
[fallout4-depth-domains-20261116.md](fallout4-depth-domains-20261116.md) and the
[depth handoff note](fallout4-depth-hook-integration-20261116.md); the camera arithmetic it
reuses is described in [rust-fallout4-camera.md](rust-fallout4-camera.md).

## What moved

| Former C++ | Rust | Owns |
| --- | --- | --- |
| `verify_native_depth_domains` | `verify` | all 35 expected-byte guards, the process `verified` flag |
| `sample` | `sample` | TLS block and context, renderer/device/window/chain owners, dimensions, TAA cache, ratios and viewport flags, mode-0 range intervals, camera and graph owners, depth slot/texture/SRV, near inverse, the three unique cache entries |
| `raster_owner` | `raster_owner` | the non-temporal owner subset |
| `producer_raster` | `producer_raster` | immediate context on the owning device, one viewport and its texel-centre rect, DSV/depth texture, shadow selectors, descriptor, dirty bit, physical slot map, slot0 colour RTV/texture, shadow raster and ports, default/dynamic viewport rule, containing scissor |
| `math`, `valid_camera` | `math`, `valid_camera` | camera validity, world cache agreement, near basis, near inverse, recovered projection with the largest-error receipt, composed near view-Z, world map, finite maps, both forward-W bounds |
| `same`, `same_raster`, `same_raster_owner`, `identity` | same names, `same_identity` | byte comparisons (`memcmp` on floats compares bits) |
| `capture_native_raster`, `native_raster_snapshot_current`, `native_depth_snapshot_current`, `capture_native_depth_domains`, `make_native_depth_domains` | `capture_raster`, `raster_current`, `snapshot_current`, `capture`, `make` | the same sequencing, stage choice, double samples and confirmations |
| `refuse` bitmask, lease budget | `refuse`, `LEASE_BUDGET` | once-per-stage receipts, sixteen lease receipts |

`depth_domains.h` keeps `NativeDepthSnapshot`, `NativeDepthCamera`, the six function names,
their comments and default `report` arguments. The functions are now inline wrappers over six
`rsf_fallout4_depth_*` exports, and both sides assert the snapshot layout (1304 bytes, key
offsets). `render_hooks.cpp`, `render_inputs.cpp`, `resolution.cpp`, `cpu_events.cpp` and the
camera math are unchanged; the original render, refusal fallback and scope handling in
`render_hooks.cpp` see the same results and receipts.

## The leaves

`depth_domains.cpp` now holds only what needs C++ or COM, passed per call as
`NativeDepthLeaves` (`depth_domains_platform.h`), so the crate imports no native symbol and the
camera fixture still links the crate alone:

- `tls_slots`: `__readgsqword(0x58)`.
- `identity`: the canonical `IUnknown` of a live object. The former `same_object(a, b)` required
  both pointers and both queries and compared the identities; Rust compares the identity the
  facts leaf took for the bound object with `identity(native pointer)`, nonzero on both sides.
  The objects stay alive between the two queries because the context binding or the engine owns
  them, as it did for the former comparison.
- `context`, `targets`, `scissor`, `texture`, `view`: plain D3D11 facts (type, device, viewports,
  OM views and their resources, descriptions, rasterizer scissor). Each leaf only reports. The
  second `RSGetViewports` and `RSGetScissorRects` calls are made only when the first count is one,
  as before, so a one-element buffer is never overrun.
- `refusal`, `lease`: the former `snprintf` texts unchanged, under the former `try`. Rust chooses
  the stage, whether the projection or W text applies, and the once-per-stage and budget rules.

Engine memory is read in Rust with the existing guarded `crate::read` (ReadProcessMemory), which
was the C++ `read` as well. Address 0 is refused before the read, and key arithmetic wraps as
the C++ unsigned arithmetic did.

## Deliberate differences

- Call order inside one stage. The facts leaves query D3D11 in batches: the context leaf takes the
  device and viewports even for a non-immediate context, and the targets leaf describes the bound
  colour RTV before the selector and slot reads the C++ made first. A refusal can therefore follow
  a few `Get*` calls the C++ would have skipped. They have no side effect beyond reference counts
  released before the leaf returns, and the refusal stage is unchanged.
- `make` reads the render pass as a copy after clearing `output`. `render_hooks.cpp` passes
  `pass.depth_domains` as the output of the same pass; the C++ cleared it through the reference
  first, and none of the checked fields overlap it, so the values compared are the same.
- A failed partial `ReadProcessMemory` no longer leaves partial bytes in a snapshot field. Only
  receipt text for an already refused stage could have shown them.

## Preserved details

Stage numbers and names, the order of every check and which snapshot each refusal reports;
fields written before a failing step remain in the reported copy; `std::max({1, |a|, |b|})` as
`f32::max`; the recovered projection error loop with `>=` replacement; `ceil(x - 0.5)` rect edges
in f64; the `float(width) * port` viewport equalities in f32; f64 forward-W bounds through
`camera::depth_domain_forward`; DirectXMath multiply, inverse and translation through
`camera::xm`; `XMMatrixTranspose` as a pure shuffle; D24 and Texture2D enumerators, asserted on the
C++ side against the SDK headers; depth not inverted, domain endpoint flags and clear value 1.

`camera.rs` and `camera/xm.rs` changed only in visibility (the helpers and the two plain structs
are `pub(crate)`). `CameraDepthBound` lost its `dead_code` allowance because Rust now reads it.
`cpu_events.rs` made its current-identity and render-source exports `pub(crate)` so the depth owner
calls the same functions the C++ called through `cpu_events.h`.

## Checks and open gaps

Checks performed: line-by-line comparison of the C++ at `1eda132` against the Rust and the leaves,
a scripted comparison showing all 35 guard sites, their bytes and every hex offset unchanged,
layout assertions on both sides, `cargo fmt` with the installed Rust 1.100.0 rustfmt (the pinned
1.94.1 toolchain is not installed here), `cargo metadata --locked --offline` and
`git diff --check`. New Rust unit tests cover stage numbering, an admitted fixture camera, four
named math refusals and the bytewise comparisons.

Open: build, link, Clippy, the plugin contract, the camera fixture and the new unit tests are not
run, and no game run exercised the leaves. Bit parity of the camera math rests on the earlier
camera port review. A native run with refusal receipts enabled would confirm the D3D11 facts and
the receipt text.
