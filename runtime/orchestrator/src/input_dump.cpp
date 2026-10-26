// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump.h>
#include <rescaleframe/input_dump_d3d12.h>
#include "../../backends/common/frame_capture12.h"
#include <rescaleframe/version.h>
#include <rescaleframe/sha256_file.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <limits>
#include <mutex>
#include <sstream>
#include <string>
#include <vector>

namespace rsf { uint32_t texture_pixel_bytes(DXGI_FORMAT format); }

namespace {
using Microsoft::WRL::ComPtr;
struct Entry {
    std::string name, description;
    uint32_t width = 0, height = 0, format = 0, view = 0;
    float scale = 1;
    int32_t raw = RSF_TEXTURE_ERROR_INVALID_ARGUMENT, preview = RSF_TEXTURE_ERROR_INVALID_ARGUMENT;
    bool available = false;
    bool pending = false;
    uint32_t native_api = 1, native_format = 0;
};
struct Dump {
    std::filesystem::path directory, bundle;
    std::string game, executable_sha256;
    rsf_dump_log_fn log = nullptr;
    void* user = nullptr;
    bool key_down = false, armed = false, in_sr = false;
    uint64_t started = 0;
    rsf_game_render_pass identity{};
    uint32_t backend = 0;
    int32_t sr_result = -1;
    bool constants = false, fg_constants = false, presented = false;
    int32_t present_result = 0;
    std::vector<Entry> entries;
    uint64_t ticket = 0;
    uint32_t pending = 0;
    bool generic = false, closed = false;
    bool native_frame_known = false;
    uint32_t gpu_api = 0;
    size_t gpu_ready_count = 0;
    void* gpu_device = nullptr;
    rsf_frame_record record{};
    std::recursive_mutex guard;
};
Dump& state() { static Dump s; return s; }
std::filesystem::path utf8_path(const char* value) { return std::filesystem::path(reinterpret_cast<const char8_t*>(value)); }
std::string utf8_string(const std::filesystem::path& value) {
    const auto bytes = value.u8string(); return {reinterpret_cast<const char*>(bytes.data()), bytes.size()};
}
void say(const std::string& message) { auto& s = state(); if (s.log) s.log(s.user, message.c_str()); }
bool matches(const rsf_game_render_pass* pass)
{
    const auto& s = state(); const auto& id = s.identity;
    if (s.generic) return s.armed && pass && pass->struct_size >= sizeof(*pass) && (pass->flags & RSF_GAME_RENDER_PRIMARY) &&
        pass->session_id == s.record.session_id && pass->source_frame_id == s.record.frame_id && uint32_t(pass->view_key) == s.record.view_id;
    return state().armed && id.source_frame_id && pass && pass->struct_size >= sizeof(*pass) &&
        (pass->flags & RSF_GAME_RENDER_PRIMARY) && id.session_id == pass->session_id &&
        id.source_frame_id == pass->source_frame_id && id.native_frame == pass->native_frame &&
        id.view_key == pass->view_key && id.family_key == pass->family_key;
}
// JSON never writes NaN or infinity, including unbounded far planes.
void number(std::ostream& out, float value)
{ if (std::isfinite(value)) out << std::setprecision(9) << value; else out << "null"; }
void array(std::ostream& out, const float* values, uint32_t count)
{ out << '['; for (uint32_t i = 0; i < count; ++i) { if (i) out << ','; number(out, values[i]); } out << ']'; }
void manifest(const char* status)
{
    auto& s = state();
    std::ofstream out(s.bundle / "manifest.json", std::ios::binary | std::ios::trunc);
    out << "{\n\"schema\":1,\"game\":" << std::quoted(s.game) << ",\"executable_sha256\":" << std::quoted(s.executable_sha256)
        << ",\"rsf_version\":\"" RSF_VERSION_STRING "\",\"status\":" << std::quoted(status)
        << ",\"frame_id\":" << s.identity.source_frame_id
        << ",\"source_frame_id\":" << (s.generic && (s.record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID) ? std::string("null") : std::to_string(s.identity.source_frame_id))
        << ",\"session_id\":" << s.identity.session_id
        << ",\"native_frame\":" << (s.native_frame_known ? std::to_string(s.identity.native_frame) : std::string("null")) << ",\"view_key\":" << s.identity.view_key
        << ",\"family_key\":" << s.identity.family_key << ",\"scope_id\":" << s.identity.scope_id
        << ",\"viewport_key\":" << s.identity.viewport_key << ",\"backend\":" << s.backend
        << ",\"sr_result\":" << s.sr_result << ",\"matched_present\":" << (s.presented ? "true" : "false")
        << ",\"present_result\":" << s.present_result
        << ",\"constants\":" << (s.constants ? s.generic ? "\"frame.json\"" : "\"sr_constants.json\"" : "null")
        << ",\"fg_constants\":" << (s.fg_constants ? "\"fg_constants.json\"" : "null")
        << ",\"preview_alpha\":\"opaque\",\"raw_encoding\":\"little-endian packed rows; original GPU storage, subresource 0; format is DXGI-compatible encoding, native_api/native_format preserve origin\""
        << ",\"preview_views\":{\"0\":\"stored RGB clamped to 0..1\",\"1\":\"UE encoded velocity decoded to NDC; zero gray, unwritten blue\","
        "\"2\":\"decoded motion: zero gray, R=X G=Y, blue marks invalid sentinel\",\"3\":\"linear HDR: Reinhard then sRGB\","
        "\"4\":\"device depth: log2(1+65535*z)/16; reversed Z near white, sky black\",\"5\":\"first channel grayscale\",\"6\":\"source alpha grayscale\","
        "\"7\":\"dense displacement: zero gray, R=X G=Y, units in image description, no unwritten sentinel\"},\n\"images\":[\n";
    bool first = true;
    for (const auto& e : s.entries) {
        if (!first) out << ",\n";
        first = false;
        out << "{\"name\":" << std::quoted(e.name) << ",\"description\":" << std::quoted(e.description)
            << ",\"status\":\"" << (e.pending ? "pending" : !e.available ? "unavailable" : e.raw || e.preview ? "failed" : "captured")
            << "\",\"width\":" << e.width << ",\"height\":" << e.height << ",\"dxgi_format\":" << e.format
            << ",\"native_api\":" << e.native_api << ",\"native_format\":" << e.native_format
            << ",\"preview_view\":" << e.view << ",\"preview_scale\":";
        number(out, e.scale);
        out << ",\"raw_result\":" << e.raw << ",\"preview_result\":" << e.preview << ",\"raw\":";
        if (!e.raw) out << std::quoted(e.name + ".bin"); else out << "null";
        out << ",\"raw_metadata\":";
        if (!e.raw) out << std::quoted(e.name + "_raw.json"); else out << "null";
        out << ",\"png\":";
        if (!e.preview) out << std::quoted(e.name + ".png"); else out << "null";
        out << '}';
    }
    out << "\n]}\n";
    out.close();
    if (!out) say("website dump: manifest write failed");
}
void texture(void* context, const char* name, const char* description, void* source, rsf_dump_view view, float scale)
{
    auto& s = state();
    if (!s.armed || !s.identity.source_frame_id || !context ||
        std::any_of(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name; })) return;
    Entry e; e.name = name; e.description = description; e.view = view; e.scale = scale; e.available = source != nullptr;
    if (source) {
        auto* immediate = static_cast<ID3D11DeviceContext*>(context);
        ComPtr<ID3D11Device> device; immediate->GetDevice(&device);
        ComPtr<ID3D11Texture2D> input;
        if (SUCCEEDED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&input)))) {
            D3D11_TEXTURE2D_DESC desc{}; input->GetDesc(&desc);
            e.width = desc.Width; e.height = desc.Height; e.format = e.native_format = uint32_t(desc.Format);
            // Colour previews of display UNORM surfaces retain their stored transfer function.
            if (view == RSF_DUMP_VIEW_HDR && desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT &&
                desc.Format != DXGI_FORMAT_R11G11B10_FLOAT) e.view = RSF_DUMP_VIEW_RAW;
            const std::string prefix = utf8_string(s.bundle / name);
            rsf_texture_dump_options options{sizeof(options), RSF_TEXTURE_DUMP_ABI_VERSION,
                prefix.c_str(), e.view, scale, s.log, s.user};
            say("website dump: " + e.name);
            // Copy commands obey predication. A rejected draw must not leave an empty readback.
            ComPtr<ID3D11Predicate> predicate; BOOL predicate_value = FALSE;
            immediate->GetPredication(&predicate, &predicate_value);
            struct Restore { ID3D11DeviceContext* context; ID3D11Predicate* predicate; BOOL value;
                ~Restore() { context->SetPredication(predicate, value); } } restore{immediate, predicate.Get(), predicate_value};
            immediate->SetPredication(nullptr, FALSE);
            e.raw = rsf_dump_texture_bytes(device.Get(), context, input.Get(), &options);
            e.preview = rsf_dump_texture_png(device.Get(), context, input.Get(), &options, nullptr);
        }
    }
    s.entries.push_back(std::move(e));
}
void finish(const char* status)
{
    auto& s = state();
    const char* expected[]{"sr_source_jittered", "sr_color", "sr_depth", "sr_scene_depth", "sr_motion", "sr_exposure", "sr_engine_exposure", "sr_backend_output",
        "sr_game_motion", "sr_motion_decoded", "sr_output", "sr_opaque_color", "sr_transparency_layer",
        "sr_reactive_mask", "sr_transparency_mask", "sr_bias_mask", "sr_cloud_depth", "fg_depth", "fg_motion",
        "hudless_color", "hud_color", "hud_alpha", "hud_composed_color", "fg_scene_color", "fg_ui", "final_color"};
    // Omitted stages remain explicit instead of silently reusing a different frame's textures.
    for (const char* name : expected) {
        if (std::none_of(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name; })) {
            Entry e; e.name = name; e.description = "Not produced at the matched handoff in this frame"; s.entries.push_back(std::move(e));
        }
    }
    const char* required[]{"sr_source_jittered", "sr_color", "sr_depth", "sr_motion", "sr_game_motion", "sr_motion_decoded", "sr_output",
        "fg_depth", "fg_motion", "hudless_color", "hud_color", "hud_alpha", "hud_composed_color", "fg_scene_color", "final_color"};
    if (!s.generic && std::string(status) == "complete") for (const char* name : required) {
        if (std::none_of(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name && e.available && !e.raw && !e.preview; })) {
            status = "partial"; break;
        }
    }
    manifest(status);
    s.armed = s.in_sr = false;
    say("website dump " + std::string(status) + ": " + utf8_string(s.bundle));
}
void failed()
{
    state().armed = state().in_sr = false;
    say("website dump stopped after an allocation or file failure; partial files retained");
}
}

