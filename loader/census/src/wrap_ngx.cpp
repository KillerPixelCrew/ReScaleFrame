// SPDX-License-Identifier: GPL-3.0-only
// NVIDIA NGX (DLSS) entry points. The parameter object is read through its virtual getters.
//
// Layout source: nvsdk_ngx_params.h, NVSDK_NGX_Parameter, declared in the order
//   Set x8 (ull, float, double, uint, int, ID3D11Resource*, ID3D12Resource*, void*),
//   Get x8 (the same types by pointer), Reset.
// MSVC lays overloaded virtual functions out as one group per name, in reverse declaration order,
// so the slots are Set 0..7 = void*, D3D12, D3D11, int, uint, double, float, ull; Get 8..15 in the
// same reverse order; Reset 16. tests/vendor_census.cpp checks these indices against a class
// compiled from the same declarations.
#include "wrap_generic.h"
#include <rescaleframe/census_d3d12_observer.h>

namespace {
void (*ngx_created_observer)(void*, const void*, uint32_t){};
void (*ngx_evaluate_observer)(void*, void*, const void*, uint32_t, uint64_t){};
void (*ngx_released_observer)(void*){};
}
extern "C" void rsf_census_observe_ngx_d3d12(
    void (*created)(void*, const void*, uint32_t),
    void (*evaluate)(void*, void*, const void*, uint32_t, uint64_t),
    void (*released)(void*))
{
    ngx_created_observer = created;
    ngx_evaluate_observer = evaluate;
    ngx_released_observer = released;
}

