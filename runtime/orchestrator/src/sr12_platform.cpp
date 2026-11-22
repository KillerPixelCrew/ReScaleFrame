// SPDX-License-Identifier: MIT
// Exports, ABI checks and capture lifetime for the Rust D3D12 SR owner.
#include <rescaleframe/native_sr_d3d12.h>
#include "../../backends/common/frame_capture12.h"
#include <new>

// Values sr12.rs names directly.
static_assert(DXGI_FORMAT_R32G32B32A32_FLOAT == 2 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10 &&
              DXGI_FORMAT_R32G32_FLOAT == 16 && DXGI_FORMAT_R32G8X24_TYPELESS == 19 &&
              DXGI_FORMAT_D32_FLOAT_S8X24_UINT == 20 && DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS == 21 &&
              DXGI_FORMAT_R11G11B10_FLOAT == 26 && DXGI_FORMAT_R16G16_FLOAT == 34 &&
              DXGI_FORMAT_R32_TYPELESS == 39 && DXGI_FORMAT_D32_FLOAT == 40 && DXGI_FORMAT_R32_FLOAT == 41 &&
              DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_D24_UNORM_S8_UINT == 45 &&
              DXGI_FORMAT_R24_UNORM_X8_TYPELESS == 46 && DXGI_FORMAT_R16_FLOAT == 54);
static_assert(D3D12_RESOURCE_STATE_RENDER_TARGET == 0x4 && D3D12_RESOURCE_STATE_UNORDERED_ACCESS == 0x8 &&
              D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 && D3D12_RESOURCE_STATE_COPY_DEST == 0x400 &&
              D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800 && D3D12_RESOURCE_DIMENSION_TEXTURE2D == 3);
static_assert(RSF_SR_DLSS == 1 && RSF_SR_XESS == 5 && RSF_API_D3D12 == 2);
static_assert(RSF_SR_AUTO == 7 && RSF_QUALITY_ULTRA_QUALITY == 5 &&
              RSF_SR12_PLAN_ABI_VERSION == 1 && sizeof(rsf_sr12_plan_result) == 24);
static_assert(RSF_DUMP_VIEW_RAW == 0 && RSF_DUMP_VIEW_HDR == 3);
static_assert(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT == 256);

// The bounded original-pixel capture owner. Construction reads the process environment.
extern "C" void* rsf_sr12_platform_capture_create() { return new (std::nothrow) rsf::FrameCapture12(); }
extern "C" void rsf_sr12_platform_capture_destroy(void* capture) { delete static_cast<rsf::FrameCapture12*>(capture); }
extern "C" int rsf_sr12_platform_capture_enabled(void* capture) { return static_cast<rsf::FrameCapture12*>(capture)->enabled(); }
extern "C" int rsf_sr12_platform_capture_configure(void* capture, const char* directory, uint64_t first, uint32_t frames)
{
    return static_cast<rsf::FrameCapture12*>(capture)->configure(directory, first, frames);
}
extern "C" int rsf_sr12_platform_capture_flush(void* capture, uint32_t slot) { return static_cast<rsf::FrameCapture12*>(capture)->flush(slot); }
extern "C" int rsf_sr12_platform_capture_record(void* capture, void* list, void* source, uint32_t state, const rsf_frame_record* record,
    const char* role, uint32_t slot, uint32_t ordinal, uint32_t dispatch_reset, uint32_t preview)
{
    return static_cast<rsf::FrameCapture12*>(capture)->record(static_cast<ID3D12GraphicsCommandList*>(list),
        static_cast<ID3D12Resource*>(source), static_cast<D3D12_RESOURCE_STATES>(state), *record, role, slot,
        ordinal, dispatch_reset, preview);
}
extern "C" int rsf_sr12_platform_capture_event(void* capture, const rsf_frame_record* record, const char* name, uint64_t sequence,
    uint32_t reset, int32_t result)
{
    return static_cast<rsf::FrameCapture12*>(capture)->event(*record, name, sequence, reset != 0, result);
}
extern "C" int rsf_sr12_platform_capture_wants(void* capture, uint64_t frame) { return static_cast<rsf::FrameCapture12*>(capture)->wants(frame); }
extern "C" int rsf_sr12_platform_capture_force_reset(void* capture, uint64_t frame)
{
    return static_cast<rsf::FrameCapture12*>(capture)->force_sr_history_reset(frame);
}

