// SPDX-License-Identifier: GPL-3.0-only
// Latency and Streamline entry points: NVAPI Reflex through nvapi_QueryInterface, NvLowLatencyVk,
// Streamline (sl.interposer and the feature functions it hands out) and AMD's Anti-Lag 2 and AGS
// loaders.
//
// Layout sources:
//   NVAPI Reflex  nvapi.h NV_SET_SLEEP_MODE_PARAMS_V1, NV_LATENCY_MARKER_PARAMS_V1 (version u32,
//                 frameID u64 at 8, markerType u32 at 16); the SDK header is not in the tree, so
//                 these offsets are recorded as assumed.
//   Streamline    vendor/streamline/include (sl_core_types.h, sl_consts.h, sl_reflex.h, sl_pcl.h,
//                 sl_dlss.h, sl_dlss_g.h). tests/vendor_census_layouts.cpp checks them.
#include <iterator>

#include "wrap_generic.h"

namespace rsf_census {

namespace {

// ---- NVAPI Reflex -----------------------------------------------------------------------------
constexpr uint32_t kIdSetSleepMode = 0xAC1CA9E0;
constexpr uint32_t kIdSleep = 0x852CD1D2;
constexpr uint32_t kIdSetLatencyMarker = 0xD9984C05;
constexpr uint32_t kIdGetLatency = 0x1A587F9C;
constexpr uint32_t kIdD3D12SetAsyncFrameMarker = 0x13C98F73;

Fn reflex_fn(uint32_t id) noexcept {
    switch (id) {
    case kIdSetSleepMode: return Fn::NvapiSetSleepMode;
    case kIdSleep: return Fn::NvapiSleep;
    case kIdSetLatencyMarker: return Fn::NvapiSetLatencyMarker;
    case kIdGetLatency: return Fn::NvapiGetLatency;
    case kIdD3D12SetAsyncFrameMarker: return Fn::NvapiD3D12SetAsyncFrameMarker;
    default: return Fn::Count;
    }
}

const char* nv_marker_name(uint32_t t) noexcept {
    static const char* const kNames[] = {"SIMULATION_START", "SIMULATION_END", "RENDERSUBMIT_START",
                                         "RENDERSUBMIT_END", "PRESENT_START", "PRESENT_END",
                                         "INPUT_SAMPLE", "TRIGGER_FLASH", "PC_LATENCY_PING",
                                         "OUT_OF_BAND_RENDERSUBMIT_START", "OUT_OF_BAND_RENDERSUBMIT_END",
                                         "OUT_OF_BAND_PRESENT_START", "OUT_OF_BAND_PRESENT_END"};
    return t < std::size(kNames) ? kNames[t] : "other";
}

void* nvapi_query_interface(uint32_t id) noexcept {
    using Target = void* (*)(uint32_t);
    auto o = reinterpret_cast<Target>(orig<Fn::NvapiQueryInterface>());
    Call c(Fn::NvapiQueryInterface, _ReturnAddress());
    void* p = o ? o(id) : nullptr;
    const Fn f = reflex_fn(id);
    if (c.full()) {
        c.json().hex("id", id);
        c.json().hex("result", reinterpret_cast<uint64_t>(p));
        if (f != Fn::Count) c.json().str("reflex_entry", kFns[idx(f)].id);
    }
    c.finish(reinterpret_cast<uint64_t>(p));
    // Hand out a recording thunk for the Reflex entries; everything else passes through.
    return (f != Fn::Count && p) ? bind_thunk(f, p) : p;
}

void dec_nv_sleep_mode(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    uint8_t raw[40]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), raw, sizeof(raw))) {
        uint32_t version, interval;
        std::memcpy(&version, raw, 4);
        std::memcpy(&interval, raw + 8, 4);
        j.hex("version", version);
        j.boolean("low_latency_mode", raw[4] != 0);
        j.boolean("low_latency_boost", raw[5] != 0);
        j.u64("minimum_interval_us", interval);
        j.boolean("use_markers_to_optimize", raw[12] != 0);
        j.str("layout", "assumed: mode@4 boost@5 interval@8 markers@12");
    } else {
        j.boolean("params_unreadable", true);
    }
}

void dec_nv_sleep(Json& j, const Q* a) noexcept { j.hex("device", a[0]); }