extern "C" RSF_RUNTIME_API void rsf_input_dump_configure(const char* directory, const char* game,
    const char* executable_sha256, rsf_dump_log_fn log, void* user) try
{
    auto& s = state(); std::lock_guard lock(s.guard); s.log = log; s.user = user;
    if (s.armed) finish("cancelled_on_reconfigure");
    s.game = game ? game : ""; s.executable_sha256 = executable_sha256 ? executable_sha256 : "";
    s.directory = directory ? utf8_path(directory) / "website" : std::filesystem::path{};
    say("website input dump: F10 captures the next source frame as named opaque PNGs and raw buffers");
} catch (...) { failed(); }

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
extern "C" RSF_RUNTIME_API void rsf_input_dump_request(void) try
{
    auto& s = state();
    if (s.directory.empty()) rsf_input_dump_poll();
    std::lock_guard lock(s.guard);
    if (s.directory.empty() || s.armed) return;
    SYSTEMTIME time{}; GetLocalTime(&time);
    char name[100]{};
    std::snprintf(name, sizeof(name), "scene-%04u%02u%02u-%02u%02u%02u-%03u-%llu-%lu-%llu", time.wYear, time.wMonth,
        time.wDay, time.wHour, time.wMinute, time.wSecond, time.wMilliseconds,
        static_cast<unsigned long long>(GetTickCount64()), GetCurrentProcessId(), static_cast<unsigned long long>(s.ticket + 1));
    std::filesystem::create_directories(s.directory);
    s.bundle = s.directory / name;
    if (!std::filesystem::create_directory(s.bundle)) { say("website dump: could not create a unique directory"); return; }
    s.identity = {}; s.entries.clear(); s.constants = s.fg_constants = s.presented = false;
    s.backend = 0; s.sr_result = -1; s.present_result = 0;
    ++s.ticket; s.pending = 0; s.closed = s.generic = false; s.record = {};
    s.native_frame_known = false;
    s.gpu_api = 0; s.gpu_ready_count = 0; s.gpu_device = nullptr;
    s.armed = true; s.started = GetTickCount64();
    manifest("waiting_for_sr");
    say("website dump armed: " + utf8_string(s.bundle));
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_cancel(uint64_t session) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (s.armed && (!session || !s.identity.session_id || session == s.identity.session_id)) finish("cancelled");
} catch (...) { failed(); }

