// SPDX-License-Identifier: MIT
#include "input_dump_platform.h"
#include <rescaleframe/input_dump_d3d12.h>
#include "../../backends/common/frame_capture12.h"
#include <rescaleframe/sha256_file.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstring>
#include <filesystem>
#include <fstream>

namespace rsf { uint32_t texture_pixel_bytes(DXGI_FORMAT format); }

namespace {
using Microsoft::WRL::ComPtr;

// Also mirrored by the private Rust call table.
static_assert(sizeof(rsf_dump_clock) == 48 && sizeof(rsf_dump_surface) == 12 && sizeof(rsf_dump_encoding) == 8);
static_assert(sizeof(rsf_dump_pixels) == 40 && sizeof(rsf_texture_dump_options) == 40);
static_assert(sizeof(rsf_named_frame_dump) == 48 && sizeof(rsf_named_frame_dump_report) == 16);

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

uint32_t pixel_bytes(uint32_t format) { return rsf::texture_pixel_bytes(DXGI_FORMAT(format)); }

void encode_pixels(const rsf_dump_pixels* pixels, uint32_t row, uint32_t api, uint32_t native_format,
    const rsf_texture_dump_options* options, rsf_dump_encoding* result) try
{
    const auto* utf8 = reinterpret_cast<const char8_t*>(options->output_prefix_utf8);
    const std::u8string prefix(utf8);
    std::ofstream raw(std::filesystem::path(prefix + u8".bin"), std::ios::binary);
    for (uint32_t y = 0; y < pixels->height; ++y)
        raw.write(static_cast<const char*>(pixels->data) + size_t(y) * pixels->row_pitch, row);
    raw.close();
    result->raw = raw ? RSF_TEXTURE_OK : RSF_TEXTURE_ERROR_WRITE_FAILED;

    std::ofstream metadata(std::filesystem::path(prefix + u8"_raw.json"), std::ios::binary);
    metadata << "{\"width\":" << pixels->width << ",\"height\":" << pixels->height << ",\"format\":" << pixels->format
        << ",\"native_api\":" << api << ",\"native_format\":" << native_format << ",\"row_pitch\":" << row
        << ",\"source_row_pitch\":" << pixels->row_pitch << ",\"bytes\":" << uint64_t(row) * pixels->height << "}\n";
    metadata.close();
    if (!metadata) result->raw = RSF_TEXTURE_ERROR_WRITE_FAILED;
    result->preview = rsf_dump_pixels_png(pixels, options, nullptr);
} catch (...) { *result = {RSF_TEXTURE_ERROR_WRITE_FAILED, RSF_TEXTURE_ERROR_WRITE_FAILED}; }

void bind_platform()
{
    static const rsf_dump_platform platform{sizeof(platform), read_clock, read_local_time, executable_sha256,
        open_texture, backbuffer, encode_texture, pixel_bytes, encode_pixels, rsf_game_depth_domains_requested};
    static const bool bound = (rsf_core_dump_bind_platform(&platform), true);
    (void)bound;
}
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

struct rsf_input_dump12 { rsf::FrameCapture12 capture{false}; };
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump12_create(rsf_input_dump12** out) try
{
    if (!out) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = new rsf_input_dump12; return RSF_BACKEND_OK;
} catch (...) { if (out) *out = nullptr; return RSF_BACKEND_ERROR_INIT_FAILED; }
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump12_record(rsf_input_dump12* self, void* list, void* texture,
    const rsf_frame_record* record, const char* name, uint32_t resource_state, uint32_t slot, rsf_dump_view view, float scale) try
{
    if (!self || !list || !texture || !record || record->struct_size < sizeof(*record) || !name) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (record->abi_version != RSF_GAME_FRAME_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!rsf_input_dump_wants(record)) return RSF_BACKEND_OK;
    return self->capture.record(static_cast<ID3D12GraphicsCommandList*>(list), static_cast<ID3D12Resource*>(texture),
        D3D12_RESOURCE_STATES(resource_state), *record, name, slot, 0, UINT32_MAX, view, scale ? scale : 1) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump12_submitted(rsf_input_dump12* self, void* queue, uint32_t slot) try
{
    if (!self || !queue) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return self->capture.submitted(static_cast<ID3D12CommandQueue*>(queue), slot) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump12_poll(rsf_input_dump12* self) try
{ return !self ? RSF_BACKEND_ERROR_INVALID_ARGUMENT : self->capture.flush_ready() ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED; }
catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" RSF_RUNTIME_API void rsf_input_dump12_abandon(rsf_input_dump12* self, uint32_t slot)
{ if (self) self->capture.abandon(slot); }
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump12_destroy(rsf_input_dump12* self) try
{
    if (!self) return RSF_BACKEND_OK;
    if (!self->capture.flush_ready()) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (self->capture.pending()) return RSF_BACKEND_ERROR_NOT_READY;
    delete self; return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
