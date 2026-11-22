# Unity native bridge leaves

The U03 follow-up starts at `e5d73636`. It moves the remaining first-party
dispatch from both C++ bridge paths into the existing Unity Rust archive.

| Area | Source closure |
| --- | --- |
| Shared host and managed entry points | `games/unity-mono/src/bridge_native.rs` owns host publication, managed API construction, callback and packet leases, exports, CPU fact delivery, and the UnityPlayer registry fingerprint. Mono still uses the original RVA, expected bytes and SHA-256, now checked before invoking the typed Unity lookup. The Win32 message hook calls the U02 classifier while the message is live and always forwards through `CallNextHookEx`. |
| D3D12 | Rust reserves and configures the three Unity events through SDK leaves. The package's second configuration of the window event still binds the active render texture. Rust still resets the package slot before dispatch, checks capacity before inline capture, and flushes the exact session/frame/view receipts before delivering the window pass. |
| Package lifecycle | `bridge_native/package.rs` selects the renderer, owns open, close and commit, gets the preview device and queue, and routes Vulkan reconfiguration. The package still stops Vulkan admission before checking callbacks and Vulkan completion. Preview drain can refuse that retirement, and commit releases the preview before graphics. |
| D3D11 | `bridge_d3d11.rs` calls the existing Runtime graphics C ABI through targeted bindings and takes typed references in Rust. The native-queue unwrap, same-adapter fallback, shared fences and two-timeline retirement keep their existing order. The prior D3D11 C state header and the package's COM output shim were retired. |

The D3D11 surface lease now constructs its drop guard only after
admission succeeds. An eagerly constructed guard on refusal would end
another callback's lease. The shared frame callback guard uses the same
success-only construction.

## Remaining C++ calls

- `games/unity-mono/src/bridge.cpp` binds the pinned Unity interfaces and
  adapts SDK GUIDs, event configuration, recording state and
  `ExecuteCommandList` resource-state records.
- `integrations/unity/native/bridge.cpp` binds Unity's D3D11 device and
  delegates Vulkan SDK registration to U04. It also adapts U04's owning
  `ComPtr` pair and contains the existing C++ exception guards around
  Vulkan render and window creation. U04 implementation bodies and the
  U05 window presenter are unchanged.
- The public NativeRenderingPlugin pin `522254181faf188efa8b50c3e3bf6fce720b26e4`
  has no D3D11 `GetSwapChain` member. The provisioned local header extends
  that interface. The leaf uses the member only when the supplied header
  declares it. With the public pin, a D3D11 window pass has no engine
  swapchain pointer and HUD-less capture requiring the implicit swapchain
  buffer is refused. Packet-colour and Editor preview copy routes still
  use their existing input. Neither header provides runtime validation of
  the extension.

## Source checks

Rust 1.94.1 formatting and locked offline Windows metadata passed for the
package and Mono feature configurations. Bindgen 0.72.1 reproduced the
targeted bindings. The shared packet and surface bindings matched the
base byte for byte. Standalone MSVC `/Zs /W4 /WX` passed for the Mono
bridge with and without the SDK, the shared D3D12 package bridge, and the
package bridge and its caller. Those checks used the public pin, with a
second D3D11 bridge check using the provisioned extension header.

Source comparisons verified the managed ABI and exports, the unchanged
packet/receipt bodies and classifier, the original registry fingerprint,
the D3D12 event and recording order, the Vulkan policy and retirement
order, and the unchanged U05 presenter. Builds, fixtures and live device
behavior remain unverified.
