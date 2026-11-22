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

## Module and bridge forwarding follow-up from b5a7434c

| Source | Disposition |
| --- | --- |
| `native_sr_fsr4_vulkan.cpp` | The production table now binds Rust directly for provider info, sharpness storage and imported COM image release. The SDK-gated binder still supplies the real bridge and guards calls that reach the bridge SDK, the host proc resolver and the split callback. |
| `runtime/vulkan/fsr4/bridge.cpp` | Rust resolves the owning module's legacy directory and loads and binds the signed provider. Rust releases the list, three allocators, fences, queue, device, adapter and factory in the prior order, then closes the event and unloads the provider. The native capsule still drops the INT8 lease first and deletes itself last. |
| `runtime/vulkan/fsr4-fg/bridge.cpp` | The same Rust Windows loader and release mechanism binds the five typed SDK exports and retires the capsule's COM references, event and module in their established order. Rust releases the DXGI swapchain reference at the existing point before checked swapchain-context destruction. |
| FG Vulkan C++ binder, SDK headers and standalone exports | The optional SDK binder, exact function shapes, standalone exports, typed SDK description helpers, retained create descriptors and SDK destruction calls stay at their existing entry points. G07 capture still enters its existing native service binder where required. |
| Standalone FSR3, fixtures, D07 and `PORT.md` | Git comparison against the base found no changes. |

Both loaders retain the module for checked cleanup on missing-export failure.
SR retains its separate module and export refusal codes, and FG retains the
HRESULT and missing-interface mapping. The Rust SR directory lookup keeps the
borrowed module reference, MAX_PATH bound and trailing separator.

Bindgen 0.72.1 reproduced both changed SDK projections at FidelityFX revision
`7539ae8b6568ad29ed78a753404351daf3b5cca7`. Rust 1.94.1 formatting, locked
offline Windows metadata and Git whitespace checks passed. MSVC `/Zs /W4 /WX`
passed for both native provider binders and the FG bridge, and for both the
embedded and standalone SR bridges. This was a source-only check.
