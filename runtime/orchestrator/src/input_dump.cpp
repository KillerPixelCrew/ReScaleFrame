// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/version.h>
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
#include <sstream>
#include <string>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
struct Entry {
    std::string name, description;
    uint32_t width = 0, height = 0, format = 0, view = 0;
    float scale = 1;
    int32_t raw = RSF_TEXTURE_ERROR_INVALID_ARGUMENT, preview = RSF_TEXTURE_ERROR_INVALID_ARGUMENT;
    bool available = false;
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
};
Dump& state() { static Dump s; return s; }
void say(const std::string& message) { auto& s = state(); if (s.log) s.log(s.user, message.c_str()); }
bool matches(const rsf_game_render_pass* pass)
{
    const auto& id = state().identity;
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
        << ",\"source_frame_id\":" << s.identity.source_frame_id << ",\"session_id\":" << s.identity.session_id
        << ",\"native_frame\":" << s.identity.native_frame << ",\"view_key\":" << s.identity.view_key
        << ",\"family_key\":" << s.identity.family_key << ",\"scope_id\":" << s.identity.scope_id
        << ",\"viewport_key\":" << s.identity.viewport_key << ",\"backend\":" << s.backend
        << ",\"sr_result\":" << s.sr_result << ",\"matched_present\":" << (s.presented ? "true" : "false")
        << ",\"present_result\":" << s.present_result
        << ",\"constants\":" << (s.constants ? "\"sr_constants.json\"" : "null")
        << ",\"fg_constants\":" << (s.fg_constants ? "\"fg_constants.json\"" : "null")
        << ",\"preview_alpha\":\"opaque\",\"raw_encoding\":\"little-endian packed rows; original DXGI storage, subresource 0\""
        << ",\"preview_views\":{\"0\":\"stored RGB clamped to 0..1\",\"1\":\"UE encoded velocity decoded to NDC; zero gray, unwritten blue\","
        "\"2\":\"decoded motion: zero gray, R=X G=Y, blue marks invalid sentinel\",\"3\":\"linear HDR: Reinhard then sRGB\","
        "\"4\":\"device depth: log2(1+65535*z)/16; reversed Z near white, sky black\",\"5\":\"first channel grayscale\",\"6\":\"source alpha grayscale\","
        "\"7\":\"dense pixel displacement: zero gray, R=X G=Y, no unwritten sentinel\"},\n\"images\":[\n";
    bool first = true;
    for (const auto& e : s.entries) {
        if (!first) out << ",\n";
        first = false;
        out << "{\"name\":" << std::quoted(e.name) << ",\"description\":" << std::quoted(e.description)
            << ",\"status\":\"" << (!e.available ? "unavailable" : e.raw || e.preview ? "failed" : "captured")
            << "\",\"width\":" << e.width << ",\"height\":" << e.height << ",\"dxgi_format\":" << e.format
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
            e.width = desc.Width; e.height = desc.Height; e.format = uint32_t(desc.Format);
            // Colour previews of display UNORM surfaces retain their stored transfer function.
            if (view == RSF_DUMP_VIEW_HDR && desc.Format != DXGI_FORMAT_R16G16B16A16_FLOAT &&
                desc.Format != DXGI_FORMAT_R11G11B10_FLOAT) e.view = RSF_DUMP_VIEW_RAW;
            const std::string prefix = (s.bundle / name).string();
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
        "hudless_color", "hud_color", "hud_alpha", "hud_composed_color", "fg_hudless_color", "fg_ui", "final_color"};
    // Omitted stages remain explicit instead of silently reusing a different frame's textures.
    for (const char* name : expected) {
        if (std::none_of(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name; })) {
            Entry e; e.name = name; e.description = "Not produced at the matched handoff in this frame"; s.entries.push_back(std::move(e));
        }
    }
    const char* required[]{"sr_source_jittered", "sr_color", "sr_depth", "sr_motion", "sr_game_motion", "sr_motion_decoded", "sr_output",
        "fg_depth", "fg_motion", "hudless_color", "hud_color", "hud_alpha", "hud_composed_color", "fg_hudless_color", "final_color"};
    if (std::string(status) == "complete") for (const char* name : required) {
        if (std::none_of(s.entries.begin(), s.entries.end(), [&](const Entry& e) { return e.name == name && e.available && !e.raw && !e.preview; })) {
            status = "partial"; break;
        }
    }
    manifest(status);
    s.armed = s.in_sr = false;
    say("website dump " + std::string(status) + ": " + s.bundle.string());
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
    auto& s = state(); s.log = log; s.user = user;
    s.game = game ? game : "unknown"; s.executable_sha256 = executable_sha256 ? executable_sha256 : "";
    s.directory = directory ? std::filesystem::path(directory) / "website" : std::filesystem::path{};
    say("website input dump: F10 captures the next source frame as named opaque PNGs and raw buffers");
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_request(void) try
{
    auto& s = state();
    if (s.directory.empty() || s.armed) return;
    SYSTEMTIME time{}; GetLocalTime(&time);
    char name[100]{};
    std::snprintf(name, sizeof(name), "scene-%04u%02u%02u-%02u%02u%02u-%03u-%llu", time.wYear, time.wMonth,
        time.wDay, time.wHour, time.wMinute, time.wSecond, time.wMilliseconds,
        static_cast<unsigned long long>(GetTickCount64()));
    std::filesystem::create_directories(s.directory);
    s.bundle = s.directory / name;
    if (!std::filesystem::create_directory(s.bundle)) { say("website dump: could not create a unique directory"); return; }
    s.identity = {}; s.entries.clear(); s.constants = s.fg_constants = s.presented = false;
    s.backend = 0; s.sr_result = -1; s.present_result = 0;
    s.armed = true; s.started = GetTickCount64();
    manifest("waiting_for_sr");
    say("website dump armed: " + s.bundle.string());
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_texture(void* context, const char* name, const char* description,
    void* source, rsf_dump_view view, float scale) try
{ if (state().in_sr && name && description) texture(context, name, description, source, view, scale); }
catch (...) { failed(); }
extern "C" RSF_RUNTIME_API int rsf_input_dump_active(void)
{ return state().armed && state().in_sr; }
extern "C" RSF_RUNTIME_API void rsf_input_dump_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin) try
{
    auto& s = state();
    if (!s.armed || !pass || pass->struct_size < sizeof(*pass)) return;
    if (pass->role == RSF_GAME_RENDER_SR) {
        if (begin && !s.identity.source_frame_id && pass->source_frame_id && (pass->flags & RSF_GAME_RENDER_PRIMARY)) {
            s.identity = *pass;
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
            texture(context, "hud_alpha", "Native HUD raster alpha coverage", pass->ui_input, RSF_DUMP_VIEW_ALPHA, 1);
        } else texture(context, "hud_composed_color", "Native HUD composition output", pass->color_output_readable ? pass->color_output_readable : pass->color_output, RSF_DUMP_VIEW_HDR, 1);
    }
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE && !begin)
        texture(context, "fg_hudless_color", "Completed engine scene handed to FG before final Slate/window UI", pass->scene_surface, RSF_DUMP_VIEW_HDR, 1);
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
    if (!matches(pass) || !frame || !frame->record) return;
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
extern "C" RSF_RUNTIME_API void rsf_input_dump_present(void* chain, void* context) try
{
    auto& s = state();
    if (s.directory.empty()) return;
    if (s.armed) {
        rsf_game_render_pass window{}; window.struct_size = sizeof(window);
        if (rsf_native_window_match(chain, &window) && s.identity.source_frame_id &&
            window.source_frame_id == s.identity.source_frame_id && window.session_id == s.identity.session_id &&
            window.viewport_key == s.identity.viewport_key) {
            ComPtr<ID3D11Texture2D> buffer;
            if (SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetBuffer(0, IID_PPV_ARGS(&buffer))))
                texture(context, "final_color", "Final real frame with game HUD, before the RSF overlay and interpolation", buffer.Get(), RSF_DUMP_VIEW_HDR, 1);
            s.presented = true;
        } else if (GetTickCount64() - s.started > 15000) finish("incomplete_no_matched_frame");
    }
    DWORD process = 0; GetWindowThreadProcessId(GetForegroundWindow(), &process);
    const bool key = process == GetCurrentProcessId() && (GetAsyncKeyState(VK_F10) & 0x8000) != 0;
    if (key && !s.key_down) rsf_input_dump_request();
    s.key_down = key;
} catch (...) { failed(); }
extern "C" RSF_RUNTIME_API void rsf_input_dump_presented(void* chain, int32_t result) try
{
    auto& s = state(); if (!s.armed || !s.presented) return;
    rsf_game_render_pass window{}; window.struct_size = sizeof(window);
    if (!rsf_native_window_match(chain, &window) || window.source_frame_id != s.identity.source_frame_id ||
        window.session_id != s.identity.session_id || window.viewport_key != s.identity.viewport_key) return;
    s.present_result = result;
    const bool failed_image = std::any_of(s.entries.begin(), s.entries.end(), [](const Entry& e) { return e.available && (e.raw || e.preview); });
    finish(s.sr_result || FAILED(result) || failed_image || !s.constants ? "partial" : "complete");
} catch (...) { failed(); }