extern "C" RSF_RUNTIME_API void rsf_input_dump_poll(void) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (s.directory.empty()) {
        wchar_t executable[32768]{}, root[32768]{};
        if (!GetModuleFileNameW(nullptr, executable, 32768)) return;
        auto game = s.game.empty() ? std::filesystem::path(executable).stem() : utf8_path(s.game.c_str()).filename();
        if (game == L"." || game == L".." || game.empty()) game = std::filesystem::path(executable).stem();
        const DWORD custom = GetEnvironmentVariableW(L"RSF_DUMP_DIR", root, 32768);
        std::filesystem::path directory;
        if (custom && custom < 32768) directory = root;
        else {
            const DWORD size = GetEnvironmentVariableW(L"LOCALAPPDATA", root, 32768);
            if (!size || size >= 32768) return;
            directory = std::filesystem::path(root) / "ReScaleFrame" / game;
        }
        std::string hash; rsf::sha256_file(executable, hash);
        rsf_input_dump_configure(utf8_string(directory).c_str(), utf8_string(game).c_str(), hash.c_str(), s.log, s.user);
    }
    if (s.armed && GetTickCount64() - s.started > 30000) finish("incomplete_waiting_for_gpu_or_frame");
    DWORD process = 0; GetWindowThreadProcessId(GetForegroundWindow(), &process);
    const bool key = process == GetCurrentProcessId() && (GetAsyncKeyState(VK_F10) & 0x8000) != 0;
    if (key && !s.key_down) rsf_input_dump_request();
    s.key_down = key;
} catch (...) { failed(); }