void dec_nv_marker(Json& j, const Q* a) noexcept {
    j.hex("device_or_queue", a[0]);
    uint8_t raw[24]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), raw, sizeof(raw))) {
        uint64_t frame;
        uint32_t type, version;
        std::memcpy(&version, raw, 4);
        std::memcpy(&frame, raw + 8, 8);
        std::memcpy(&type, raw + 16, 4);
        j.hex("version", version);
        j.u64("frame_id", frame);
        j.u64("marker_type", type);
        j.str("marker", nv_marker_name(type));
        j.str("layout", "assumed: frameID@8 markerType@16");
    } else {
        j.boolean("params_unreadable", true);
    }
}

void post_nv_get_latency(Json& j, const Q* a, Q) noexcept {
    j.hex("device", a[0]);
    uint8_t raw[16]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), raw, sizeof(raw))) {
        uint64_t first;
        std::memcpy(&first, raw + 8, 8);
        j.boolean("latency_report_present", first != 0);
        j.u64("first_report_frame_id", first);
    }
}

// ---- NvLowLatencyVk ---------------------------------------------------------------------------
void dec_nvll_marker(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    uint8_t raw[16]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), raw, sizeof(raw))) {
        uint64_t frame;
        uint32_t type;
        std::memcpy(&frame, raw, 8);
        std::memcpy(&type, raw + 8, 4);
        j.u64("frame_id", frame);
        j.u64("marker_type", type);
        j.str("marker", nv_marker_name(type));
        j.str("layout", "assumed: frameID@0 markerType@8");
    }
}
void dec_nvll_mode(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    uint8_t raw[16]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), raw, sizeof(raw))) {
        j.bytes_hex("params_first_16_bytes", raw, sizeof(raw));
    }
}
void dec_nvll_sleep(Json& j, const Q* a) noexcept {
    j.hex("device", a[0]);
    j.u64("signal_value", a[1]);
}

// ---- Streamline -------------------------------------------------------------------------------
constexpr size_t kSlHeader = 32; // sl::BaseStructure: next, structType GUID, structVersion

const char* sl_feature_name(uint32_t f) noexcept {
    switch (f) {
    case 0: return "DLSS";
    case 2: return "NIS";
    case 3: return "Reflex";
    case 4: return "PCL";
    case 5: return "DeepDVC";
    case 6: return "Latewarp";
    case 1000: return "DLSS_G";
    case 1001: return "DLSS_RR";
    case 1002: return "NvPerf";
    case 1003: return "DirectSR";
    case 1004: return "DLSS_NR";
    case 9999: return "ImGUI";
    default: return "other";
    }
}

const char* sl_buffer_name(uint32_t t) noexcept {
    static const char* const kNames[] = {
        "Depth", "MotionVectors", "HUDLessColor", "ScalingInputColor", "ScalingOutputColor", "Normals",
        "Roughness", "Albedo", "SpecularAlbedo", "IndirectAlbedo", "SpecularMotionVectors",
        "DisocclusionMask", "Emissive", "Exposure", "NormalRoughness", "DiffuseHitNoisy",
        "DiffuseHitDenoised", "SpecularHitNoisy", "SpecularHitDenoised", "ShadowNoisy", "ShadowDenoised",
        "AmbientOcclusionNoisy", "AmbientOcclusionDenoised", "UIColorAndAlpha", "ShadowHint",
        "ReflectionHint", "ParticleHint", "TransparencyHint", "AnimatedTextureHint",
        "BiasCurrentColorHint", "RaytracingDistance", "ReflectionMotionVectors", "Position",
        "InvalidDepthMotionHint", "Alpha", "OpaqueColor", "ReactiveMaskHint",
        "TransparencyAndCompositionMaskHint", "ReflectedAlbedo", "ColorBeforeParticles"};
    return t < std::size(kNames) ? kNames[t] : "other";
}

uint32_t sl_viewport(const void* vp) noexcept {
    uint32_t v = 0xFFFFFFFFu;
    if (plausible_ptr(vp)) rd(static_cast<const uint8_t*>(vp) + kSlHeader, v);
    return v;
}

// sl::FrameToken is polymorphic: its first virtual function is `operator uint32_t() const`.
bool sl_frame_index(const void* token, uint32_t& out) noexcept {
    auto f = reinterpret_cast<uint32_t (*)(const void*)>(vtable_slot(token, 0));
    if (!f) return false;
    return guard([&] { out = f(token); });
}

