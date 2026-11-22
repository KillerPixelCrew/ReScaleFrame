# Vulkan FG resource and fixture boundaries

Base revision `6c935471`. This follow-up removes the shared C++ Vulkan
resource owner class and retires the old FidelityFX fixture harness header.

- `vulkan_fg_resources.h` declares the existing opaque Rust dispatch table
  and resource operations. FidelityFX and the two DLSS native consumers use
  standard lifetime handles with the existing Rust deleter. Device dispatch,
  memory selection, fences and waits still run in Rust. The XeSS SR leaf still
  includes `vulkan_fg_resources_native.h`, so that file retains only its
  forwarding include and table alias. The XeSS leaf remains outside this
  replay.
- Rust owns the FidelityFX fixture's counters, fault flags and WSI route
  state. The fixture builds actual Vulkan Present records in Rust and still
  enters the same provider exports. `ffx_fixture.cpp` only keeps the native
  leaf and SDK callback records, their typed assertions, and adapters to the
  private hook entry points. A synchronous Present still tries to retire the
  owner while its route borrow is live, and the existing scenario checks
  failed unregister, teardown retry and successor admission.
- `wsi.rs` constructs the corrected SDK's actual typed registration record
  using bindings from its header. The C++ fixture calls the actual registration
  implementation and the five SDK forwarding functions. Foreign-device and
  unregistered paths now check the fallback's distinct swapchain handle or
  Vulkan results, including timeout and incomplete. The registered Present
  callback checks the original queue identity.
- The vendor patch inputs and the SDK entry points and exception guards
  in the native provider shells stay in place. V05 and V06 provider sources,
  public entry names, production queue and receipt paths, and CTest names are
  unchanged. The two DLSS source changes are limited to consuming the shared
  opaque device ABI. DLSS fixtures belong to V04.

Source verification used targeted bindgen 0.72.1 for the resource and V02
fixture bindings. The corrected WSI record uses `VULKAN_SDK/Include`.
Pinned Rust 1.94.1 formatting, locked offline Windows workspace metadata and
diff checks passed. MSVC syntax-only checks passed with `/Zs /W4 /WX` for the
C resource header probe, C++ Vulkan header probe, standalone corrected SDK
fixture and SDK-disabled FidelityFX and DLSS translation units. Syntax of
the optional SDK-enabled leaf and FFX harness and runtime behavior remain
unverified.
