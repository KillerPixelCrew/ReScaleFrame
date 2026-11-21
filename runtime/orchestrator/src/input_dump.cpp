// SPDX-License-Identifier: MIT
#include "input_dump_platform.h"
#include <rescaleframe/capture_names.h>
#include <rescaleframe/input_dump_d3d12.h>
#include "../../backends/common/frame_capture12.h"
#include <rescaleframe/sha256_file.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstring>

namespace {
using Microsoft::WRL::ComPtr;

// Also mirrored by the private Rust call table.
static_assert(sizeof(rsf_dump_clock) == 48 && sizeof(rsf_dump_surface) == 12 && sizeof(rsf_dump_encoding) == 8);
static_assert(sizeof(rsf_dump_pixels) == 40 && sizeof(rsf_texture_dump_options) == 40);
static_assert(sizeof(rsf_named_frame_dump) == 48 && sizeof(rsf_named_frame_dump_report) == 16);
static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_BACKEND_ERROR_ABI_MISMATCH == -2 && RSF_BACKEND_ERROR_INIT_FAILED == -6 &&
    RSF_BACKEND_ERROR_NOT_READY == -8 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9);

void read_clock(rsf_dump_clock* out)
{
    DWORD foreground = 0;
    GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
    out->process = GetCurrentProcessId();
    out->tick = GetTickCount64();
    out->key_down = uint32_t(foreground == out->process && (GetAsyncKeyState(VK_F10) & 0x8000) != 0);
}

void read_local_time(rsf_dump_clock* out)
{
    SYSTEMTIME now{};
    GetLocalTime(&now);
    out->year = now.wYear;
    out->month = now.wMonth;
    out->day = now.wDay;
    out->hour = now.wHour;
    out->minute = now.wMinute;
    out->second = now.wSecond;
    out->millisecond = now.wMilliseconds;
}

void executable_sha256(const wchar_t* executable, char* out) try
{
    std::string hash;
    if (rsf::sha256_file(executable, hash)) std::memcpy(out, hash.c_str(), 65);
} catch (...) { out[0] = 0; }

void* open_texture(void* context, void* source, rsf_dump_surface* surface)
{
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11Texture2D> texture;
    if (!immediate || !source || immediate->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE ||
        FAILED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&texture)))) return nullptr;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    *surface = {desc.Width, desc.Height, uint32_t(desc.Format)};
    return texture.Detach();
}

void* backbuffer(void* chain)
{
    ComPtr<ID3D11Texture2D> buffer;
    if (!chain || FAILED(static_cast<IDXGISwapChain*>(chain)->GetBuffer(0, IID_PPV_ARGS(&buffer)))) return nullptr;
    return buffer.Detach();
}

void encode_texture(void* context, void* source, const rsf_texture_dump_options* options,
    uint32_t include_raw, rsf_dump_encoding* result) try
{
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11Device> device;
    immediate->GetDevice(&device);
    // Copy commands obey predication. Restore the exact predicate and value.
    ComPtr<ID3D11Predicate> predicate;
    BOOL predicate_value = FALSE;
    immediate->GetPredication(&predicate, &predicate_value);
    struct Restore {
        ID3D11DeviceContext* context;
        ID3D11Predicate* predicate;
        BOOL value;
        ~Restore() { context->SetPredication(predicate, value); }
    } restore{immediate, predicate.Get(), predicate_value};
    immediate->SetPredication(nullptr, FALSE);
    if (include_raw) result->raw = rsf_dump_texture_bytes(device.Get(), immediate, source, options);
    result->preview = rsf_dump_texture_png(device.Get(), immediate, source, options, nullptr);
} catch (...) { *result = {RSF_TEXTURE_ERROR_WRITE_FAILED, RSF_TEXTURE_ERROR_WRITE_FAILED}; }

int32_t preview_pixels(const rsf_dump_pixels* pixels, const rsf_texture_dump_options* options)
{
    return rsf_dump_pixels_png(pixels, options, nullptr);
}

void bind_platform()
{
    static const rsf_dump_platform platform{sizeof(platform), read_clock, read_local_time, executable_sha256,
        open_texture, backbuffer, encode_texture, preview_pixels, rsf_game_depth_domains_requested};
    static const bool bound = (rsf_core_dump_bind_platform(&platform), true);
    (void)bound;
}
}

extern "C" void rsf_core_dump_capture_sr_parameters(const rsf_frame_record*, uint32_t,
    float, float, float, uint32_t);

void rsf::capture_sr_parameters(const rsf_frame_record& record, uint32_t backend, float x,
    float y, float exposure, uint32_t reset)
{
    bind_platform();
    rsf_core_dump_capture_sr_parameters(&record, backend, x, y, exposure, reset);
}