void put_sl_frame(Json& j, const char* key, const void* token) noexcept {
    uint32_t i;
    if (sl_frame_index(token, i)) {
        j.u64(key, i);
    } else {
        j.hex("frame_token", reinterpret_cast<uint64_t>(token));
    }
}

void put_sl_tags(Json& j, const void* tags, uint32_t n) noexcept {
    // sl::ResourceTag: header, Resource* at 32, BufferType at 40, lifecycle at 44, Extent at 48.
    constexpr size_t kTagSize = 64;
    j.arr("tags");
    for (uint32_t i = 0; i < n && i < 16; ++i) {
        uint8_t t[kTagSize];
        if (!plausible_ptr(tags) || !safe_read(static_cast<const uint8_t*>(tags) + i * kTagSize, t, kTagSize)) break;
        void* res;
        uint32_t type, life, ext[4];
        std::memcpy(&res, t + 32, 8);
        std::memcpy(&type, t + 40, 4);
        std::memcpy(&life, t + 44, 4);
        std::memcpy(ext, t + 48, 16);
        j.obj();
        j.u64("buffer_type", type);
        j.str("buffer", sl_buffer_name(type));
        j.u64("lifecycle", life);
        j.u64("extent_top", ext[0]);
        j.u64("extent_left", ext[1]);
        j.u64("extent_width", ext[2]);
        j.u64("extent_height", ext[3]);
        if (res && plausible_ptr(res)) {
            // sl::Resource: header, ResourceType (char) at 32, native at 40, memory at 48, view at 56,
            // state at 64, width at 68, height at 72, nativeFormat at 76, mips at 80, layers at 84.
            uint8_t r[96];
            if (safe_read(res, r, sizeof(r))) {
                void* native;
                uint32_t state, w, h, fmt;
                std::memcpy(&native, r + 40, 8);
                std::memcpy(&state, r + 64, 4);
                std::memcpy(&w, r + 68, 4);
                std::memcpy(&h, r + 72, 4);
                std::memcpy(&fmt, r + 76, 4);
                j.u64("resource_type", r[32]);
                j.hex("state", state);
                j.u64("width", w);
                j.u64("height", h);
                j.u64("native_format", fmt);
                if (r[32] == 0 && native) json_d3d12_resource(j, "native", native);
                else json_ptr_or_null(j, "native_ptr", native);
            }
        }
        j.end_obj();
    }
    j.end_arr();
}

void dec_sl_init(Json& j, const Q* a) noexcept {
    uint8_t p[144]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) && safe_read(reinterpret_cast<void*>(a[0]), p, sizeof(p))) {
        uint32_t log_level, nplug, nfeat, app_id, engine, render_api;
        uint64_t flags;
        void* feats;
        void* ver;
        void* proj;
        void* logs;
        std::memcpy(&log_level, p + 36, 4);
        std::memcpy(&nplug, p + 48, 4);
        std::memcpy(&logs, p + 56, 8);
        std::memcpy(&flags, p + 88, 8);
        std::memcpy(&feats, p + 96, 8);
        std::memcpy(&nfeat, p + 104, 4);
        std::memcpy(&app_id, p + 108, 4);
        std::memcpy(&engine, p + 112, 4);
        std::memcpy(&ver, p + 120, 8);
        std::memcpy(&proj, p + 128, 8);
        std::memcpy(&render_api, p + 136, 4);
        j.boolean("show_console", p[32] != 0);
        j.u64("log_level", log_level);
        j.u64("num_plugin_paths", nplug);
        wchar_t path[260];
        if (read_arg_wstring(logs, path, 260) && path[0]) j.wstr("path_to_logs_and_data", path);
        j.hex("flags", flags);
        j.u64("application_id", app_id);
        j.u64("engine", engine);
        char s[96];
        if (read_arg_string(ver, s, sizeof(s)) && s[0]) j.str("engine_version", s);
        if (read_arg_string(proj, s, sizeof(s)) && s[0]) j.str("project_id", s);
        j.u64("render_api", render_api);
        j.arr("features_to_load");
        for (uint32_t i = 0; i < nfeat && i < 24; ++i) {
            uint32_t f;
            if (!rd(static_cast<const uint8_t*>(feats) + i * 4, f)) break;
            j.u64(nullptr, f);
        }
        j.end_arr();
    } else {
        j.boolean("preferences_unreadable", true);
    }
    j.hex("sdk_version", a[1]);
}

