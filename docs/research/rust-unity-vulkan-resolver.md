# Unity Vulkan resolver ownership

The baseline is `17904281` on `NightHammer1000/rust-unity-vulkan-dispatch-owner`. Commit
`4944b717` had moved the saved loader functions, the route keys and the interop extension merge
into `games/unity-mono/src/vulkan_resolver.rs`. This source audit covers what
`integrations/unity/native/vulkan_bridge.cpp` still decided at the baseline and moves it into
that Rust owner. It builds on the [DXGI swapchain audit](rust-unity-vulkan-dxgi.md) and the
[shared graphics device audit](rust-unity-vulkan-shared-device.md).

## Question and approach

The question was which resolver and interception state, and which device-extension decisions,
were still made natively, and whether moving them changes when anything is published or what a
null resolution does. The answer comes from reading the baseline source, the private header and
the Rust module side by side. Nothing here was built or run.

## What moved

| Baseline native decision | Now in Rust | Native leaf left |
| --- | --- | --- |
| `initialize_intercept` returned `get_instance_proc` when the bind succeeded, else Unity's resolver | `rsf_unity_vk_bind_resolver` publishes the interceptor table with the loader's resolver and returns the one Unity uses | Builds the slot-indexed table and casts the result |
| `intercept()` switched the route slot to a native interceptor | `rsf_unity_vk_resolver_resolve` saves the original and returns the registered interceptor | `get_instance_proc` and `get_device_proc` call the loader and pass the result |
| `device_extensions` enumerated and `strcmp`-matched the required names | `available` does the two-call enumeration, sizing, count check and name match | `device_extensions` is one `vkEnumerateDeviceExtensionProperties` call |
| `create_shared` checked the recorded request, then two `vkGetDeviceProcAddr` lookups | `rsf_unity_vk_interop_ready` owns the entry-point names and the short-circuit order | `device_function_exists` performs one lookup |
| `intercept_create_swapchain` fell back to the saved `vkGetDeviceProcAddr` for `vkDestroySwapchainKHR` | `rsf_unity_vk_resolver_device_function` picks the saved function or the fallback key | `resolve_device_function` performs the typed lookup |

The Unity interface pointer, the event base, the typed create-info merge in `create_device` and
all Vulkan, D3D12, DXGI and Win32 calls stay native. The Rust shared-device, swapchain registry,
D3D11 and preview owners are unchanged. The only edit inside the native shared-device leaf is the
one readiness expression, which now asks the resolver.

## Preserved behaviour

- **Keys.** The same five byte-exact, case-sensitive keys: `vkGetDeviceProcAddr`,
  `vkCreateDevice`, `vkCreateSwapchainKHR`, `vkDestroySwapchainKHR` and `vkQueuePresentKHR`.
  `vkGetInstanceProcAddr` is still never routed, so Unity resolving it through the hook gets the
  loader's own function.
- **Instance and device routes.** `vkGetDeviceProcAddr` is intercepted only from the instance
  resolver. Resolved through the device resolver it still returns the loader's device resolver,
  and lookups through that pointer bypass the hook, as at the baseline. The other four keys route
  from either resolver.
- **Initialization route.** A null loader resolver returns null to Unity and publishes nothing, as
  the baseline's failed bind returned `original`. The new incomplete-table refusal is unreachable
  with the constant native table and also publishes nothing.
- **Disabled route.** Without a registered interceptor a routed key forwards its original and is
  not saved. The baseline saved it. That state cannot occur because Unity only receives
  `get_instance_proc` after the table is published.
- **Error routes.** A missing saved `vkCreateDevice`, or a positive count with a null name list,
  still returns `VK_ERROR_INITIALIZATION_FAILED`; a merge allocation failure still returns
  `VK_ERROR_OUT_OF_HOST_MEMORY`. A missing saved `vkCreateSwapchainKHR` or `vkQueuePresentKHR`
  still returns `VK_ERROR_INITIALIZATION_FAILED` from the native interceptor.
