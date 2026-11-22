# AC7 velocity-pass model and view history in Rust

Base: `3856821` on `NightHammer1000/rust-ac7-velocity-history`. This is a source comparison of
[velocity_pass.cpp](../../games/ac7/src/velocity_pass.cpp) before and after the port. Nothing was
built, linked or run for it. It continues
[the source and variant admission port](rust-ac7-velocity-pass-cache.md).

## Question

Can the translucent velocity pass's history bookkeeping (model and view identity, frame rotation,
history validity, record accounting, written and commit state, and epoch retirement) move into
`rsf-ac7` while the D3D11 buffers and textures, their allocation, clears, rehash, copies, the
HLSL, shader dispatch, draw identities and the public C entry points stay as they are?

## Traced paths

The bridge entry points are unchanged and listed in the earlier note. These are the paths that
touch history state, with the behavior at `3856821`:

| Path | History behavior at the base |
| --- | --- |
| `rsf_ac7_velocity_geometry`, view | After variant admission, count bounds and indirect validation, `views[(session, history_key or view_key)]` is inserted, then the native depth must be an unarrayed, single-mip, single-sample texture at the draw's extent. Same `uint32_t(native_frame)`: valid only when the stored native depth is the draw's. Otherwise any missing target or a different extent resets the entry (`v={}`: frame `UINT32_MAX`, epoch 0, unwritten) and creates depth, DSV, motion RTV/SRV and merged RTV; a failure leaves it reset. Success records frame, family, view key, epoch and native depth, clears `written`, copies depth and clears motion. |
| `rsf_ac7_velocity_geometry`, model | After the view succeeds, `models[key]` is inserted. The key is the session, history or view key, primitive, material, vertex bytecode hash, vertex factory, start and base vertex (the last three zero for particles), and the generated-geometry hash. On a new native frame: `usable` is set when the previous frame is not the sentinel, is exactly one less (wrapping) and the scope has no `RSF_GAME_RENDER_RESET`; previous and current swap, records reset and the new current UAV is cleared. The epoch is refreshed. Then a count above `UINT32_MAX - records` or a total above `UINT32_MAX / 128` refuses; otherwise records grow and capacity doubles from 1024 to at least twice the records. A smaller current buffer is replaced by a cleared larger one into which the old current is rehashed (mode 1); a failed allocation refuses with the records already counted. |
| `rsf_ac7_velocity_geometry`, draw | `history = usable && previous buffer exists`, mask from the previous capacity, previous SRV bound at the variant's history slot. After the motion draw the view is `written`. The hash build writes into the current buffer. |
| `rsf_ac7_velocity_commit` | Finds the view by the pass's session and history or view key. It needs `written`, the same frame, view key, family key and native depth. The motion target must match the view extent, be single-sample, unarrayed and R16G16 UNORM or TYPELESS, on the renderer's device. After the merge and the depth copy `written` clears. |
| `rsf_ac7_velocity_frame_end` | Only with a renderer: the CPU log reports `models.size()` before retirement, profiling resets, the epoch advances, then models and views with `epoch + 2 < epoch` are erased. |
| Device change | A draw on another device builds a new `Renderer` (epoch 1, empty maps) and resets the Rust variant table. |
| `rsf_ac7_velocity_release` | Destroys the renderer with its history and clears the Rust caches. |

## Moved

[velocity_history.rs](../../games/ac7/src/velocity_history.rs) now owns, through
[velocity_history.h](../../games/ac7/src/velocity_history.h):

- Model identity, including the history-key fallback and the particle rule, and per model the
  frame, record count, epoch and `usable`. `rsf_ac7_velocity_model_begin` returns a slot, whether
  to rotate, the validity and the required capacity, and whether the records fit. The rotation
  and epoch refresh happen before the overflow refusal, as before.
- View identity and per view the frame, extent, family and view keys, epoch, `written`, and
  whether every target exists. `rsf_ac7_velocity_view_begin` answers same frame, reuse or
  allocate; allocate resets the entry exactly as `v={}` did. `rsf_ac7_velocity_view_ready` records
  the frame after the targets exist. `rsf_ac7_velocity_view_written` and
  `rsf_ac7_velocity_view_committed` set and clear `written`.
- The commit's logical identity: `rsf_ac7_velocity_view_commit` matches written, frame, view key
  and family key and returns the slot and extent. The commit port
  later replaced this call with `rsf_ac7_velocity_commit_view`, which builds the identity itself.
- The epoch, starting at 1, and retirement. `rsf_ac7_velocity_history_frame_end` advances it,
  removes models then views whose epoch plus 2 is below it, and reports each retired slot to the
  native release leaf after the lock is released. `rsf_ac7_velocity_model_count` feeds the log.
- `rsf_ac7_velocity_history_reset` on renderer replacement and release.

`velocity_pass.cpp` keeps the D3D11 buffers and textures in vectors indexed by those slots, the
native depth identity check at a same-frame draw and at commit, the depth-desc validation, every
allocation, the swap and UAV clear on rotation, growth with rehash, the depth copy and motion
clear, the parameter packing, both draws, the hash build, the merge and the copies. A retired slot
is reset to empty and reused only for a new entry, so a slot never carries resources across
identities.

## Deliberate differences

- The view entry is created after the native depth-desc check instead of before it. A draw whose
  depth fails that check used to leave a resourceless view at epoch 0, which was never matched by
  a commit (unwritten) and was erased by the first frame end that took the epoch above 2. Now no entry is made. Nothing
  observable depends on that entry.
- Target completeness is a flag set only after all five targets exist, instead of testing each
  pointer. A partial creation always went through `v={}` first, so the outcomes match.
- Ordered maps became hash tables with dense slots. Neither order was observable.
- If the retirement report cannot be reserved, the epoch still advances and the entries are
  retired at the next frame end. The old `erase_if` did not allocate.
- The history shares no lock with the source cache. The pass's calls were single-threaded before
  and stay that way; the lock is defensive.

## Checks

Source comparison against the base, including every former use of `Model`, `View`, `epoch`,
`written`, `view_key` and `ModelKey`. `cargo fmt -p rsf-ac7` with the pinned toolchain, locked
offline Cargo metadata, and Git whitespace. The Rust tests in `velocity_history/tests.rs` cover
validity across consecutive, skipped, reset and wrapped frames, capacity doubling, overflow
refusal after rotation, particle and history-key identity, allocation retry and extent change,
same-frame epochs, commit matching, retirement timing, slot reuse, reset and null pointers. They
were not run. Build, link, Cargo tests, Clippy, CTest including `ac7_velocity_shader`, deployment
and game runs are user-owned and not done.

## Remaining in C++

Capture and stream buffer sizing, indirect argument validation, record counts and packing,
per-draw parameter packing (rect, jitter, opacity cutoff, history flag and mask), the commit's
native depth, format and device checks and merge, CPU profiling, the device renderer lifecycle and
every D3D11 call and HLSL source. The stream policy port later
moved the sizing, indirect range, record counts, packing and parameter packing.
