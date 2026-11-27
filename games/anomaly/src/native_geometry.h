// SPDX-License-Identifier: MIT
#pragma once
#include "native_geometry_platform.h"
#include "native_hooks.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// The geometry producer's native entry points. The producer is the C11 leaf native_geometry_platform.c under the Rust
// owner (native_geometry.rs); the hooks, scope guards and guarded engine reads stay native, the decisions do not.
// Callers use the Rust exports in native_geometry_platform.h for everything that needs no engine memory:
//   rsf_anomaly_geometry_active()              geometry hooks installed and capturing
//   rsf_anomaly_geometry_set_flush(fn)         shared CBackend::constants.flush slot. Called on the render thread AFTER
//                                              the original flush returned, i.e. after every native constant upload
//                                              and state apply and immediately before the engine's D3D draw call,
//                                              whether or not geometry capture is enabled. Must not throw or
//                                              reenter the renderer. Null clears it. Set only with hooks quiescent.
//   rsf_anomaly_geometry_set_near(fn)          shared CRender::rmNear slot. The geometry producer owns the only detour
//                                              on rmNear (geometry site 14); other adapter users (the SR world window)
//                                              register here instead of patching the same bytes a second time. Called
//                                              on the render thread after the original returned, whether or not
//                                              capture is enabled. Null clears it.
//   rsf_anomaly_geometry_set_foliage(enabled)  foliage identity: trees are level visuals that never enter the model
//                                              registry; with this on an unregistered FTreeVisual is classified
//                                              IdentifiedFoliage (named by its address) instead of
//                                              UnidentifiedDeformed. Off by default. Set only with hooks quiescent.
//   rsf_anomaly_geometry_set_draw / _set_retired  draw sink and retirement sink (see the callback types).
//   rsf_anomaly_geometry_stats_read, _coverage_read, _report  monotonic counters for live validation. They prove hook
//                                              invocation, not temporal correctness.
//   rsf_anomaly_geometry_reset()               device or session teardown. Generation never resets.
#ifdef __cplusplus
#define RSF_GEOMETRY_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_GEOMETRY_NOEXCEPT
#endif

// `build` is the index of the profile selected for the running executable by its exact name and SHA-256 (the order of
// native_profile.rs). Every site and unique guard of that build is checked first.
bool rsf_anomaly_geometry_install_native(void* image, size_t build) RSF_GEOMETRY_NOEXCEPT;
bool rsf_anomaly_geometry_disable_native(void) RSF_GEOMETRY_NOEXCEPT;
// Opens the view and World context for one CRender::Render around `original(self)`, including on a C++ unwind out of
// the original. Inert when geometry hooks are not active.
void rsf_anomaly_geometry_render_scene(void (*original)(void*), void* self);
// Main (0), scope (1) or 255 when the bytes are unreadable, from the engine's own IsSVPFrame predicate
// (isActive && dwFrame % frameDelay == 0). `module` is the executable base of build `build`.
uint8_t rsf_anomaly_geometry_current_view(const void* module, size_t build) RSF_GEOMETRY_NOEXCEPT;
// The world matrix in force for this draw (RCache.xforms.m_w), row-vector Fmatrix floats. False when unreadable.
bool rsf_anomaly_geometry_world_matrix(float out[16]) RSF_GEOMETRY_NOEXCEPT;
// Guarded access to the CPU copy the engine stages for vertex-stage constant buffer `slot` (the reflection's
// constant-buffer index of the shader bound for this draw). `expected_size` is that buffer's reflected size; the engine
// object must report the same size or nothing is read or written. False on any mismatch or fault.
bool rsf_anomaly_geometry_read_constants(uint32_t slot, uint32_t expected_size, uint32_t offset, void* out,
    uint32_t bytes) RSF_GEOMETRY_NOEXCEPT;
bool rsf_anomaly_geometry_write_constants(uint32_t slot, uint32_t expected_size, uint32_t offset, const void* in,
    uint32_t bytes) RSF_GEOMETRY_NOEXCEPT;
// Opens and closes the view/World context of one CRender::Render for callers that own their own guard.
bool rsf_anomaly_geometry_render_view_enter(rsf_anomaly_geometry_context*) RSF_GEOMETRY_NOEXCEPT;
// The copy of the live draw on this thread, owned by `storage` until released. Null when no live identity exists.
typedef struct rsf_anomaly_geometry_capture_storage {
    rsf_anomaly_geometry_snapshot snapshot;
    rsf_anomaly_geometry_matrix world;
    rsf_anomaly_geometry_matrix* bones;
    bool present;
} rsf_anomaly_geometry_capture_storage;
const rsf_anomaly_geometry_snapshot* rsf_anomaly_geometry_capture_current(
    rsf_anomaly_geometry_capture_storage* storage) RSF_GEOMETRY_NOEXCEPT;
void rsf_anomaly_geometry_capture_release(rsf_anomaly_geometry_capture_storage* storage) RSF_GEOMETRY_NOEXCEPT;
// The detour table of `build` (profile order). Valid until the next call. Null for an unknown build.
const rsf_anomaly_hook_site* rsf_anomaly_geometry_hook_sites(size_t build) RSF_GEOMETRY_NOEXCEPT;
#ifdef RSF_ANOMALY_GEOMETRY_TESTING
void rsf_anomaly_geometry_test_bind_module(void* module, size_t build) RSF_GEOMETRY_NOEXCEPT;
bool rsf_anomaly_geometry_test_guard_lifecycle(void* module, size_t build, size_t other) RSF_GEOMETRY_NOEXCEPT;
#endif

#ifdef __cplusplus
}
#endif
#undef RSF_GEOMETRY_NOEXCEPT