- **Enumeration.** The count query, a zero-filled list of that size, and the second query are
  unchanged. An empty list passes null again, as the MSVC empty vector's `data()` did. A
  non-success result, including `VK_INCOMPLETE`, or a second count above the first, refuses
  interop and keeps Unity's request. Allocation failure refuses, as the native `catch` did. One
  deliberate difference: a 256-byte name without a terminator now matches nothing, where `strcmp`
  would have read past the array. The Vulkan specification requires the terminator.
- **Readiness.** The recorded device short-circuits the lookups. Otherwise the import-semaphore
  lookup runs first and the memory lookup only if it succeeded, as with the native `&&`. The mutex
  is not held across either callback.
- **Public ABI.** The Unity plugin exports and `rsfu_*` are unchanged. Only the private
  `vulkan_resolver.h` boundary changed, and `vulkan_bridge.cpp` is its only consumer:
  `rsf_unity_vk_resolver_route` became `rsf_unity_vk_resolver_resolve`,
  `rsf_unity_vk_interop_enabled` became `rsf_unity_vk_interop_ready`, the bind takes the table,
  the create's preflight callback became an enumeration leaf, and the device-function fallback is
  new. `vulkan_bridge.cpp` asserts the 260-byte `VkExtensionProperties` layout that Rust mirrors.

## Publish timing

- The interceptor table and the loader's instance resolver are published under one mutex hold
  before Unity receives `get_instance_proc`. At the baseline only the resolver was published there
  and the switch was compiled in, so no lookup can observe a partial table.
- A routed original is saved before its interceptor is returned, as at the baseline. Interceptors
  read the saved function when they run, not when they were resolved. The slots are process-wide,
  so the last resolution of a key replaces earlier ones for every instance and device. A second
  instance or device whose loader functions differ would redirect the first one's interceptors.
  This is unchanged and has no runtime evidence either way.
- The interop device is recorded after a successful create and cleared when a later
  non-interop create returns the same handle value. `create_shared` reads it later, after Unity
  publishes its device through `IUnityGraphicsVulkanV2::Instance`.
- The swapchain-destroy fallback is looked up per call and never saved, as at the baseline.
- Resolver state is process-lived. `rsf_uvk_unload` and the checked bridge release do not clear it,
  because Unity keeps the returned function pointers for the device's life.

## Null resolutions

- A null name or a null saved resolver returns null from `get_instance_proc` and
  `get_device_proc` before any loader call. This stays native.
- A null loader result for a routed key is returned unchanged and not saved. An earlier saved
  original for that key stays in place. The baseline route behaved the same.
- A null fallback, or no saved `vkGetDeviceProcAddr`, leaves `vkCreateSwapchainKHR` forwarding
  Unity's request unchanged with no `TRANSFER_SRC` usage and no registration. Present then finds
  no source and forwards.

## Gaps found, not changed

- If Unity created a swapchain through the hook but did not resolve `vkDestroySwapchainKHR`
  through it, the fallback destroy only covers rollback inside creation. Unity's later destroy
  goes straight to the driver, so `intercept_destroy_swapchain` never retires the record. Its
  present-ready semaphores leak, and a new swapchain that reuses the handle value is refused by
  registration, which makes creation return `VK_ERROR_INITIALIZATION_FAILED`. Whether Unity can
  resolve the two functions differently is unverified.
- The last-resolution-wins slots described above.

## Verification scope

Source comparison against `17904281` covers the keys, both routes, the initialization, disabled
and error routes, the enumeration sequence, the readiness order and the private signatures on
both sides. New Rust unit tests cover routing, binding, null and disabled resolutions, the
enumeration and name matching, the merge and the lookup order; they have not been run. Rust
1.94.1 `rustfmt --check`, locked offline `cargo metadata` and `git diff --check` passed.
Compilation and linking, Clippy, the tests, device and player runs, deployment and push are left
to the user.
