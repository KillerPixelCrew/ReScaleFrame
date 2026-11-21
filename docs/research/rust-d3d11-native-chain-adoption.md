# D3D11 outer native-chain adoption in Rust

Compared with `119892c2`, `d3d11_host.rs` now owns the transaction behind
`rsf_d3d11_host_wrap_native_swapchain`, which the C++ `wrap_chain` platform
entry used to run. The export's admission is unchanged: ABI prefix, null
device, context, slot and slot contents return `INVALID`; a bridge mode other
than 2, a missing bridge, a contended lease, or a configured or scene-bound
host return `NOT_READY`. The graphics lease stays held across the adoption.

The private table replaces `wrap_chain` with four typed leaves at slots
15-18: `wrap_native` (calls `rsf_d3d11_present_wrap_native_chain`),
`com_identity` (`QueryInterface` for `IUnknown`), `chain_present` (reads
`IDXGISwapChain` vtable slot 8 for the log) and `com_release` (returns the
reported count). The table grows from 57 to 60 pointers; `auxiliary` moves
to slot 59. C++ static assertions and the Rust const block check the size and
the first and last new slots.

The validation order is the same. The context must be immediate (the existing
`is_immediate` leaf, `GetType`), then the context's device comes from the
existing `child_device` leaf (`ID3D11DeviceChild::GetDevice`). Then
`same_object` queries `IUnknown` on the declared device and the context's
device in that order and compares the pointers. Either failure returns
`INVALID`. A null context device now fails the comparison. Before, it would
have dereferenced a null `ComPtr`. `GetDevice` does not return null for a
live context.

The facade must come back with a successful HRESULT, be non-null and pass
`present_is_owner` before the caller's slot changes. A failed HRESULT or null
facade logs the same `HRESULT=0x%08lx` line and returns `NOT_READY`. A
non-owner facade is released, then logged, then `NOT_READY` is returned. Only
after the identity receipt (both chains, their `IUnknown` identities and
Present entries, and a second owner check) does the slot take the facade's
reference. The diagnostic native identity reference is dropped first, then
the caller's native reference is released and the count logged. The receipts
keep their former text. Pointers use the MSVC `%p` form (`{:016X}`), as in
`overlay_host.rs` and `d3d11_present.rs`.

Ownership audit. The bridge header says a set `out` owns one facade
reference. The old wrapper ignored a non-null facade beside a failed HRESULT
and never released it. Rust now takes ownership of any non-null `out` and
releases it on refusal. The current bridge clears `out` first and sets it only
on `S_OK`, so this is a defensive correction and the bridge's behavior is
unchanged. Exception path: the bridge has its own function-try-block, and
the other former throw sites (`QueryInterface`, `Release`, `snprintf`, the
Rust `host_log` callback) cannot throw. The old `catch (...)` was effectively
unreachable. Had it fired after the slot write, it would have returned
`NOT_READY` with the facade already installed. The `wrap_native` leaf keeps a
`catch (...)` returning `E_FAIL`, which Rust refuses with `NOT_READY` before
touching the slot. Rust formatting and logging cannot unwind.

The public export, `rsf_d3d11_native_chain` layout, `rsf_d3d11_present_*`
declarations, auxiliary SR, presentation bridge commands and `install_bridge`
are unchanged. Source comparison matched the 60 slots by name and order
across the C header, the C++ designated initializer and the Rust mirror. Leaf
signatures were compared by hand. Pinned rustfmt (1.94.1), locked offline
Cargo metadata and `git diff --check` passed. C++ compile and link, Clippy,
Rust and native tests, and device and game verification of a mode 2 outer
creation are pending.
