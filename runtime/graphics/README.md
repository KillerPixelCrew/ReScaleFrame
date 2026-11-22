# Graphics helpers

D3D11 utilities shared by the carriers and runtime. Game-specific view interpretation lives in `games/ac7`; vendor evaluation lives in `runtime/backends`.

| Module | Purpose |
| --- | --- |
| `d3d11_observer` | Track selected allocations and invoke a Present callback |
| `resource_roles`, `frame_tap` | Classify resources, find candidate colour/depth/motion bindings, and describe the draws into a named target |
| `resource_ref` | Retain and release COM resources across the C ABI |
| `constant_buffer_read` | Stage and read a buffer with type/device checks |
| `motion_decode`, `motion_resolve` | Decode biased sparse velocity and resolve dense camera/object motion with validity/depth conventions |
| `d3d11_state` | Save everything the device context has bound, and put it back |
| `texture_dump` | Read supported texture formats into diagnostic TGA/JSON files |
| `fullscreen_pass` | Fullscreen copy, tonemap, premultiplied composite and coverage modes; composite formula `ui.rgb + (1 - ui.a) * dst` does not by itself validate a game UI guide |
| `overlay_renderer`, `overlay_input` | Render egui meshes and collect window input |

The frame tap reports geometry draws, research draw facts and constant uploads to the AC7 motion
capture and velocity pass, and applies the native translucency sampler bias. The AC7 SR path uses
engine-owned views, graph roles and queued identity. The earlier D3D11 binding-matcher renderer
(interface extraction, scene promotion, depth replay) was removed; Git history and
[the extraction note](../../docs/research/ac7-ui-extraction.md) keep its measurements.

## Present is hooked by detour, beside the other hooks

Steam's overlay and RivaTuner Statistics Server both patch the entry of dxgi's Present and re-check it on their first present, one taking what it displaced as its original and the other dropping it. A vtable patch there recursed until the stack overflowed on the first Windows launch. The observer now follows the jump chain from the entry to the first function inside a module, the outermost hook's own function or dxgi's Present when nothing is patched, and detours that with MinHook; nobody re-asserts its own function's prologue. A trampoline over the file's bytes for the entry reaches the genuine body for a re-entered call. `overlay_input` uses the same library to detour the user32 cursor functions while the panel is open, which is how the panel takes a mouse in a game that warps and hides it. [The note](../../docs/research/windows-present-hook-coexistence.md) has the measurements.

## The Windows runtime rewrites its vtable

The stock D3D11 runtime keeps the immediate context's vtable on the heap and rewrites the whole work-submission family of entries, draws, dispatches, copies and clears, whenever a flush-class call runs and again on the next piece of work, flipping between two implementations. Each rewrite discards patched hooks. DXVK's table is static, which is why every Wine run of the tap's test passed and why no Windows run of this project could have observed a draw after the first read-back. Every hook now checks a sentinel slot on entry and re-applies the table when it is gone, the work-submission hooks check again on the way out, the flush-class calls are hooked as pass-throughs for that check, and `rsf_frame_tap_refresh` covers the flush inside Present. `vtable_refreshes` in the status counts it. [The measurement](../../docs/research/d3d11-runtime-vtable-rewrite.md) has the slot table.

Run context work on the owning render thread. Save and restore all affected graphics state around injected work. Hook installation, rollback, and teardown must account for callbacks already in flight.

The decode and texture helpers have synthetic tests. The observer has recorded game runs. The state save and restore and the tap's constant upload path are tested under Wine on DXVK against a real device, each checked by asking the context what actually got bound. See [the tracker](../../docs/implementation.md).
