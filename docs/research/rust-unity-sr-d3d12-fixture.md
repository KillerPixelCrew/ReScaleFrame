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
| No arguments | Check the 384-byte camera record, 520-byte Unity packet and 72-byte managed API. Reject null SR12 setup and evaluation, and check that create clears its output pointer. No GPU creation on this route. |
| Direct SR12 | Use a feature-level 12.0 D3D12 device and optional non-software vendor selection. Upload the original FP16 HDR colour, reversed depth and zero motion, evaluate the selected provider, submit and join the real queue, then read finite centre RGB on the original thresholds. Backend zero preserves the input-only and native copy path. |
| Optional SR diagnostics | `RSF_UNITY_GPU_DEBUG` enables and inspects the real debug layer. `RSF_SR_ENGINE_EXTENTS` requests 426×240 using ABI 2. `RSF_SR_TYPELESS_INPUTS` preserves the original resource format substitutions. `RSF_DLSS_TEST_PRESET` validates through the native header helper. `RSF_SR_EXTERNAL_EXPOSURE` still supplies the 1×1 R32F pass texture, with the baseline setup flags. This replay alone does not establish that the provider consumed external exposure. |
| Shared Streamline | Adopt the real host, acquire and pace source 1, report the original markers and set FG constants on viewport zero using the same SDK token after SR evaluation. Destroy SR before completing the shared token. With Streamline headers absent, the direct SDK constants assertion is omitted as before. |
| RDR2 shared host | `RSF_RDR2_HOST_INI` and `RSF_RDR2_HOST_ROOT` invoke the existing RDR2 setup and Runtime host. Send the final-scene pass, apply live sharpening while two more recordings are unsubmitted, require BUSY on the fourth, and prove slot reuse after actual submission and fence completion. Call host Present bookkeeping around actual DXGI Present. `RSF_RDR2_HOST_SWITCHES` polls the live INI for FSR3 then DLSS and checks that the same interposer remains loaded. |
| RDR2 cold FG | `RSF_RDR2_COLD_FG` creates the intercepted three-buffer `sgaWindow` chain. Replay 140 CPU-only sources and an ordered Present, then 40 rendered sources. Preserve real DXGI Present, queue submission receipts, the keyed input and ping events, pace/order/duplicate output, the FG tags and mode checks, and the optional overlay cursor loop. |

Typed Windows COM calls, data upload, recording, fence joins, window ownership and
scenario policy are in `tests/game-sdk/src/unity_sr_d3d12.rs` and its `gpu.rs` and
`rdr2.rs` modules. `tests/unity_sr_d3d12.cpp` supplies the native DirectXMath
projection and half decoding, the inline preset validator, and the C++ Streamline
SDK call with the actual SDK token and constant types. The Rust bindgen surface
covers the existing Runtime and GameSDK calls from `tests/unity_sr_d3d12_abi.h`.
The tests keep the existing before/after queue and Present callbacks. U02 view
receipts and production Unity bridges, headers and managed interfaces stay outside
this fixture change.

Verified at source level with bindgen 0.72.1 on the fixture ABI only, locked offline
Cargo feature metadata, pinned 1.94.1 formatting and parse-only syntax, and MSVC
`/Zs` for the C entry and the C++ leaf with and without the real Streamline headers.
Rust dependency type checking, native linking, CTest, GPU execution and lint remain
user-owned.