void dec_sl_set_tag(Json& j, const Q* a) noexcept {
    j.u64("viewport", sl_viewport(reinterpret_cast<void*>(a[0])));
    j.hex("command_buffer", a[3]);
    put_sl_tags(j, reinterpret_cast<void*>(a[1]), static_cast<uint32_t>(a[2]));
}
void dec_sl_set_tag_frame(Json& j, const Q* a) noexcept {
    put_sl_frame(j, "frame", reinterpret_cast<void*>(a[0]));
    j.u64("viewport", sl_viewport(reinterpret_cast<void*>(a[1])));
    j.hex("command_buffer", a[4]);
    put_sl_tags(j, reinterpret_cast<void*>(a[2]), static_cast<uint32_t>(a[3]));
}

void dec_sl_constants(Json& j, const Q* a) noexcept {
    // sl::Constants v2: five float4x4 (320 bytes) after the header, then the fields below.
    uint8_t c[464]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) && safe_read(reinterpret_cast<void*>(a[0]), c, sizeof(c))) {
        float f[18];
        std::memcpy(f, c + 352, sizeof(f)); // jitter, mvec scale, pinhole, pos, up, right, fwd
        j.obj("constants");
        j.f32("jitter_x", f[0]);
        j.f32("jitter_y", f[1]);
        j.f32("mvec_scale_x", f[2]);
        j.f32("mvec_scale_y", f[3]);
        float tail[5];
        std::memcpy(tail, c + 424, sizeof(tail)); // near, far, fov, aspect, mvec invalid value
        j.f32("camera_near", tail[0]);
        j.f32("camera_far", tail[1]);
        j.f32("camera_fov", tail[2]);
        j.f32("camera_aspect", tail[3]);
        j.boolean("depth_inverted", c[444] == 1);
        j.boolean("camera_motion_included", c[445] == 1);
        j.boolean("motion_vectors_3d", c[446] == 1);
        j.boolean("reset", c[447] == 1);
        j.boolean("orthographic", c[448] == 1);
        j.boolean("motion_vectors_dilated", c[449] == 1);
        j.boolean("motion_vectors_jittered", c[450] == 1);
        j.end_obj();
    }
    put_sl_frame(j, "frame", reinterpret_cast<void*>(a[1]));
    j.u64("viewport", sl_viewport(reinterpret_cast<void*>(a[2])));
}

void dec_sl_evaluate(Json& j, const Q* a) noexcept {
    j.u64("feature", static_cast<uint32_t>(a[0]));
    j.str("feature_name", sl_feature_name(static_cast<uint32_t>(a[0])));
    put_sl_frame(j, "frame", reinterpret_cast<void*>(a[1]));
    j.u64("num_inputs", static_cast<uint32_t>(a[3]));
    j.hex("command_buffer", a[4]);
}

void dec_sl_feature_arg(Json& j, const Q* a) noexcept {
    j.u64("feature", static_cast<uint32_t>(a[0]));
    j.str("feature_name", sl_feature_name(static_cast<uint32_t>(a[0])));
}

void dec_sl_viewport(Json& j, const Q* a) noexcept { j.u64("viewport", sl_viewport(reinterpret_cast<void*>(a[0]))); }

void dec_sl_dlssg_options(Json& j, const Q* a) noexcept {
    j.u64("viewport", sl_viewport(reinterpret_cast<void*>(a[0])));
    uint32_t o[17]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) &&
        safe_read(static_cast<uint8_t*>(reinterpret_cast<void*>(a[1])) + kSlHeader, o, sizeof(o))) {
        static const char* const kModes[] = {"Off", "On", "Auto", "Dynamic"};
        j.u64("mode", o[0]);
        j.str("mode_name", o[0] < 4 ? kModes[o[0]] : "other");
        j.u64("num_frames_to_generate", o[1]);
        j.hex("flags", o[2]);
        j.u64("dynamic_res_width", o[3]);
        j.u64("dynamic_res_height", o[4]);
        j.u64("num_back_buffers", o[5]);
        j.u64("mvec_depth_width", o[6]);
        j.u64("mvec_depth_height", o[7]);
        j.u64("color_width", o[8]);
        j.u64("color_height", o[9]);
        j.u64("color_buffer_format", o[10]);
        j.u64("mvec_buffer_format", o[11]);
        j.u64("depth_buffer_format", o[12]);
        j.u64("hudless_buffer_format", o[13]);
        j.u64("ui_buffer_format", o[14]);
    }
}