// Forward the public symbols through this archive so embedded and DLL builds
// both pull in the same native leaves and emit the existing export directives.
#define RSF_DUMP_EXPORT(result, name, signature, arguments) \
    extern "C" result rsf_core_dump_##name signature; \
    extern "C" RSF_RUNTIME_API result rsf_input_dump_##name signature \
    { bind_platform(); return rsf_core_dump_##name arguments; }
RSF_DUMP_EXPORT(void, configure, (const char* dir, const char* game, const char* hash, rsf_dump_log_fn log, void* user),
    (dir, game, hash, log, user))
RSF_DUMP_EXPORT(void, request, (), ())
RSF_DUMP_EXPORT(void, cancel, (uint64_t session), (session))
RSF_DUMP_EXPORT(void, poll, (), ())
RSF_DUMP_EXPORT(int, begin_record, (const rsf_frame_record* record, uint32_t backend), (record, backend))
RSF_DUMP_EXPORT(int, wants, (const rsf_frame_record* record), (record))
RSF_DUMP_EXPORT(int, wants_frame, (uint64_t frame), (frame))
RSF_DUMP_EXPORT(int, current, (rsf_frame_record* record), (record))
RSF_DUMP_EXPORT(int, requested, (), ())
RSF_DUMP_EXPORT(uint64_t, reserve, (const rsf_frame_record* record, const char* name, const char* description, rsf_dump_view view, float scale),
    (record, name, description, view, scale))
RSF_DUMP_EXPORT(int, pixels, (uint64_t ticket, const char* name, const rsf_dump_pixels* pixels, uint32_t api, uint32_t format),
    (ticket, name, pixels, api, format))
RSF_DUMP_EXPORT(void, failed, (uint64_t ticket, const char* name, int32_t result), (ticket, name, result))
RSF_DUMP_EXPORT(void, complete, (const rsf_frame_record* record, int32_t result), (record, result))
RSF_DUMP_EXPORT(uint64_t, gpu_submission, (const rsf_frame_record* record), (record))
RSF_DUMP_EXPORT(void, gpu_completed, (uint64_t submission, uint32_t api, void* device), (submission, api, device))
RSF_DUMP_EXPORT(int, gpu_ready, (uint64_t ticket, const char* name, uint32_t api, void* device), (ticket, name, api, device))
RSF_DUMP_EXPORT(void, pass_metadata, (const rsf_game_render_pass* pass), (pass))
RSF_DUMP_EXPORT(void, result, (const rsf_frame_record* record, const char* stage, uint32_t backend, int32_t result),
    (record, stage, backend, result))
RSF_DUMP_EXPORT(void, sr_frame, (const rsf_sr_frame* frame, uint32_t backend, uint32_t input_sdr), (frame, backend, input_sdr))
RSF_DUMP_EXPORT(int, texture_record, (void* context, const rsf_frame_record* record, const char* name, const char* description,
    void* texture, rsf_dump_view view, float scale), (context, record, name, description, texture, view, scale))
RSF_DUMP_EXPORT(int, active, (), ())
RSF_DUMP_EXPORT(void, present, (void* swapchain, void* context, const rsf_game_render_pass* window), (swapchain, context, window))
RSF_DUMP_EXPORT(void, presented, (const rsf_game_render_pass* window, int32_t result), (window, result))
RSF_DUMP_EXPORT(void, pass, (void* context, const rsf_game_render_pass* pass, uint32_t begin), (context, pass, begin))
RSF_DUMP_EXPORT(void, texture, (void* context, const char* name, const char* description, void* texture, rsf_dump_view view, float scale),
    (context, name, description, texture, view, scale))
RSF_DUMP_EXPORT(void, sr, (void* context, const rsf_dlss_frame* frame, uint32_t backend), (context, frame, backend))
RSF_DUMP_EXPORT(void, sr_result, (int32_t result), (result))
RSF_DUMP_EXPORT(void, fg, (void* context, const rsf_game_render_pass* pass, const rsf_dlss_frame* frame), (context, pass, frame))
RSF_DUMP_EXPORT(void, fg_prepare, (const rsf_game_render_pass* pass, const rsf_fg_frame* frame, int32_t result), (pass, frame, result))
RSF_DUMP_EXPORT(int32_t, named_texture, (void* context, void* source, const rsf_named_frame_dump* request, rsf_named_frame_dump_report* report),
    (context, source, request, report))
#undef RSF_DUMP_EXPORT

extern "C" int32_t rsf_core_dump12_create(rsf_input_dump12**, const rsf_core_capture12_platform*);
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump12_create(rsf_input_dump12** out)
{
    return rsf_core_dump12_create(out, rsf::FrameCapture12::native_platform());
}

#define RSF_DUMP12_EXPORT(result, name, signature, arguments) \
    extern "C" result rsf_core_dump12_##name signature; \
    extern "C" RSF_RUNTIME_API result rsf_input_dump12_##name signature \
    { return rsf_core_dump12_##name arguments; }
RSF_DUMP12_EXPORT(int32_t, record, (rsf_input_dump12* self, void* list, void* texture, const rsf_frame_record* record,
    const char* name, uint32_t state, uint32_t slot, rsf_dump_view view, float scale),
    (self, list, texture, record, name, state, slot, view, scale))
RSF_DUMP12_EXPORT(int32_t, submitted, (rsf_input_dump12* self, void* queue, uint32_t slot), (self, queue, slot))
RSF_DUMP12_EXPORT(int32_t, poll, (rsf_input_dump12* self), (self))
RSF_DUMP12_EXPORT(void, abandon, (rsf_input_dump12* self, uint32_t slot), (self, slot))
RSF_DUMP12_EXPORT(int32_t, destroy, (rsf_input_dump12* self), (self))
#undef RSF_DUMP12_EXPORT
