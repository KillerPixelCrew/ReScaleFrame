# Anomaly tree and grass motion vectors: capture, identity and the b13 history

Status: built and fixture-tested (contract tests, Intel UHD hardware fixture with the D3D debug layer). Not run in the
game. Standard 1.5.3 DX11 (`AnomalyDX11.exe`, SHA-256 `c857b6f4...`), xray-monolith `028ccf5`.

## Question

With `[Anomaly] Motion=1` trees (`FTreeVisual`) and grass (`CDetailManager`) were the last geometry with motion
validity 0: 45% of the main-pass entries were `unidentified_deformed` trees, and the foliage variants always received the
all-zero b13 buffer. What does the foliage VS need as previous state (`games/anomaly/shaders/foliage/README.md`, 5056
bytes), where does the engine hold the current values, and how are they matched across frames without a pointer to the
instance?

## Why this approach

The engine already stages every shader constant in a CPU copy before it uploads it, so the draw's constants can be read at
the draw-time site (`CBackend::constants.flush` hook) instead of being reconstructed from guessed time or wind:

* `FTreeVisual::Render` (`FTreeVisual.cpp:137-163`) sets `m_xform`, `consts`, `wave`, `wind` through `RCache.tree.set_*`
  into the VS constant table.
* `CDetailManager::hw_Render_dump` (`dx10DetailManager_VS.cpp:104-247`) sets `consts`, `wave`, `dir2D`, then writes the
  instance rows (`c_storage`) straight into the staged `array` and draws each batch of up to 61 instances.

Both end in `dx10ConstantBuffer::m_pBufferData`. Reading it after the original flush returns gives exactly what the next
`DrawIndexed` uses, with no new engine-state derivation. The alternatives (derive wind from `Device.fTimeGlobal` and the
environment, walk `m_visibles` and `SlotItem` lists) need much more struct layout and would only approximate the values.

## Evidence (installed exe, `py -3 tests/anomaly_geometry_evidence.py --verify`)

| Fact | Where |
| --- | --- |
| `CBackend::constants.flush` `0x84b930` walks six tables of 14 `ref_cbuffer` pointers (VS, PS, GS, HS, DS, CS, 0x70 apart), each entry calling `dx10ConstantBuffer::Flush` `0x83de40`. The VS table is the first, `lea rbx` base minus 0x70 = **`0xd63e78`** (CBackend `0xd63bc0` + 0x2b8). | flush body |
| `dx10ConstantBuffer::Flush`: `m_bChanged` +0x68, `m_pBuffer` +0x50, `m_pBufferData` **+0x60**, `UpdateSubresource` (context vtable +0x180) copies at call time. `m_uiBufferSize` is +0x58 by source layout (u32 between the two pointers); the reader also requires it to equal the shader reflection's buffer size. | Flush body, `dx10ConstantBuffer.h` |
| The table index is the shader reflection's constant-buffer index (`R_constant_table::parse`, `set_Constants`). The loader now keeps the STOCK reflection (`Prepared::stock`) so the index and variable offsets come from the bytecode the engine itself reflected. | `dx10r_constants.cpp:364-398` |
| `CDetailManager::Render` `0x86a8c0` calls `hw_Render` `0x8aa320` (only caller), which calls `hw_Render_dump` `0x8aa6c0` three times (wave 0, wave 1, still). The dump has exactly two flush sites (full batch, last partial batch), each followed by `DrawIndexed`. | callers/xrefs |
| `FTreeVisual_PM::last_lod` is +0x108 (`pSWI` +0x100), written in `Render` when the LOD argument is non-negative. | `0x8d5a66`, `0x8d5b03` |

New byte guards (`geometry_profile.h`): detour `CDetailManager::Render` and layout guards for `dx10ConstantBuffer::Flush` and
the `last_lod` load. Total 24 expected-16 guards; a changed byte refuses the whole install before anything is written.

## What the runtime does

Geometry layer (`native_geometry.*`)

* `CDetailManager::Render` is a detour that sets a thread-local window. Every flush inside it is `DrawTimeFacts::detail`
  in its pass; counters `detail renders` and `flush in detail` appear in the draw-time log line.
* With foliage identity enabled (the motion stage enables it, `Motion=1` and `MotionFoliage=1`) an unregistered tree
  visual is classified `IdentifiedFoliage` instead of `unidentified_deformed`. It is reported inside `identified=` and as
  `identified_foliage=`. The facts carry the visual address, the visual type and, for a progressive tree, `last_lod`.
