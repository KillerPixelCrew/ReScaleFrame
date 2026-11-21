# OpenGL shared-surface owner Rust port

The baseline is `30a15e8`. `runtime/presentation/src/gl_shared_surface.cpp` held
the whole OpenGL and D3D12 sharing owner behind
`rescaleframe/gl_shared_surface.h`. This port moves its policy into
`runtime/orchestrator/src/gl_shared_surface.rs` in the existing orchestrator
Rust archive. The C++ file keeps only native leaves. The header and its
consumers are unchanged: `gl_pipeline_platform.cpp` still places the nine
`rsf_gl_*` functions in the Rust GL pipeline's platform table, and
`tests/gl_shared_surface.cpp` still drives them directly.

## Ownership

| Boundary | Owner |
| --- | --- |
| Admission | Rust records the WGL context and thread at open and admits every call only on both. Setup size, ABI version 1, the device and queue pair, the queue's device and the adapter LUID are checked as before. |
| Formats | Rust maps the four `RSF_GL_*` formats to their DXGI and GL internal formats. The C++ leaves pin the DXGI values with `static_assert`. |
| Lifetime | Rust owns the GL semaphore, the fence NT handle, the CPU event, the D3D12 objects (through `ComRef`), and each texture's NT handle, GL memory object and GL name. Partial open and texture creation release only what they created. |
| Synchronization | Rust keeps three command slots, the fence serial, slot completion values, the GL release before the queue wait, the queue signal before the GL acquire and the 10 s CPU wait. |
| Native leaves | `gl_shared_surface.cpp` resolves the 13 GL extension entry points into storage Rust holds, and performs single WGL, GL, DXGI and D3D12 calls. |

`rsf_gl_open` remains in C++ because it carries the leaf table. It forwards to
the internal `rsf_core_gl_open`. The other eight entry points are defined with
the same names and signatures in the Rust archive, which every consumer of
`ReScaleFrame::Presentation` already links. None of the nine names is a DLL
export, before or after.

## Defect in the baseline submit

In the baseline, `rsf_gl_submit` closed the list, called
`ExecuteCommandLists` and then returned `FEATURE_FAILED` if the queue
`Signal` failed, before clearing `recording`. The commands were already on
the queue, and GL had released the textures without an acquire. After that:

- `rsf_gl_begin` and `rsf_gl_drain` refused every call with
  `INVALID_ARGUMENT` because the owner was still recording, so
  `rsf_gl_close` refused too.
- A retried `rsf_gl_submit` called `Close` again on the already closed list.
- In `gl_pipeline.rs`, `configure` drains before replacing surfaces and
  `close` drains before releasing them. Both kept returning the drain
  refusal, so the pipeline could neither resize nor close for the rest of the
  process.

Two neighbouring failures had the same shape. A refused queue `Wait` in begin
closed the list but left the GL textures released. A refused `Close` in
submit kept the owner recording on a list that could not be executed.

This was found by source review. A synthetic test of the baseline was not
feasible: its `rsf_gl_open` needs a current WGL context exposing
`GL_EXT_memory_object_win32` and `GL_EXT_semaphore_win32` and a D3D12 device on
the same adapter, which is the device fixture this session must not run. The
new Rust tests below drive the same call sequence through fake leaves.

## Corrected recovery

A transaction whose queue work has ended without a successful completion
signal is recorded as an unsignalled handoff with its slot and texture names.
`rsf_gl_submit` returns `FEATURE_FAILED`, and a repeated submit is refused
without touching the list. The next `rsf_gl_begin`, `rsf_gl_drain` or
`rsf_gl_close` first retries the queue signal. Only when it is accepted, and
therefore ordered after the executed list, does GL wait on that value and the
slot receive its completion. Until then drain and close keep refusing, so
callers keep their textures, and the slot is never waited on or reset. The
executed list is never closed or submitted again.

A refused `Close` executes nothing. The owner signals past the queue's
pending GL wait and lets GL reacquire on that value. A refused queue `Wait`
closes the list and has GL reacquire on the value its own signal satisfies.
Both return `FEATURE_FAILED` and leave the owner ready for the next begin.

Each slot also holds a reference to the D3D12 resources its transaction
released to D3D12, until a CPU fence wait proves that slot complete: on slot
reuse or on a successful drain. A texture destroyed while a handoff is
unsignalled leaves that pending GL acquire, while its resource stays leased.
The normal path issues the same calls in the same order as the baseline, with
an added reference and release per leased resource.

Smaller differences:

- The draining of pre-existing GL errors stops after 32 errors instead of
  looping until `GL_NO_ERROR`.
- The GL semaphore is imported after the event and command slots are created,
  so earlier open failures leave no GL object to delete.
- Refused queue waits, closes and completion signals are logged in the
  existing `OpenGL sharing: ... failed (0x...)` form.

## Validation

Nine Rust tests in `gl_shared_surface/tests.rs` drive the owner through a
recording leaf table and reference-counted fake COM objects. The fake fence
completes only when a CPU wait asks for a value the queue or GL signalled, so a
released lease shows an observed completion. They cover these cases:

- The exact baseline call order for begin, submit and drain, plus format
  mapping, slot rotation and a slot-reuse wait.
- A refused signal after execute, with drain and close refused, a repeated
  submit and a texture destroyed mid-handoff. The test checks the retained
  lease, the later acquire on the next accepted value, a single close and
  execute of the list, and one reset of its slot after completion.
- Recovery through the next begin, a refused queue wait, a refused close, a
  stalled slot wait and drain, owner and order admission, and setup refusal.
- Partial open and import failures, which release only what they created.

The checks used the pinned Rust 1.94.1 toolchain, worktree-local Cargo and
CMake directories, and copied yyjson pins.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --locked -- -D warnings` and `cargo test --workspace --locked`: 342 tests
  passed, including the seven existing OpenGL pipeline cases.
- Binding regeneration with bindgen 0.72.1 added only `rsf_gl_setup` and was
  stable on a second run.
- MSVC Release (VS 2026) built the Runtime, the AC7 carrier, the Loader and
  its `version.dll` alias, Minecraft, the Godot extension and
  `rsf_gl_shared_surface_test`. Their export names and ordinals matched a
  separate `30a15e8` build with the same pins: Runtime 381, AC7 256, Loader
  273, `dinput8.dll` 273, `version.dll` 273, Minecraft 267 and Godot 398.
- `native_vulkan_dll_exports`, `orchestrator_domain_contract`,
  `gpu_sr_policy`, `gpu_sr_policy_embedded`, `shared_shim_exports` and
  `d3d11_host_contract` passed. `gl_shared_surface` was built and skipped
  without `RSF_RUN_GL_INTEROP_TESTS`.

Not run: the OpenGL device fixture, the threaded GL pipeline fixture, Minecraft
and `eng/verify.ps1`. The recovery paths are synthetic-tested only. If a lost
device keeps refusing queue signals, drain and close keep refusing and the
owner retains its resources, because completion cannot be proven. Results of
the GL semaphore signal and wait calls are still unchecked, as before.