extern "C" RSF_RUNTIME_API int rsf_input_dump_wants(const rsf_frame_record* record) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    return s.armed && s.generic && record && record->struct_size >= sizeof(*record) &&
        record->abi_version == RSF_GAME_FRAME_ABI_VERSION &&
        s.record.session_id == record->session_id && s.record.frame_id == record->frame_id &&
        s.record.view_id == record->view_id && s.record.resource_generation == record->resource_generation;
}
catch (...) { return 0; }
extern "C" RSF_RUNTIME_API int rsf_input_dump_wants_frame(uint64_t frame) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    return s.armed && s.generic && s.record.frame_id == frame;
}
catch (...) { return 0; }
extern "C" RSF_RUNTIME_API int rsf_input_dump_current(rsf_frame_record* record) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!s.armed || !s.generic || !record || record->struct_size < sizeof(*record)) return 0;
    *record = s.record; return 1;
}
catch (...) { return 0; }
extern "C" RSF_RUNTIME_API int rsf_input_dump_requested(void) try
{ auto& s = state(); std::lock_guard lock(s.guard); return s.armed; }
catch (...) { return 0; }
extern "C" RSF_RUNTIME_API int rsf_input_dump_begin_record(const rsf_frame_record* record, uint32_t backend) try
{
    rsf_input_dump_poll(); auto& s = state(); std::lock_guard lock(s.guard);
    if (!s.armed || !record || record->struct_size < sizeof(*record) ||
        record->abi_version != RSF_GAME_FRAME_ABI_VERSION || !record->session_id || !record->frame_id) return 0;
    if (s.identity.source_frame_id) return rsf_input_dump_wants(record);
    s.generic = true; s.record = *record; s.backend = backend; s.sr_result = 0;
    s.identity = {}; s.identity.struct_size = sizeof(s.identity); s.identity.session_id = record->session_id;
    s.identity.source_frame_id = record->frame_id; s.identity.view_key = record->view_id;
    s.identity.camera = record->camera;
    std::ofstream out(s.bundle / "frame.json", std::ios::binary);
    out << "{\"session_id\":" << record->session_id << ",\"frame_id\":" << record->frame_id
        << ",\"source_frame_id\":" << ((record->flags & RSF_FRAME_FLAG_AMBIGUOUS_ID) ? std::string("null") : std::to_string(record->frame_id))
        << ",\"view_id\":" << record->view_id << ",\"resource_generation\":" << record->resource_generation
        << ",\"flags\":" << record->flags << ",\"screen\":" << record->screen << ",\"backend\":" << backend
        << ",\"camera_available\":" << (record->camera.struct_size >= sizeof(record->camera) ? "true" : "false")
        << ",\"render\":[" << record->render_width << ',' << record->render_height << "],\"output\":[" << record->output_width << ',' << record->output_height
        << "],\"frame_time_ms\":"; number(out, record->frame_time_ms);
    out << ",\"jitter_pixels\":"; array(out, record->camera.jitter_pixels, 2);
    out << ",\"previous_jitter_pixels\":"; array(out, record->camera.previous_jitter_pixels, 2);
    out << ",\"view_to_clip\":"; array(out, record->camera.view_to_clip, 16);
    out << ",\"clip_to_view\":"; array(out, record->camera.clip_to_view, 16);
    out << ",\"view_to_world\":"; array(out, record->camera.view_to_world, 16);
    out << ",\"world_to_view\":"; array(out, record->camera.world_to_view, 16);
    out << ",\"clip_to_previous\":"; array(out, record->camera.clip_to_previous_clip, 16);
    out << ",\"near_plane\":"; number(out, record->camera.near_plane);
    out << ",\"far_plane\":"; number(out, record->camera.far_plane);
    out << ",\"depth_inverted\":" << record->camera.depth_inverted << ",\"motion_jittered\":" << record->camera.motion_jittered << "}\n";
    out.close(); s.constants = bool(out); manifest("capturing");
    return 1;
} catch (...) { failed(); return 0; }
extern "C" RSF_RUNTIME_API uint64_t rsf_input_dump_reserve(const rsf_frame_record* record, const char* name,
    const char* description, rsf_dump_view view, float scale) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!rsf_input_dump_wants(record) || !name || !*name || !description || view > RSF_DUMP_VIEW_DENSE_MOTION || !std::isfinite(scale) ||
        std::string(name).find_first_not_of("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-") != std::string::npos ||
        s.entries.size() >= 96 || std::any_of(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name; })) return 0;
    Entry entry; entry.name = name; entry.description = description; entry.view = view; entry.scale = scale ? scale : 1;
    entry.available = entry.pending = true; s.entries.push_back(std::move(entry)); ++s.pending;
    return s.ticket;
} catch (...) { failed(); return 0; }
namespace {
void finish_if_ready()
{
    auto& s = state(); if (!s.armed || !s.generic || !s.closed || s.pending) return;
    const bool error = std::any_of(s.entries.begin(), s.entries.end(), [](const Entry& e) { return e.available && (e.raw || e.preview); });
    const bool final = std::any_of(s.entries.begin(), s.entries.end(), [](const Entry& e) { return e.name == "final_color" && e.available && !e.raw && !e.preview; });
    finish(error || s.sr_result || FAILED(s.present_result) || !s.constants || !final ? "partial" : "complete");
}
}
extern "C" RSF_RUNTIME_API void rsf_input_dump_failed(uint64_t ticket, const char* name, int32_t result) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!s.armed || !ticket || ticket != s.ticket || !name) return;
    for (auto& entry : s.entries) if (entry.name == name && entry.pending) {
        entry.pending = false; entry.raw = entry.preview = result; --s.pending; break;
    }
    finish_if_ready();
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API int rsf_input_dump_pixels(uint64_t ticket, const char* name,
    const rsf_dump_pixels* pixels, uint32_t api, uint32_t native_format) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!s.armed || !ticket || ticket != s.ticket || !name || !pixels || pixels->struct_size < sizeof(*pixels)) return 0;
    auto found = std::find_if(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name && e.pending; });
    if (found == s.entries.end()) return 0;
    auto& entry = *found; entry.width = pixels->width; entry.height = pixels->height; entry.format = pixels->format;
    entry.native_api = api; entry.native_format = native_format;
    if (entry.view == RSF_DUMP_VIEW_HDR && pixels->format != DXGI_FORMAT_R16G16B16A16_FLOAT &&
        pixels->format != DXGI_FORMAT_R11G11B10_FLOAT && pixels->format != DXGI_FORMAT_R32G32B32A32_FLOAT) entry.view = RSF_DUMP_VIEW_RAW;
    const uint32_t bytes = rsf::texture_pixel_bytes(DXGI_FORMAT(pixels->format));
    const uint64_t row = uint64_t(bytes) * pixels->width;
    if (!bytes || !pixels->data || !pixels->height || !row || pixels->row_pitch < row ||
        pixels->data_size < uint64_t(pixels->row_pitch) * (pixels->height - 1) + row || row * pixels->height > 256ull * 1024 * 1024) {
        rsf_input_dump_failed(ticket, name, RSF_TEXTURE_ERROR_INVALID_ARGUMENT); return 0;
    }
    std::ofstream raw(s.bundle / (entry.name + ".bin"), std::ios::binary);
    for (uint32_t y = 0; y < pixels->height; ++y)
        raw.write(static_cast<const char*>(pixels->data) + size_t(y) * pixels->row_pitch, std::streamsize(row));
    raw.close(); entry.raw = raw ? RSF_TEXTURE_OK : RSF_TEXTURE_ERROR_WRITE_FAILED;
    std::ofstream metadata(s.bundle / (entry.name + "_raw.json"), std::ios::binary);
    metadata << "{\"width\":" << pixels->width << ",\"height\":" << pixels->height << ",\"format\":" << pixels->format
        << ",\"native_api\":" << api << ",\"native_format\":" << native_format << ",\"row_pitch\":" << row
        << ",\"source_row_pitch\":" << pixels->row_pitch << ",\"bytes\":" << row * pixels->height << "}\n";
    metadata.close(); if (!metadata) entry.raw = RSF_TEXTURE_ERROR_WRITE_FAILED;
    const auto prefix = utf8_string(s.bundle / entry.name);
    rsf_texture_dump_options options{sizeof(options), RSF_TEXTURE_DUMP_ABI_VERSION, prefix.c_str(), entry.view, entry.scale, s.log, s.user};
    entry.preview = rsf_dump_pixels_png(pixels, &options, nullptr);
    const bool okay = !entry.raw && !entry.preview;
    entry.pending = false; --s.pending; finish_if_ready(); return okay ? 1 : 0;
} catch (...) { rsf_input_dump_failed(ticket, name, RSF_TEXTURE_ERROR_WRITE_FAILED); return 0; }
extern "C" RSF_RUNTIME_API int rsf_input_dump_texture_record(void* context, const rsf_frame_record* record,
    const char* name, const char* description, void* source, rsf_dump_view view, float scale) try
{
    if (!context || !source || !rsf_input_dump_wants(record)) return 0;
    const auto ticket = rsf_input_dump_reserve(record, name, description, view, scale);
    if (!ticket) return 0;
    auto& s = state(); std::filesystem::path bundle;
    { std::lock_guard lock(s.guard); bundle = s.bundle; }
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    ComPtr<ID3D11Texture2D> texture; ComPtr<ID3D11Device> device;
    if (FAILED(static_cast<IUnknown*>(source)->QueryInterface(IID_PPV_ARGS(&texture))) || immediate->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) {
        rsf_input_dump_failed(ticket, name, RSF_TEXTURE_ERROR_INVALID_ARGUMENT); return 0;
    }
    immediate->GetDevice(&device); D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    if (view == RSF_DUMP_VIEW_HDR && desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT && desc.Format != DXGI_FORMAT_R11G11B10_FLOAT)
        view = RSF_DUMP_VIEW_RAW;
    const std::string prefix = utf8_string(bundle / name);
    rsf_texture_dump_options options{sizeof(options), RSF_TEXTURE_DUMP_ABI_VERSION, prefix.c_str(), view, scale, s.log, s.user};
    ComPtr<ID3D11Predicate> predicate; BOOL predicate_value = FALSE; immediate->GetPredication(&predicate, &predicate_value);
    struct Restore { ID3D11DeviceContext* context; ID3D11Predicate* predicate; BOOL value;
        ~Restore() { context->SetPredication(predicate, value); } } restore{immediate, predicate.Get(), predicate_value};
    immediate->SetPredication(nullptr, FALSE);
    const auto raw = rsf_dump_texture_bytes(device.Get(), immediate, texture.Get(), &options);
    const auto preview = rsf_dump_texture_png(device.Get(), immediate, texture.Get(), &options, nullptr);
    std::lock_guard lock(s.guard);
    if (!s.armed || s.ticket != ticket) return 0;
    for (auto& entry : s.entries) if (entry.name == name && entry.pending) {
        entry.width = desc.Width; entry.height = desc.Height; entry.format = entry.native_format = uint32_t(desc.Format);
        entry.native_api = 1; entry.view = view; entry.raw = raw; entry.preview = preview; entry.pending = false; --s.pending; break;
    }
    finish_if_ready(); return !raw && !preview ? 1 : 0;
} catch (...) { failed(); return 0; }
extern "C" RSF_RUNTIME_API void rsf_input_dump_complete(const rsf_frame_record* record, int32_t result) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!rsf_input_dump_wants(record)) return;
    s.closed = s.presented = true; s.present_result = result; finish_if_ready();
    if (s.armed) manifest("waiting_for_gpu");
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API uint64_t rsf_input_dump_gpu_submission(const rsf_frame_record* record) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    return rsf_input_dump_wants(record) ? (s.ticket << 8) | uint64_t(s.entries.size()) : 0;
} catch (...) { return 0; }
extern "C" RSF_RUNTIME_API void rsf_input_dump_gpu_completed(uint64_t submission, uint32_t api, void* device) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    const size_t count = size_t(submission & 255u);
    if (!s.armed || !device || !submission || (submission >> 8) != s.ticket || count > s.entries.size()) return;
    if (s.gpu_api == api && s.gpu_device == device) s.gpu_ready_count = (std::max)(s.gpu_ready_count, count);
    else { s.gpu_api = api; s.gpu_device = device; s.gpu_ready_count = count; }
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API int rsf_input_dump_gpu_ready(uint64_t ticket, const char* name, uint32_t api, void* device) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!s.armed || !ticket || ticket != s.ticket || !name || s.gpu_api != api || s.gpu_device != device) return 0;
    for (size_t i = 0; i < s.gpu_ready_count; ++i) if (s.entries[i].name == name) return 1;
    return 0;
} catch (...) { return 0; }
extern "C" RSF_RUNTIME_API void rsf_input_dump_texture(void* context, const char* name, const char* description,
    void* source, rsf_dump_view view, float scale) try
{ if (state().in_sr && name && description) texture(context, name, description, source, view, scale); }
catch (...) { failed(); }
extern "C" RSF_RUNTIME_API int rsf_input_dump_active(void)
{ return state().armed && state().in_sr; }
extern "C" RSF_RUNTIME_API void rsf_input_dump_pass_metadata(const rsf_game_render_pass* pass) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!s.armed || !s.generic || !pass || pass->struct_size < sizeof(*pass) || pass->session_id != s.record.session_id ||
        (pass->source_frame_id ? pass->source_frame_id : pass->native_frame) != s.record.frame_id) return;
    s.identity.native_frame = pass->native_frame; s.native_frame_known = true;
    s.identity.view_key = pass->view_key; s.identity.family_key = pass->family_key;
    s.identity.scope_id = pass->scope_id; s.identity.viewport_key = pass->viewport_key;
    std::ofstream out(s.bundle / "engine_pass.json", std::ios::binary);
    out << "{\"session_id\":" << pass->session_id << ",\"source_frame_id\":" << pass->source_frame_id
        << ",\"native_frame\":" << pass->native_frame << ",\"view_key\":" << pass->view_key << ",\"history_key\":" << pass->history_key
        << ",\"scope_id\":" << pass->scope_id << ",\"camera_valid\":" << pass->camera_valid << ",\"flags\":" << pass->flags
        << ",\"motion_camera_included\":" << pass->motion_camera_included << ",\"motion_to_uv\":";
    array(out, pass->motion_to_uv, 2);
    out << ",\"render_rect\":[" << pass->render_rect[0] << ',' << pass->render_rect[1] << ',' << pass->render_rect[2] << ',' << pass->render_rect[3]
        << "],\"output_rect\":[" << pass->output_rect[0] << ',' << pass->output_rect[1] << ',' << pass->output_rect[2] << ',' << pass->output_rect[3] << "]}\n";
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_result(const rsf_frame_record* record, const char* stage, uint32_t backend, int32_t result) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!rsf_input_dump_wants(record) || !stage || (std::string(stage) != "sr" && std::string(stage) != "fg")) return;
    if (std::string(stage) == "sr") s.sr_result = result;
    std::ofstream out(s.bundle / (std::string(stage) + "_status.json"), std::ios::binary);
    out << "{\"session_id\":" << record->session_id << ",\"source_frame_id\":" << record->frame_id << ",\"view_id\":" << record->view_id
        << ",\"backend\":" << backend << ",\"result\":" << result << "}\n";
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_sr_frame(const rsf_sr_frame* frame, uint32_t backend, uint32_t input_sdr) try
{
    auto& s = state(); std::lock_guard lock(s.guard);
    if (!frame || frame->struct_size < sizeof(*frame) || !rsf_input_dump_wants(frame->record)) return;
    std::ofstream out(s.bundle / "sr_parameters.json", std::ios::binary);
    out << "{\"source_frame_id\":" << frame->record->frame_id << ",\"backend\":" << backend << ",\"input_sdr\":" << input_sdr
        << ",\"reset\":" << frame->reset << ",\"jitter_pixels\":["; number(out, frame->jitter_x); out << ','; number(out, frame->jitter_y);
    out << "],\"motion_scale\":["; number(out, frame->motion_scale_x); out << ','; number(out, frame->motion_scale_y);
    out << "],\"pre_exposure\":"; number(out, frame->pre_exposure);
    out << ",\"view_space_to_meters\":"; number(out, frame->view_space_to_meters); out << "}\n";
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin) try
{
    auto& s = state();
    if (!s.armed || !pass || pass->struct_size < sizeof(*pass)) return;
    if (pass->role == RSF_GAME_RENDER_SR) {
        if (begin && !s.identity.source_frame_id && pass->source_frame_id && (pass->flags & RSF_GAME_RENDER_PRIMARY)) {
            s.identity = *pass;
            s.native_frame_known = true;
            // Only copied scalar identity is kept. Leased resource pointers are never retained.
            s.identity.color_input = s.identity.color_output = s.identity.depth = s.identity.motion = s.identity.exposure = nullptr;
            s.identity.color_output_readable = s.identity.ui_input = s.identity.scene_surface = s.identity.sampled_texture = s.identity.swapchain = nullptr;
            manifest("capturing");
        }
        s.in_sr = begin && matches(pass);
    }
    if (!matches(pass)) return;
    if (pass->role == RSF_GAME_RENDER_UI_COMPOSITE) {
        if (begin) {
            texture(context, "hudless_color", "Tonemapped scene immediately before native HUD composition", pass->color_input, RSF_DUMP_VIEW_HDR, 1);
            texture(context, "hud_color", "Native HUD raster RGB; source alpha is preserved in raw data", pass->ui_input, RSF_DUMP_VIEW_RAW, 1);
            texture(context, "hud_alpha", "Stored native HUD alpha; translucent/additive producers can leave it zero", pass->ui_input, RSF_DUMP_VIEW_ALPHA, 1);
        } else texture(context, "hud_composed_color", "Native HUD composition output", pass->color_output_readable ? pass->color_output_readable : pass->color_output, RSF_DUMP_VIEW_HDR, 1);
    }
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE && !begin)
        texture(context, "fg_scene_color", "Actual completed FG scene input; game UI can already be composed here", pass->scene_surface, RSF_DUMP_VIEW_HDR, 1);
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_sr(void* context, const rsf_dlss_frame* frame, uint32_t backend) try
{
    auto& s = state(); if (!s.in_sr || !s.armed || !frame) return;
    s.backend = backend;
    texture(context, "sr_color", frame->color_encoded ? "SR backend colour: bounded display-range transport" : "SR backend colour: jittered linear HDR", frame->color_in,
        frame->color_encoded ? RSF_DUMP_VIEW_RAW : RSF_DUMP_VIEW_HDR, 1);
    texture(context, "sr_depth", "SR backend device depth", frame->depth, RSF_DUMP_VIEW_DEPTH, 1);
    texture(context, "sr_motion", "SR backend motion; scales and camera inclusion in sr_constants.json", frame->motion,
        frame->camera_motion_included ? RSF_DUMP_VIEW_DENSE_MOTION : RSF_DUMP_VIEW_DECODED_MOTION,
        frame->camera_motion_included ? 1.0f / 32 : 8.0f);
    texture(context, "sr_exposure", "Exposure texture supplied to the SR backend; absent when colour transport uses auto exposure", frame->exposure, RSF_DUMP_VIEW_SCALAR, 1);
    std::ofstream out(s.bundle / "sr_constants.json", std::ios::binary | std::ios::trunc);
    out << "{\"render\":[" << frame->render_width << ',' << frame->render_height << "],\"output\":[" << frame->output_width << ',' << frame->output_height
        << "],\"backend\":" << backend << ",\"quality\":" << frame->quality << ",\"preset\":" << frame->preset
        << ",\"color_encoded\":" << frame->color_encoded << ",\"depth_inverted\":" << frame->depth_inverted
        << ",\"camera_motion_included\":" << frame->camera_motion_included << ",\"reset\":" << frame->reset
        << ",\"jitter_pixels\":["; number(out, frame->jitter_x); out << ','; number(out, frame->jitter_y);
    out << "],\"motion_scale\":["; number(out, frame->motion_scale_x); out << ','; number(out, frame->motion_scale_y);
    out << "],\"motion_invalid_value\":"; number(out, frame->motion_invalid_value);
    out << ",\"view_to_clip\":"; array(out, frame->camera_view_to_clip, 16);
    out << ",\"clip_to_view\":"; array(out, frame->clip_to_camera_view, 16);
    out << ",\"clip_to_previous\":"; array(out, frame->clip_to_prev_clip, 16);
    out << ",\"previous_to_clip\":"; array(out, frame->prev_clip_to_clip, 16);
    out << ",\"position\":"; array(out, frame->camera_position, 3);
    out << ",\"up\":"; array(out, frame->camera_up, 3);
    out << ",\"right\":"; array(out, frame->camera_right, 3);
    out << ",\"forward\":"; array(out, frame->camera_forward, 3);
    out << ",\"near_plane\":"; number(out, frame->near_plane);
    out << ",\"far_plane\":"; number(out, frame->far_plane);
    out << ",\"vertical_fov_radians\":"; number(out, frame->vertical_fov);
    out << ",\"aspect_ratio\":"; number(out, frame->aspect_ratio);
    out << ",\"previous_jitter_pixels\":"; array(out, s.identity.camera.previous_jitter_pixels, 2);
    out << ",\"view_to_world\":"; array(out, s.identity.camera.view_to_world, 16);
    out << ",\"world_to_view\":"; array(out, s.identity.camera.world_to_view, 16);
    out << ",\"motion_jittered\":" << s.identity.camera.motion_jittered << ",\"frame_time_seconds\":";
    number(out, s.identity.camera.frame_time_seconds);
    out << ",\"render_rect\":[" << s.identity.render_rect[0] << ',' << s.identity.render_rect[1] << ',' << s.identity.render_rect[2] << ',' << s.identity.render_rect[3]
        << "],\"output_rect\":[" << s.identity.output_rect[0] << ',' << s.identity.output_rect[1] << ',' << s.identity.output_rect[2] << ',' << s.identity.output_rect[3] << "]}\n";
    out.close(); s.constants = bool(out);
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_sr_result(int32_t result)
{ if (state().in_sr) state().sr_result = result; }
extern "C" RSF_RUNTIME_API void rsf_input_dump_fg(void* context, const rsf_game_render_pass* pass, const rsf_dlss_frame* frame) try
{
    if (!matches(pass) || !frame) return;
    texture(context, "fg_depth", "Normalized depth offered to FG for this source frame", frame->depth, RSF_DUMP_VIEW_DEPTH, 1);
    texture(context, "fg_motion", "Dense previous-minus-current pixel motion offered to FG", frame->motion, RSF_DUMP_VIEW_DENSE_MOTION, 1.0f / 32);
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_fg_prepare(const rsf_game_render_pass* pass, const rsf_fg_frame* frame, int32_t result) try
{
    if (!frame || !frame->record || (state().generic ? !rsf_input_dump_wants(frame->record) : !matches(pass))) return;
    auto& s = state(); const auto& record = *frame->record;
    std::ofstream out(s.bundle / "fg_constants.json", std::ios::binary | std::ios::trunc);
    out << "{\"source_frame_id\":" << record.frame_id << ",\"session_id\":" << record.session_id
        << ",\"prepare_result\":" << result << ",\"resource_generation\":" << record.resource_generation
        << ",\"flags\":" << record.flags << ",\"screen\":" << record.screen << ",\"input_qpc\":" << record.input_qpc
        << ",\"interpolate\":" << frame->interpolate << ",\"frame_time_ms\":"; number(out, record.frame_time_ms);
    out << ",\"render\":[" << record.render_width << ',' << record.render_height << "],\"output\":[" << record.output_width << ',' << record.output_height
        << "],\"motion_scale\":["; number(out, frame->motion_scale_x); out << ','; number(out, frame->motion_scale_y);
    out << "],\"jitter_pixels\":"; array(out, record.camera.jitter_pixels, 2);
    out << ",\"previous_jitter_pixels\":"; array(out, record.camera.previous_jitter_pixels, 2);
    out << ",\"view_to_clip\":"; array(out, record.camera.view_to_clip, 16);
    out << ",\"clip_to_view\":"; array(out, record.camera.clip_to_view, 16);
    out << ",\"view_to_world\":"; array(out, record.camera.view_to_world, 16);
    out << ",\"world_to_view\":"; array(out, record.camera.world_to_view, 16);
    out << ",\"clip_to_previous\":"; array(out, record.camera.clip_to_previous_clip, 16);
    out << ",\"near_plane\":"; number(out, record.camera.near_plane);
    out << ",\"far_plane\":"; number(out, record.camera.far_plane);
    out << ",\"vertical_fov_radians\":"; number(out, record.camera.vertical_fov_radians);
    out << ",\"depth_inverted\":" << record.camera.depth_inverted << ",\"motion_jittered\":" << record.camera.motion_jittered << "}\n";
    out.close(); s.fg_constants = bool(out);
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_present(void* chain, void* context, const rsf_game_render_pass* pass) try
{
    auto& s = state();
    if (s.directory.empty()) return;
    if (s.armed) {
        const auto window = pass ? *pass : rsf_game_render_pass{};
        if (pass && s.identity.source_frame_id &&
            window.source_frame_id == s.identity.source_frame_id && window.session_id == s.identity.session_id &&
            (s.generic || window.viewport_key == s.identity.viewport_key)) {
            ComPtr<ID3D11Texture2D> buffer;
            if (SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetBuffer(0, IID_PPV_ARGS(&buffer))))
                texture(context, "final_color", "Final real frame with game HUD, before the RSF overlay and interpolation", buffer.Get(), RSF_DUMP_VIEW_HDR, 1);
            s.presented = true;
        } else if (GetTickCount64() - s.started > 15000) finish("incomplete_no_matched_frame");
    }
    rsf_input_dump_poll();
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_presented(const rsf_game_render_pass* pass, int32_t result) try
{
    auto& s = state(); if (!s.armed || !s.presented) return;
    const auto window = pass ? *pass : rsf_game_render_pass{};
    if (!pass || window.source_frame_id != s.identity.source_frame_id ||
        window.session_id != s.identity.session_id || (!s.generic && window.viewport_key != s.identity.viewport_key)) return;
    if (s.generic) { rsf_input_dump_complete(&s.record, result); return; }
    s.present_result = result;
    const bool failed_image = std::any_of(s.entries.begin(), s.entries.end(), [](const Entry& e) { return e.available && (e.raw || e.preview); });
    finish(s.sr_result || FAILED(result) || failed_image || !s.constants ? "partial" : "complete");
} catch (...) { failed(); }
