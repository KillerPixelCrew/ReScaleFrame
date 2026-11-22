# Unity native C11 leaves

This closes the U02, U03 and U04 C++ entries in `PORT.md` from `e84cb076` on
`rust/orchestrator-anomaly`. The four first-party C++ files in the Unity package and the Mono
plugin are now C11 leaves. The C++ that only adapted Rust calls or caught exceptions that
could not occur was deleted.

| Former file | Result |
| --- | --- |
| `integrations/unity/native/package.cpp` | `package.c` holds the device event registration, the constant `rsf_unity_package_platform` table, the `rsfu_*` exports and `UnityPluginLoad`/`UnityPluginUnload`. The table lambdas are static functions that only adapt `bool` and `int` results. The layout `static_assert`s are `_Static_assert`s. `package.h` declared two functions with no caller and was deleted. |
| `games/unity-mono/src/bridge.cpp` | `bridge.c` holds the pinned D3D12 interface calls, the `rsf_unity_d3d12_engine` table and the resource-state records. It is compiled into the Mono plugin and, with `RSF_UNITY_PACKAGE_BRIDGE`, into the package. |
| `integrations/unity/native/bridge.cpp` | `bridge.c` holds the renderer query, event reservation and the D3D11 interface. The Vulkan wrappers it held are gone (see below). |
| `integrations/unity/native/vulkan_bridge.cpp` | `vulkan_bridge.c` holds the Vulkan interface calls, the loader hook table for `vulkan_resolver.rs`, the Vulkan layout assertions and the foreign exception guards. `vulkan_bridge.h` now holds three C declarations. |

## What moved to Rust or was removed

- `rsf_unity_package_choose_fg`, `rsf_unity_package_create_planned` and
  `rsf_unity_package_streamline_host` wrapped `rsf_fg_default_backend`, `rsf_sr12_create_planned`
  and `rsf_d3d11_present_host`. All three are Rust exports of the linked Runtime core, so
  `package_state/native.rs` calls them directly. The `try`/`catch` around the planned create could
  not fire, because a Rust `extern "C"` function aborts instead of unwinding.
- The `fg_window` table entry had a `catch_exception` flag for the same kind of dead guard around
  `rsf_fg12_window`, which is Rust. The entry is now `void (*)(const rsf_game_render_pass*)`.
  The preview observer returns 1 once the call returns. `package_abi.rs` was regenerated with
  bindgen 0.72.1.
- `rsf_unity_package_vulkan_create_window`, `rsf_unity_package_vulkan_render`,
  `rsf_unity_package_vulkan_graphics` and the `rsf_uvk_*` C++ wrappers behind them only forwarded
  to `vulkan_bridge.rs` exports and caught exceptions those exports cannot raise.
  `bridge_native/package.rs` calls `rsf_uvk_bridge_create_window`, `rsf_uvk_bridge_render` and
  `rsf_uvk_bridge_graphics` directly. The render call passes the reserved event base Rust
  already keeps in `EVENT_ID`, which is the value the C++ leaf copied at bind time. The log for a
  caught render exception went with the dead path. The `ComPtr` round trip for the preview
  device and queue is gone: Rust receives the two owned references it handed out.
- `rsfu_surface` and the `vulkan_owns` table entry call `rsf_uvk_bridge_update_window` and
  `rsf_uvk_bridge_owns_window` directly.

## Constraints that keep native code

- Unity interfaces are C function-pointer tables, so every call is plain C. Unity's headers, compiled as
  C, define each interface GUID as an external constant. Four package leaves include
  `IUnityGraphics.h`, so these definitions would collide at link time.
  `games/unity-mono/src/unity_sdk.h` gives each leaf's copies a prefix named by
  `RSF_UNITY_SDK_LEAF`, and `RSF_UNITY_GET` replaces `IUnityInterfaces::Get<T>()` with the same
  `GetInterface` call. The Vulkan header's `UnityVulkanWholeImage` null constant gets the same
  treatment.
- The pinned `IUnityGraphicsD3D12.h` names `UnityD3D12GraphicsQueueAccess`,
  `UnityGraphicsD3D12RecordingState` and `UnityD3D12PluginEventConfig` without typedefs, which only
  C++ accepts. `bridge.c` declares the three typedefs before including it.
- The public NativeRenderingPlugin pin has no D3D11 `GetSwapChain`. The C++ leaf used SFINAE.
  `integrations/unity/CMakeLists.txt` now runs `check_c_source_compiles` against the supplied
  header and defines `RSF_UNITY_D3D11_SWAPCHAIN` only when the member exists. Without the
  member, the leaf returns null, which matches the previous behaviour. The result is cached, so
  replacing the header needs a fresh configure.
- Calls into Unity's Vulkan interface, the Vulkan loader and the host log and render-pass
  callbacks keep their exception containment. MSVC raises a C++ exception as SEH code
  `0xE06D7363`. The `__try`/`__except` filter on that code stops the exceptions the former
  catch-all stopped and lets other faults propagate, as in `integrations/steam/src/exception_guards.c`.
  The leaf refuses to build without MSVC structured exception handling.

## Checks

- MSBuild `ClCompile` of `rsf_unity_package` (`package.c`, `bridge.c`, `vulkan_bridge.c`) and
  `rsf_game_unity_mono` (`bridge.c`) with the project's `/W4 /WX` flags, plus a full build of the
  `rsf_unity_package_bridge` object library, passed. `bridge.c` also passed a syntax-only build
  with `RSF_HAVE_UNITY_NATIVE=0`, and the D3D11 leaf passed against a copy of the header without
  the swapchain getter.
- `dumpbin /symbols` shows each GUID copy defined once under its leaf prefix. A trial link of
  only the four Unity objects reported no duplicate definition. It exported `UnityPluginLoad`,
  `UnityPluginUnload` and the ten `rsfu_*` entries unmangled.
- `cargo fmt --all -- --check` passed. `cargo clippy -p rsf-unity-mono --locked --all-targets` and
  the `mono-plugin` configuration fail on the integration tip before and after this change. The
  failures are in `rsf-game-sys` lints, `vulkan_platform.rs`, the `rsf_game_render_pass` `Copy`
  loss and a duplicate `Copy` derive in `package_abi.rs`. A detached worktree at `e84cb076` gives
  the same diagnostics, apart from line shifts.
- Linking the two DLLs and running `game_unity_mono_plugin_contract` and the engine sharpening
  CTest entry wait on the tip: `rsf_rust_core` does not compile, and `tests/CMakeLists.txt` and
  `tests/anomaly_fg.cmake` do not configure.