void dec_sl_dlss_options(Json& j, const Q* a) noexcept {
    j.u64("viewport", sl_viewport(reinterpret_cast<void*>(a[0])));
    uint32_t o[6]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) &&
        safe_read(static_cast<uint8_t*>(reinterpret_cast<void*>(a[1])) + kSlHeader, o, sizeof(o))) {
        float sharp, pre, scale;
        std::memcpy(&sharp, &o[3], 4);
        std::memcpy(&pre, &o[4], 4);
        std::memcpy(&scale, &o[5], 4);
        j.u64("mode", o[0]);
        j.u64("output_width", o[1]);
        j.u64("output_height", o[2]);
        j.f32("sharpness", sharp);
        j.f32("pre_exposure", pre);
        j.f32("exposure_scale", scale);
    }
}

void dec_sl_reflex_options(Json& j, const Q* a) noexcept {
    uint8_t o[16]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) &&
        safe_read(static_cast<uint8_t*>(reinterpret_cast<void*>(a[0])) + kSlHeader, o, sizeof(o))) {
        static const char* const kModes[] = {"Off", "LowLatency", "LowLatencyWithBoost"};
        uint32_t mode, limit, thread;
        uint16_t vk;
        std::memcpy(&mode, o, 4);
        std::memcpy(&limit, o + 4, 4);
        std::memcpy(&vk, o + 10, 2);
        std::memcpy(&thread, o + 12, 4);
        j.u64("mode", mode);
        j.str("mode_name", mode < 3 ? kModes[mode] : "other");
        j.u64("frame_limit_us", limit);
        j.boolean("use_markers_to_optimize", o[8] != 0);
        j.u64("virtual_key", vk);
        j.u64("id_thread", thread);
    }
}

void dec_sl_reflex_sleep(Json& j, const Q* a) noexcept { put_sl_frame(j, "frame", reinterpret_cast<void*>(a[0])); }

void dec_sl_pcl_marker(Json& j, const Q* a) noexcept {
    const uint32_t marker = static_cast<uint32_t>(a[0]);
    static const char* const kNames[] = {"SimulationStart", "SimulationEnd", "RenderSubmitStart",
                                         "RenderSubmitEnd", "PresentStart", "PresentEnd", "",
                                         "TriggerFlash", "PCLatencyPing"};
    j.u64("marker", marker);
    j.str("marker_name", marker < std::size(kNames) && kNames[marker][0] ? kNames[marker] : "other");
    put_sl_frame(j, "frame", reinterpret_cast<void*>(a[1]));
}

void post_sl_frame_token(Json& j, const Q* a, Q) noexcept {
    void* token = nullptr;
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) && rd(reinterpret_cast<void*>(a[0]), token)) {
        put_sl_frame(j, "frame", token);
    }
    if (plausible_ptr(reinterpret_cast<void*>(a[1]))) j.u64("requested_frame_index", rd_or<uint32_t>(reinterpret_cast<void*>(a[1]), 0));
}

// slGetFeatureFunction hands out the feature entry points; replace known ones with recording thunks.
Q sl_get_feature_function(Q feature, Q name, Q out) noexcept {
    using Target = Q (*)(Q, Q, Q);
    auto o = reinterpret_cast<Target>(orig<Fn::SlGetFeatureFunction>());
    Call c(Fn::SlGetFeatureFunction, _ReturnAddress());
    char fname[64]{};
    const bool have_name = read_arg_string(reinterpret_cast<void*>(name), fname, sizeof(fname));
    if (c.full()) {
        c.json().u64("feature", static_cast<uint32_t>(feature));
        c.json().str("feature_name", sl_feature_name(static_cast<uint32_t>(feature)));
        c.json().str("function_name", fname);
    }
    const Q r = o ? o(feature, name, out) : 0;
    if (r == 0 && have_name && plausible_ptr(reinterpret_cast<void*>(out))) {
        const Fn f = lookup_name(fname);
        if (f != Fn::Count && !std::strcmp(kFns[idx(f)].api, "streamline")) {
            void* real = nullptr;
            if (rd(reinterpret_cast<void*>(out), real) && real) {
                void* thunk = bind_thunk(f, real);
                guard([&] { *reinterpret_cast<void**>(out) = thunk; });
            }
        }
    }
    c.finish(r);
    return r;
}

