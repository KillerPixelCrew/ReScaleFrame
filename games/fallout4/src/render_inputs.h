// SPDX-License-Identifier: MIT
#pragma once
// Native World ownership. The Rust/C boundary is in render_inputs_platform.h.
#include "render_records.h"
#include <rescaleframe/game_renderer.h>
#include <d3d11.h>
#include <dxgi.h>

// Stack-owned synchronous callback lease. Never retain this snapshot after the
// native callback; allocation references do not retain the frame's contents. Every
// nonnull COM pointer is one owned reference: release the lease with
// rsf_fallout4_native_inputs_release and move it with rsf_fallout4_native_inputs_move.
typedef struct NativeInputs {
    ID3D11DeviceContext* context;
    IDXGISwapChain* chain;
    ID3D11Texture2D* colour;
    ID3D11Texture2D* depth;
    ID3D11Texture2D* motion;
    // Optional full-output allocations. Native logical targets 14, 30 and 15;
    // 30 carries Pip-Boy material class, 15 carries VATS coverage. Their actual
    // formats remain native facts. A null lease excludes only its consumer.
    ID3D11Texture2D* refraction_normals;
    ID3D11Texture2D* pipboy;
    ID3D11Texture2D* vats;
    rsf_game_render_pass pass;
} NativeInputs;

// Release every held reference and clear the record.
void rsf_fallout4_native_inputs_release(NativeInputs*);
// Release the target, then take the source's references and clear the source.
void rsf_fallout4_native_inputs_move(NativeInputs* target, NativeInputs* source);

// Raster/resource lease only. No temporal camera/history/depth admission. All
// downstream allocations are conservatively required before native reduction.
// The output is cleared first and holds references only on a true return.
bool rsf_fallout4_read_native_raster_inputs(NativeInputs* output, const rsf_game_render_pass* frame,
    const NativeDepthSnapshot* producer);
// Supplies native resource/camera/rect/frame facts only. Caller supplies the
// verified session/source/submission/scope, role, screen and eligibility facts.
bool rsf_fallout4_read_native_inputs(NativeInputs* output, void (*report)(const char*),
    const NativeDepthSnapshot* producer);
