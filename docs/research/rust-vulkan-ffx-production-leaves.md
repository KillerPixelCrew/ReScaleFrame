# FidelityFX Vulkan production leaves

This source port starts at `6cb0c169`. It follows the typed Vulkan command
owner, G07 capture owner, and C01 shared FidelityFX provider-name parser
already integrated at that revision. The device and state fixtures use the
same entry points and callback tables.

| Checklist source | Evidence |
| --- | --- |
| `runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp` and `native_sr_fsr4_vulkan_platform.h` | Rust owns the graphics capsule, Vulkan barrier and copy recording, semaphore wait, device idle, sharpening and its command retirement. The shared G07 owner receives the same provider, record, reset, capture role and completion evidence. The SDK-gated C++ table retains exception containment at the borrowed resolver, split callback and bridge calls. |
| `runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp` and `native_fg_fsr4_vulkan_platform.h` | Rust creates the shared DX12/Vulkan importer with its generated callback ABI and binds the existing bridge image, release and fence operations. The SDK-gated C++ table still wires the bridge entry points and shared graphics table. |
| `runtime/vulkan/fsr4/bridge.cpp` and `bridge_platform.h` | Rust enumerates the matching DXGI adapter, reads its vendor, creates the D3D12 device and queue, and creates the local fence, event, three allocators and initially closed list. The capsule adopts partially created objects for checked cleanup. Rust also reads and arms the local fence, creates and exports the shared timeline fence, and describes and releases COM textures. The provider query uses C01's shared version parser and retains the separate numeric-shape flag. |
| `runtime/vulkan/fsr4-fg/bridge.cpp` and `bridge_platform.h` | Rust creates the DXGI factory, matches by LUID, reads the vendor, and creates the D3D12 device, queue, allocator, list, local fence and event. Rust also creates and exports the shared fence, resets and closes commands, and releases imported images. The native capsule retains the SDK-owned descriptor storage and its existing checked teardown. |
| Public SR/FG API headers and fixture sources | ABI and fixture bodies are unchanged. The SR graphics dispatch handoff and partially populated command record are private. |
| Standalone FSR3 receipt inputs, `sdk-fixes/` and `wsi-dispatch.h` | The tree remains byte-identical to the base, including the patch recipe and ordering. |

The SR capture owner still uses shared-submission mode, records before output
release, retires the exact command handle, and uses the drained flush during
teardown. FSR4 FG continues to use the shared graphics capture and retirement
callbacks. SR still enumerates adapters and returns its existing no-match code.
FG uses DXGI's direct LUID lookup and passes its HRESULT through. Both preserve
the shared fence object when NT handle export fails. A late SR setup failure
returns the created references to the native capsule for the existing cleanup.

The retained C++ seams include the pinned SDK headers and descriptor storage,
the SDK's DX12 resource-description helper, module loading and cleanup,
the INT8 lease, legacy standalone Vulkan exports, and the C++ exception
boundaries. No SDK build gates changed.

Bindgen 0.72.1 regenerated the repository C ABI and the FSR4 and standalone
FSR3 SDK projections. Rust 1.94.1 formatting, locked offline metadata and
Git whitespace checks completed. Compilation, linking, fixture execution,
lint and device verification remain pending.