namespace rsf_census {

namespace {

constexpr size_t kGetVoidPtr = 8;
constexpr size_t kGetD3D12 = 9;
constexpr size_t kGetD3D11 = 10;
constexpr size_t kGetInt = 11;
constexpr size_t kGetUInt = 12;
constexpr size_t kGetDouble = 13;
constexpr size_t kGetFloat = 14;
constexpr size_t kGetULL = 15;

constexpr int kNgxSuccess = 1; // NVSDK_NGX_Result_Success

template <class T>
bool call_get(const void* params, size_t slot, const char* name, T& out) noexcept {
    using Fn = int32_t (*)(const void*, const char*, T*);
    auto f = reinterpret_cast<Fn>(vtable_slot(params, slot));
    if (!f) return false;
    bool ok = false;
    guard([&] { ok = f(params, name, &out) == kNgxSuccess; });
    return ok;
}

// Read a numeric parameter whatever type the game stored. The expected type is tried first.
bool get_number(const void* params, const char* name, bool prefer_float, double& out) noexcept {
    float f = 0;
    uint32_t u = 0;
    int32_t i = 0;
    uint64_t ull = 0;
    double d = 0;
    if (prefer_float && call_get(params, kGetFloat, name, f)) return out = f, true;
    if (call_get(params, kGetUInt, name, u)) return out = u, true;
    if (call_get(params, kGetInt, name, i)) return out = i, true;
    if (!prefer_float && call_get(params, kGetFloat, name, f)) return out = f, true;
    if (call_get(params, kGetULL, name, ull)) return out = static_cast<double>(ull), true;
    if (call_get(params, kGetDouble, name, d)) return out = d, true;
    return false;
}

void put_number(Json& j, const void* params, const char* name, bool is_float, const char* key = nullptr) noexcept {
    double v;
    if (get_number(params, name, is_float, v)) {
        j.f32(key ? key : name, v);
    }
}

const char* feature_name(uint32_t id) noexcept {
    switch (id) {
    case 1: return "SuperSampling";
    case 2: return "InPainting";
    case 3: return "ImageSuperResolution";
    case 4: return "SlowMotion";
    case 5: return "VideoSuperResolution";
    case 9: return "ImageSignalProcessing";
    case 10: return "DeepResolve";
    case 11: return "FrameGeneration";
    case 12: return "DeepDVC";
    case 13: return "RayReconstruction";
    default: return "other";
    }
}

const char* quality_name(int32_t v) noexcept {
    switch (v) {
    case 0: return "MaxPerf";
    case 1: return "Balanced";
    case 2: return "MaxQuality";
    case 3: return "UltraPerformance";
    case 4: return "UltraQuality";
    case 5: return "DLAA";
    default: return "other";
    }
}

void put_create_flags(Json& j, uint32_t flags) noexcept {
    static const struct {
        uint32_t bit;
        const char* name;
    } kBits[] = {{1u << 0, "IsHDR"},        {1u << 1, "MVLowRes"},     {1u << 2, "MVJittered"},
                 {1u << 3, "DepthInverted"}, {1u << 4, "Reserved_0"}, {1u << 5, "DoSharpening"},
                 {1u << 6, "AutoExposure"}, {1u << 7, "AlphaUpscaling"}, {1u << 8, "Reserved_8"}};
    j.hex("create_flags", flags);
    j.arr("create_flag_names");
    for (const auto& b : kBits) {
        if (flags & b.bit) j.str(nullptr, b.name);
    }
    j.end_arr();
}

// NVSDK_NGX_Resource_VK, as documented in nvsdk_ngx_vk.h: an image view description followed by a
// type and a read/write flag. The offsets are not verified against the header, see the README.
void json_vk_resource(Json& j, const char* key, const void* res) noexcept {
    if (!res) {
        j.null(key);
        return;
    }
    j.obj(key);
    j.hex("ptr", reinterpret_cast<uint64_t>(res));
    uint8_t raw[64]{};
    if (plausible_ptr(res) && safe_read(res, raw, sizeof(raw))) {
        uint64_t view, image;
        uint32_t fmt, w, h, type;
        std::memcpy(&view, raw + 0, 8);
        std::memcpy(&image, raw + 8, 8);
        std::memcpy(&fmt, raw + 36, 4);
        std::memcpy(&w, raw + 40, 4);
        std::memcpy(&h, raw + 44, 4);
        std::memcpy(&type, raw + 48, 4);
        j.hex("image_view", view);
        j.hex("image", image);
        j.u64("vk_format", fmt);
        j.u64("width", w);
        j.u64("height", h);
        j.u64("resource_type", type);
        j.boolean("read_write", raw[52] != 0);
        j.str("layout", "assumed: view@0 image@8 format@36 width@40 height@44 type@48 rw@52");
    } else {
        j.boolean("desc_unreadable", true);
    }
    j.end_obj();
}

template <int Api>
void put_resource(Json& j, const void* params, const char* name) noexcept {
    void* res = nullptr;
    bool ok = false;
    if constexpr (Api == 0) ok = call_get(params, kGetD3D12, name, res);
    if constexpr (Api == 1) ok = call_get(params, kGetD3D11, name, res);
    if (!ok) ok = call_get(params, kGetVoidPtr, name, res);
    if (!ok) return;
    if constexpr (Api == 0) json_d3d12_resource(j, name, res);
    if constexpr (Api == 1) json_d3d11_resource(j, name, res);
    if constexpr (Api == 2) json_vk_resource(j, name, res);
}

void dec_init(Json& j, const Q* a) noexcept {
    j.hex("app_id_or_arg0", a[0]);
    wchar_t path[260];
    if (read_arg_wstring(reinterpret_cast<void*>(a[1]), path, 260) && path[0]) j.wstr("data_path", path);
    put_raw_args(j, a, 6);
}

void dec_init_project(Json& j, const Q* a) noexcept {
    char s[128];
    if (read_arg_string(reinterpret_cast<void*>(a[0]), s, sizeof(s))) j.str("project_id", s);
    j.u64("engine_type", a[1] & 0xFFFFFFFFu);
    if (read_arg_string(reinterpret_cast<void*>(a[2]), s, sizeof(s))) j.str("engine_version", s);
    wchar_t path[260];
    if (read_arg_wstring(reinterpret_cast<void*>(a[3]), path, 260) && path[0]) j.wstr("data_path", path);
    put_raw_args(j, a, 7);
}

void post_params_out(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "parameters", a[0]); }

void dec_params_arg(Json& j, const Q* a) noexcept { j.hex("parameters", a[0]); }

void dec_handle_arg(Json& j, const Q* a) noexcept { j.hex("handle", a[0]); }

template <int Api, int First>
void dec_create(Json& j, const Q* a) noexcept {
    // First is the index of the command list or device context: 0, or 1 for CreateFeature1.
    const uint32_t feature = static_cast<uint32_t>(a[First + 1]);
    const void* params = reinterpret_cast<const void*>(a[First + 2]);
    j.hex("command_list_or_context", a[First]);
    j.u64("feature_id", feature);
    j.str("feature", feature_name(feature));
    j.hex("parameters", a[First + 2]);
    if (!plausible_ptr(params)) return;
    j.obj("create");
    put_number(j, params, "Width", false);
    put_number(j, params, "Height", false);
    put_number(j, params, "OutWidth", false);
    put_number(j, params, "OutHeight", false);
    double q;
    if (get_number(params, "PerfQualityValue", false, q)) {
        j.i64("PerfQualityValue", static_cast<int64_t>(q));
        j.str("quality", quality_name(static_cast<int32_t>(q)));
    }
    double flags;
    if (get_number(params, "DLSS.Feature.Create.Flags", false, flags)) {
        put_create_flags(j, static_cast<uint32_t>(flags));
    }
    put_number(j, params, "DLSS.Enable.Output.Subrects", false);
    put_number(j, params, "CreationNodeMask", false);
    put_number(j, params, "VisibilityNodeMask", false);
    j.end_obj();
}

template <int First>
void post_create(Json& j, const Q* a, Q ret) noexcept {
    put_out_ptr(j, "handle", a[First + 3]);
    (void)ret;
}

template <int First>
void observe_create(const Q* a, Q result) noexcept {
    if (!ngx_created_observer || static_cast<uint32_t>(result) != kNgxSuccess) return;
    void* handle{};
    if (safe_read(reinterpret_cast<void*>(a[First + 3]), &handle, sizeof(handle)) && handle)
        ngx_created_observer(handle, reinterpret_cast<const void*>(a[First + 2]), static_cast<uint32_t>(a[First + 1]));
}
void observe_evaluate_begin(const Q* a) noexcept {
    if (ngx_evaluate_observer)
        ngx_evaluate_observer(reinterpret_cast<void*>(a[0]), reinterpret_cast<void*>(a[1]),
            reinterpret_cast<const void*>(a[2]), 0, 0);
}
void observe_evaluate_end(const Q* a, Q result) noexcept {
    if (ngx_evaluate_observer)
        ngx_evaluate_observer(reinterpret_cast<void*>(a[0]), reinterpret_cast<void*>(a[1]),
            reinterpret_cast<const void*>(a[2]), 1, result);
}
void observe_release(const Q* a, Q result) noexcept {
    if (ngx_released_observer && static_cast<uint32_t>(result) == kNgxSuccess)
        ngx_released_observer(reinterpret_cast<void*>(a[0]));
}

template <int Api>
void dec_evaluate(Json& j, const Q* a) noexcept {
    const void* params = reinterpret_cast<const void*>(a[2]);
    j.hex("command_list_or_context", a[0]);
    j.hex("handle", a[1]);
    j.hex("parameters", a[2]);
    j.hex("progress_callback", a[3]);
    if (!plausible_ptr(params)) return;
    j.obj("resources");
    put_resource<Api>(j, params, "Color");
    put_resource<Api>(j, params, "Output");
    put_resource<Api>(j, params, "Depth");
    put_resource<Api>(j, params, "MotionVectors");
    put_resource<Api>(j, params, "TransparencyMask");
    put_resource<Api>(j, params, "ExposureTexture");
    put_resource<Api>(j, params, "DLSS.Input.Bias.Current.Color.Mask");
    j.end_obj();
    j.obj("scalars");
    put_number(j, params, "Jitter.Offset.X", true);
    put_number(j, params, "Jitter.Offset.Y", true);
    put_number(j, params, "MV.Scale.X", true);
    put_number(j, params, "MV.Scale.Y", true);
    put_number(j, params, "Reset", false);
    put_number(j, params, "Sharpness", true);
    put_number(j, params, "DLSS.Render.Subrect.Dimensions.Width", false);
    put_number(j, params, "DLSS.Render.Subrect.Dimensions.Height", false);
    put_number(j, params, "DLSS.Pre.Exposure", true);
    put_number(j, params, "DLSS.Exposure.Scale", true);
    put_number(j, params, "FrameTimeDeltaInMsec", true);
    static const char* const kBases[] = {
        "DLSS.Input.Color.Subrect.Base.X",         "DLSS.Input.Color.Subrect.Base.Y",
        "DLSS.Input.Depth.Subrect.Base.X",         "DLSS.Input.Depth.Subrect.Base.Y",
        "DLSS.Input.MV.Subrect.Base.X",            "DLSS.Input.MV.Subrect.Base.Y",
        "DLSS.Input.Translucency.Subrect.Base.X",  "DLSS.Input.Translucency.Subrect.Base.Y",
        "DLSS.Output.Subrect.Base.X",              "DLSS.Output.Subrect.Base.Y",
        "DLSS.Input.Bias.Current.Color.Subrect.Base.X", "DLSS.Input.Bias.Current.Color.Subrect.Base.Y"};
    for (const char* n : kBases) put_number(j, params, n, false);
    j.end_obj();
}

} // namespace