// ---- AMD --------------------------------------------------------------------------------------
void put_guid(Json& j, const char* key, const void* p) noexcept {
    uint8_t g[16];
    if (!plausible_ptr(p) || !safe_read(p, g, sizeof(g))) return;
    char s[48];
    uint32_t d1;
    uint16_t d2, d3;
    std::memcpy(&d1, g, 4);
    std::memcpy(&d2, g + 4, 2);
    std::memcpy(&d3, g + 6, 2);
    std::snprintf(s, sizeof(s), "%08X-%04X-%04X-%02X%02X-%02X%02X%02X%02X%02X%02X", d1, d2, d3, g[8], g[9], g[10],
                  g[11], g[12], g[13], g[14], g[15]);
    j.str(key, s);
}
void dec_amd_interface(Json& j, const Q* a) noexcept {
    j.hex("outer", a[0]);
    put_guid(j, "riid", reinterpret_cast<void*>(a[1]));
}
void post_amd_interface(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "interface", a[2]); }
void dec_ags(Json& j, const Q* a) noexcept { put_raw_args(j, a, 5); }

} // namespace

void register_wrappers_misc(void** t) noexcept {
    t[idx(Fn::NvapiQueryInterface)] = reinterpret_cast<void*>(&nvapi_query_interface);
    RSF_REG(t, NvapiSetSleepMode, dec_nv_sleep_mode);
    RSF_REG(t, NvapiSleep, dec_nv_sleep);
    RSF_REG(t, NvapiSetLatencyMarker, dec_nv_marker);
    RSF_REG(t, NvapiGetLatency, nullptr, post_nv_get_latency);
    RSF_REG(t, NvapiD3D12SetAsyncFrameMarker, dec_nv_marker);

    RSF_REG(t, NvllInitialize);
    RSF_REG(t, NvllInitLowLatencyDevice, dec_nvll_sleep);
    RSF_REG(t, NvllDestroyLowLatencyDevice, dec_nv_sleep);
    RSF_REG(t, NvllSetSleepMode, dec_nvll_mode);
    RSF_REG(t, NvllSleep, dec_nvll_sleep);
    RSF_REG(t, NvllSetLatencyMarker, dec_nvll_marker);
    RSF_REG(t, NvllGetLatency, dec_nv_sleep);

    RSF_REG(t, SlInit, dec_sl_init);
    RSF_REG(t, SlShutdown);
    RSF_REG(t, SlSetTag, dec_sl_set_tag);
    RSF_REG(t, SlSetTagForFrame, dec_sl_set_tag_frame);
    RSF_REG(t, SlSetConstants, dec_sl_constants);
    RSF_REG(t, SlEvaluateFeature, dec_sl_evaluate);
    RSF_REG(t, SlGetNewFrameToken, nullptr, post_sl_frame_token);
    t[idx(Fn::SlGetFeatureFunction)] = reinterpret_cast<void*>(&sl_get_feature_function);
    RSF_REG(t, SlIsFeatureSupported, dec_sl_feature_arg);
    RSF_REG(t, SlSetD3DDevice, dec_nv_sleep);
    RSF_REG(t, SlUpgradeInterface, dec_nv_sleep);
    RSF_REG(t, SlAllocateResources, dec_sl_feature_arg);
    RSF_REG(t, SlFreeResources, dec_sl_feature_arg);
    RSF_REG(t, SlDLSSGSetOptions, dec_sl_dlssg_options);
    RSF_REG(t, SlDLSSGGetState, dec_sl_viewport);
    RSF_REG(t, SlDLSSSetOptions, dec_sl_dlss_options);
    RSF_REG(t, SlReflexSleep, dec_sl_reflex_sleep);
    RSF_REG(t, SlReflexSetOptions, dec_sl_reflex_options);
    RSF_REG(t, SlPclSetMarker, dec_sl_pcl_marker);
    RSF_REG(t, SlPclSetOptions);

    RSF_REG(t, AmdExtD3DCreateInterface, dec_amd_interface, post_amd_interface);
    RSF_REG(t, AgsInitialize, dec_ags);
    RSF_REG(t, AgsDriverExtensionsDX12CreateDevice, dec_ags);
}

} // namespace rsf_census
