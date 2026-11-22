# Unity SR D3D12 fixture port

The base is `19680cf1`. The fixture keeps the executable
`rsf_unity_sr_d3d12_test`, the no-argument CTest route `unity_sr_d3d12_contract`,
and the optional argument sequence
`backend streamline-directory fidelityfx-directory xess-directory [vendor-id] [shared]`.
The Rust entry accepts native Windows arguments and validates numeric arguments before using
them. Exit codes remain 0 for completion, 1 for failure, 2 for usage or invalid arguments,
and 77 for unavailable device or provider preparation.

| Replay | Preserved contract |
| --- | --- |
| No arguments | Check the 384-byte camera record, 520-byte Unity packet and 72-byte managed API. Reject null SR12 setup and evaluation, and check that create clears its output pointer. Compare binary16 edge values and the reversed-Z camera against independent frustum coordinates and both inverse products. No GPU creation on this route. |
| Direct SR12 | Use a feature-level 12.0 D3D12 device and optional non-software vendor selection. Upload the original FP16 HDR colour, reversed depth and zero motion, evaluate the selected provider, submit and join the real queue, then read finite centre RGB on the original thresholds. Backend zero preserves the input-only and native copy path. |
| Optional SR diagnostics | `RSF_UNITY_GPU_DEBUG` enables and inspects the real debug layer. `RSF_SR_ENGINE_EXTENTS` requests 426×240 using ABI 2. `RSF_SR_TYPELESS_INPUTS` preserves the original resource format substitutions. `RSF_DLSS_TEST_PRESET` validates through the native header helper. `RSF_SR_EXTERNAL_EXPOSURE` still supplies the 1×1 R32F pass texture, with the baseline setup flags. This replay alone does not establish that the provider consumed external exposure. |
| Shared Streamline | Adopt the real host, acquire and pace source 1, report the original markers and set FG constants on viewport zero using the same SDK token after SR evaluation. Destroy SR before completing the shared token. The constants assertion no longer depends on Streamline headers at build time. |
| RDR2 shared host | `RSF_RDR2_HOST_INI` and `RSF_RDR2_HOST_ROOT` invoke the existing RDR2 setup and Runtime host. Send the final-scene pass, apply live sharpening while two more recordings are unsubmitted, require BUSY on the fourth, and prove slot reuse after actual submission and fence completion. Call host Present bookkeeping around actual DXGI Present. `RSF_RDR2_HOST_SWITCHES` polls the live INI for FSR3 then DLSS and checks that the same interposer remains loaded. |
| RDR2 cold FG | `RSF_RDR2_COLD_FG` creates the intercepted three-buffer `sgaWindow` chain. Replay 140 CPU-only sources and an ordered Present, then 40 rendered sources. Preserve real DXGI Present, queue submission receipts, the keyed input and ping events, pace/order/duplicate output, the FG tags and mode checks, and the optional overlay cursor loop. |

Typed Windows COM calls, data upload, recording, fence joins, window ownership and
scenario policy are in `tests/game-sdk/src/unity_sr_d3d12.rs` and its `gpu.rs` and
`rdr2.rs` modules. The Windows CRT calls the fixture's C ABI `main` directly from
the Rust archive. The fixture still uses Windows wide arguments and returns its
original exit codes. The shared DXGI binary16 decoder and the portable reverse-Z
camera in `math.rs` replace the two DirectXMath leaves. The matrix check derives
near and far frustum clip points separately from the matrix constructor, and
checks inverse multiplication in both orders.

`tests/unity_sr_d3d12_preset.c` is a C11 wrapper for the header-inline preset validator,
which bindgen cannot call. The shared-Streamline check calls `slSetConstants` from Rust.
The interposer exports it with C linkage and its three reference parameters are pointers, so
`streamline.rs` loads it from the host's module and passes the host's real frame token. Its
repr(C) mirrors of `sl::Constants` (version 2) and `sl::ViewportHandle` repeat the SDK's
versioned header and member defaults. Layout assertions carry the sizes and offsets MSVC reports
for the Streamline 2.14.1 headers, and a one-off comparison of the mirror's bytes against a
constructed `sl::Constants` with the same fields matched exactly. No Streamline header is
needed to build the fixture, so `tests/unity_sr_d3d12.cpp` is deleted and the omitted-assertion
branch for missing headers is gone. The Rust bindgen surface covers the Runtime and GameSDK calls
from `tests/unity_sr_d3d12_abi.h`, now including the host module and token accessors.
The tests keep the existing before/after queue and Present callbacks. U02 view
receipts and production Unity bridges, headers and managed interfaces stay outside
this fixture change.


Verification of the follow-up port, based on `9c8fe985`. The fixture had never type-checked.
The package-only build needed the `windows` crate `std` feature for its COM error
conversions, and `DefWindowProcW` is a Rust-ABI item that must sit behind an
`extern "system"` window procedure. The first run of the new matrix check caught a sign error
in the ported projection depth term: DirectXMath's reversed call gives `near / (far - near)`.
A `cargo test` unit test now runs the binary16 and projection comparison without a GPU. The
generated bindings drop the two DirectXMath declarations, and `type rsf_result` is no longer
emitted because nothing references it. The bindgen 0.72.1 and libclang tools were not
installed, so the generated file was edited to the output the changed header and generator
produce, and the next regeneration should confirm it.

Checked: the fixture crate builds and `cargo test` passes with the feature, `cargo clippy`
reports nothing for it, MSVC `/W4 /WX /Zs` accepts the C leaf, and the fixture archive plus the
C leaf link with `main` resolved from the archive. The full executable does not link on the
integration branch used for this check: `rsf-rdr2` (`games/rdr2`) and `rsf-project-wingman` fail `cargo check --workspace` for reasons outside this fixture. GPU replays remain user-owned.
