# Fallout 4 native leaves in C11

PORT F01, F02 and F03 closed the last first-party C++ in `games/fallout4`. The seven leaf files
`host_boundary`, `cpu_events`, `render_hooks`, `render_inputs`, `resolution`, `screen` and
`depth_domains` are now C11 (`.c`). Their decisions were already Rust; what remained needed
structured exception handling, MinHook trampolines, return addresses, COM reference counts or
`snprintf` receipts. No Rust ABI changed: every exported and imported symbol, record layout and
callback table is the one the Rust crate already declared, and the four generated binding modules
(`render_records.rs`, `resolution_stages.rs`, `platform_abi.rs`, `cpu_abi.rs`) regenerate without a
diff.

## Foreign exception containment

`native_guard.h` holds the one policy. MSVC raises a C++ `throw` as structured exception
`0xE06D7363`. A leaf that called `catch (...)` under `/EHsc` now uses `__try` with a filter that
accepts only that code, so an access violation or other fault still propagates exactly as it did
past the old handler. A shell that ran an RAII exit uses `__try` with `__finally`; the Rust
`cancel` or exit runs on a normal return and on a native unwind, then the exception keeps
travelling. Other toolchains compile the same source to the plain sequence, because there is no
foreign unwinding to contain.

`rsf_fallout4_call_creation` was `noexcept`, so a throw out of
`D3D11CreateDeviceAndSwapChain` ended the process. It stays unhandled: nothing resumes a
half-created device chain. The record layout and the 32-bit D3D enumeration assertions are kept as
`_Static_assert`.

## World and leases

The thread-local `World` keeps its fields. `ComPtr` members became raw COM pointers with explicit
`com_assign`, `com_move` and `com_release` helpers that follow `ComPtr` assignment order (add the
new reference, then drop the old one). `NativeInputs` is a plain record with
`rsf_fallout4_native_inputs_release` and `_move`. `world = {}` is `world_clear`, which releases
every lease before it zeroes the record and restores the empty screen. The TLS initializer carries
the same unknown-camera sentinel the C++ member initializer did.

One difference: the C++ `thread_local` destructor released leases when a thread ended while its
World was still populated. The C record has no thread-exit hook. The render thread lives for the
process and the World clears at every admitted world and after Swap, so only a thread that dies
mid-callback would keep its references.

The scope counters were `std::atomic<uint32_t>`; they are `volatile LONG` with `Interlocked*`, and
the busy read is `InterlockedCompareExchange(.., 0, 0)`. The Present announcement flag is
`InterlockedExchange`.

## Detours

`Entry`, `Restore`, `Exit`, `Cleanup` and `PresentPhase` were destructors. Each shell now captures
`_ReturnAddress()` first, then runs its body in `__try` and the same cleanup, in the same order, in
`__finally`. Last-error save and restore points are unchanged. The typed detour and trampoline
tables became `switch` accessors, because a C static initializer cannot take a function address as
`void*` portably.

## Checked

- MSVC `/W4 /WX /std:c11` build of `rsf_game_fallout4` (Visual Studio 2026 generator, Debug)
  including the Rust staticlib; `game_fallout4_plugin_contract` loads the DLL and passes.
- `gcc -std=c11 -Wall -Wextra -Werror -fsyntax-only` over all seven files.
- Unwind semantics, with a throwaway MSVC fixture that linked the real `cpu_events.c` to stub
  Rust leaves: a normal return runs exit and not cancel and leaves the native last error; a C++
  throw from the original runs `cancel` for that hook with the native error visible, skips exit and
  reaches the caller's `catch`; raw input retires on both paths; `rsf_fallout4_cpu_deliver`
  contains a C++ throw and returns false; an access violation is not contained.
- Binding regeneration for the four Fallout 4 modules with bindgen 0.72.1: no diff.
- Not run: a game session, a Release build, and the crate's unit tests as a linked executable
  (they reference MinHook and these leaves, so they only compile standalone).