* `read_vertex_constants` / `write_vertex_constants` access one variable of a staged VS buffer, refusing any buffer whose
  reported size differs from the reflection's.

Identity and history (`motion_foliage.*`, pure CPU)

* Tree: the visual address. A level tree's `m_xform` never changes, so a previous record only matches when the xform bytes
  are equal; an address reused after a level change therefore cannot supply history (`foliage_xform_changed`). A progressive
  tree whose slide window changed is invalid (`foliage_lod_changed`).
* Grass: an instance has no pointer in the constant buffer, so its identity is what the engine stores per item and does not
  change with fade or time: the world translation (rows 0..2 `.w`) and the item's sun and hemisphere values (row 3 `x`,
  `w`). Batch number, batch slot and upload order are never identity. Previous rows are bound to the CURRENT group of the
  same item, only when both frames drew it through the same route (VS permutation plus wave set, because previous
  wave/wind are per route).
* Live groups: the batch size is not observable at the flush (the engine never clears the unused tail of the array), so
  after each read the stage fills the staged `array` with a NaN pattern. The flush already uploaded the rows; the engine
  rewrites groups 0..n-1 before its next draw and a draw only addresses those groups, so the tail is never read, and the
  next batch's live groups are exactly the non-poisoned leading ones. This is the one place the stage writes engine memory
  for foliage; it is guarded (size-checked buffer, `__try`), counted (`rows poisoned`, `poison_failed`) and switched off
  with `MotionFoliage=0`.
* Fade is folded into the effective scale and cannot be separated from the rows, so every valid item carries fade 1.0 and
  scale = length of row 0; the shader only uses these as a gate.
* Previous projection: previous camera P*V with the previous jitter removed (`remove_jitter`), so the foliage previous clip
  is unjittered as the shader contract requires; the pixel shader still gets previous jitter 0 for foliage.

b13 packing (`pack_foliage_tree`, `pack_foliage_detail`): layout checked against the README table and the HLSL header in
`anomaly_motion_contract`. A draw without previous state (first frame, camera cut, tree/route not drawn last frame, LOD
change, xform change, unreadable constants, world matrix not identity for grass) gets the initialized all-zero buffer.

## Counters (periodic log, `Anomaly motion foliage ...`)

`tree replaced/stock/valid/invalid`, `detail replaced/stock/valid/invalid`, `detail items/matched/unmatched/conflicts`,
`rows poisoned/poison_failed`, `table_overflows`, plus the new invalid reasons in `Anomaly motion invalid reasons`
(`foliage_no_constants`, `foliage_no_previous`, `foliage_lod_changed`, `foliage_xform_changed`, `foliage_world`).
"Replaced" is a foliage draw whose VS and PS variants were swapped in; "stock" a foliage draw (by geometry facts) that ran
with the stock shaders (no linkable variant, for example the blocked `deffer_tree_s_bump_d.vs` root). A detail draw is
valid when at least one instance was remapped; unmatched instances inside it are zero individually.

## Tests (no game)

* `anomaly_motion_contract`: README/HLSL layout rows, reflection layouts, packing, identity remap of instanced batches
  (items moved between batches and slots, new plants, culled plants, route change, conflicts, stale poison tail, resize,
  reset, scope), tree history and decision policy.
* `anomaly_motion_device` (Intel UHD hardware, debug layer on, local deployment): every deployed foliage root has a usable
  layout; a real `deffer_tree_flat` variant draws a sheared crown whose motion equals the CPU wind model, the base does not
  move, unchanged wind is valid zero, a pan is the camera vector, no history is zero and native MRT0/1 are bit-identical;
  a real `deffer_detail_w_flat` variant draws two plants whose previous rows were in other groups, the production remap
  binds them, motion equals the CPU model including the grown plant, an unmatched plant stays zero.
* `anomaly_geometry_hooks_contract`: 17 production sites on fixtures, foliage verdict, detail window and constant access.

## Not proven

Nothing here ran in the game. Open items for the first run: that `m_uiBufferSize` (+0x58) matches (the size check would
make every read fail and show as `foliage_no_constants`), that the engine issues no extra flush inside a detail batch (a
spurious one would count as an invalid detail draw; compare `flush in detail` with `detail replaced+stock`), that detail
shadow passes use a VS with the same layout (otherwise their stale tail leaks into the first G-buffer batch and shows as
`detail conflicts`), per-frame cost of the item table with real grass density, and image equality with `Motion=1`.
`deffer_tree_s_bump_d.vs` stays blocked (recursive include), so those trees stay stock.
