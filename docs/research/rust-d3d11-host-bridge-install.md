# D3D11 host bridge installation in Rust

Compared with `aa66153c`, the generic callback host now constructs the
presentation setup, selects the installation mode and publishes startup FG
options in `d3d11_host.rs`. The comparison follows the old C++ `install_bridge`
from the admitted host setup through the presentation and FG entry points.
Those calls are the handoff to graphics ownership. The host still installs
the bridge after allocating the process-lived callback user and before
publishing the returned services and owner.

| Source | Presentation setup |
| --- | --- |
| Host-owned paths | Runtime and Streamline use the Streamline directory. FSR3 and FSR4 use the FidelityFX directory. XeSS uses the XeSS directory. |
| Adapter metadata | Engine type, copied engine version and project ID, and units to meters pass through. Both depth fields take the adapter's `depth_inverted` value. |
| Host callbacks | Log, user, window acceptance and Present observation use the existing host callbacks and user pointer. Native FG prepare and retire keep their signatures. The latency callback ignores its user argument and forwards the same Present event to native FG. |
| Startup policy | Backend 0, runtime switching enabled, HUD-less backbuffer UI mode and a limit of 15 generated frames. Rendered-source hosts allow FSR3 and FSR4. Other hosts pass a zero choice mask. |
| Descriptor defaults | Struct size is 184 bytes on x64. Development runtime and debug timing are zero. Before-Present is null. |

Mode 0 returns before any bridge or FG call. Mode 1 installs the cold factory
hooks, mode 2 prepares the explicit creation path and mode 3 installs scoped
factory hooks. The corresponding presentation entry points are the same.
The stack descriptor is borrowed for the call. The presentation owner copies
the descriptor and retains its own strings in `rsf_core_d3d11p_prepare`.

For every enabled mode, the host sets rendered-source mode before calling
the selected installer. The previous update on refusal is preserved. That
setter reaches the existing native CPU rendered-source flag. A nonzero
installer result sets the FG log sink, then publishes FG Off with one
generated frame, Reflex Off, no frame limit and debug disabled. A zero result
skips both calls. The host stores the result with release ordering and logs
the same mode or refusal message. Refusal still permits host service creation.
Each native installer leaf translates an escaping C++ exception to zero,
as the previous wrapper did. The three FG setters forward to Rust state
updates in `native_cpu.rs` and `native_fg.rs` and run no throwing native work.

The source audit compared the existing C headers with all 27 field offsets,
size and alignment of `rsf_d3d11_present_setup` in `fg12_abi.rs`, and the six
fields of `rsf_native_fg_options` in `render_abi.rs`. The public headers and
export definitions match the baseline. The private table replaces its single
installation policy entry with nine typed entries. All 68 slots match by
name and order across the C header, C++ initializer, Rust mirror and existing
fixture initializer. The four native adoption leaves keep their original
signatures and implementations at slots 23 through 26. The Rust adoption
transaction from `1c2581d2` matches the baseline.

Pinned rustfmt 1.94.1, locked offline Cargo metadata and whitespace checks
passed. C++ and Rust compilation, linking, Clippy, tests, and device and game
validation of the bridge modes remain pending.