#define RSF_REG_NGX(t, A, API)                                                   \
    RSF_REG(t, Ngx##A##Init, dec_init);                                                        \
    RSF_REG(t, Ngx##A##InitExt, dec_init);                                                     \
    RSF_REG(t, Ngx##A##InitProjectId, dec_init_project);                                       \
    RSF_REG(t, Ngx##A##InitWithProjectId, dec_init_project);                                   \
    RSF_REG(t, Ngx##A##Shutdown);                                                              \
    RSF_REG(t, Ngx##A##Shutdown1, dec_handle_arg);                                             \
    RSF_REG(t, Ngx##A##GetParameters, nullptr, post_params_out);                               \
    RSF_REG(t, Ngx##A##AllocateParameters, nullptr, post_params_out);                          \
    RSF_REG(t, Ngx##A##GetCapabilityParameters, nullptr, post_params_out);                     \
    RSF_REG(t, Ngx##A##DestroyParameters, dec_params_arg);                                     \
    RSF_REG(t, Ngx##A##CreateFeature, (dec_create<API, 0>), (post_create<0>));                 \
    RSF_REG(t, Ngx##A##CreateFeature1, (dec_create<API, 1>), (post_create<1>));                \
    RSF_REG(t, Ngx##A##EvaluateFeature, (dec_evaluate<API>));                                  \
    RSF_REG(t, Ngx##A##ReleaseFeature, dec_handle_arg)

void register_wrappers_ngx(void** t) noexcept {
    RSF_REG_NGX(t, D3D12, 0);
    RSF_REG(t, NgxD3D12CreateFeature, (dec_create<0, 0>), (post_create<0>), (observe_create<0>));
    RSF_REG(t, NgxD3D12CreateFeature1, (dec_create<0, 1>), (post_create<1>), (observe_create<1>));
    RSF_REG(t, NgxD3D12EvaluateFeature, (dec_evaluate<0>), nullptr, observe_evaluate_end, observe_evaluate_begin);
    RSF_REG(t, NgxD3D12ReleaseFeature, dec_handle_arg, nullptr, observe_release);
    RSF_REG_NGX(t, D3D11, 1);
    RSF_REG_NGX(t, VULKAN, 2);
}

} // namespace rsf_census