extern "C" {
rsf_backend_result rsf_core_sr12_create_native(const rsf_sr12_setup*, rsf_sr12**);
uint32_t rsf_core_sr12_quality_supported(uint32_t, uint32_t);
rsf_backend_result rsf_core_sr12_create_planned_native(const rsf_sr12_setup*, rsf_sr12_plan_result*, rsf_sr12**);
rsf_backend_result rsf_core_sr12_plan(const rsf_sr12*, uint32_t*, uint32_t*);
rsf_backend_result rsf_core_sr12_evaluate(rsf_sr12*, void*, const rsf_game_render_pass*, uint32_t);
rsf_backend_result rsf_core_sr12_set_sharpening(rsf_sr12*, const rsf_sharpening_options*);
void rsf_core_sr12_destroy(rsf_sr12*);
rsf_backend_result rsf_core_sr12_present(rsf_sr12*, void*, uint32_t, uint32_t, uint32_t);
rsf_backend_result rsf_core_sr12_capture(rsf_sr12*, const char*, uint64_t, uint32_t);
rsf_backend_result rsf_core_sr12_capture_flush(rsf_sr12*, uint32_t);
rsf_backend_result rsf_core_sr12_capture_scene(rsf_sr12*, void*, const rsf_game_render_pass*, uint32_t);
}

// The public entries stay native so the Runtime DLL and every embedded consumer keep the same
// exported names and decorations.
extern "C" rsf_backend_result rsf_sr12_create(const rsf_sr12_setup* setup, rsf_sr12** out)
{
    return rsf_core_sr12_create_native(setup, out);
}
extern "C" uint32_t rsf_sr12_quality_supported(uint32_t backend, uint32_t quality)
{
    return rsf_core_sr12_quality_supported(backend, quality);
}
extern "C" rsf_backend_result rsf_sr12_create_planned(const rsf_sr12_setup* setup, rsf_sr12_plan_result* plan, rsf_sr12** out)
{
    return rsf_core_sr12_create_planned_native(setup, plan, out);
}
extern "C" rsf_backend_result rsf_sr12_plan(const rsf_sr12* self, uint32_t* width, uint32_t* height)
{
    return rsf_core_sr12_plan(self, width, height);
}
extern "C" rsf_backend_result rsf_sr12_evaluate(rsf_sr12* self, void* commands, const rsf_game_render_pass* pass, uint32_t slot)
{
    return rsf_core_sr12_evaluate(self, commands, pass, slot);
}
extern "C" rsf_backend_result rsf_sr12_set_sharpening(rsf_sr12* self, const rsf_sharpening_options* options)
{
    return rsf_core_sr12_set_sharpening(self, options);
}
extern "C" void rsf_sr12_destroy(rsf_sr12* self) { rsf_core_sr12_destroy(self); }
extern "C" rsf_backend_result rsf_sr12_present(rsf_sr12* self, void* swapchain, uint32_t sync_interval,
    uint32_t flags, uint32_t begin)
{
    return rsf_core_sr12_present(self, swapchain, sync_interval, flags, begin);
}
extern "C" rsf_backend_result rsf_sr12_capture(rsf_sr12* self, const char* directory, uint64_t first_frame, uint32_t frames)
{
    return rsf_core_sr12_capture(self, directory, first_frame, frames);
}
extern "C" rsf_backend_result rsf_sr12_capture_flush(rsf_sr12* self, uint32_t slot)
{
    return rsf_core_sr12_capture_flush(self, slot);
}
extern "C" rsf_backend_result rsf_sr12_capture_scene(rsf_sr12* self, void* commands, const rsf_game_render_pass* pass,
    uint32_t slot)
{
    return rsf_core_sr12_capture_scene(self, commands, pass, slot);
}
