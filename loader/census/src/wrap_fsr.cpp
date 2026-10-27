// SPDX-License-Identifier: GPL-3.0-only
// AMD FSR 2.x (ffx_fsr2_api), the FidelityFX API (FSR 3.1 and later) and Intel XeSS, XeFG and XeLL.
//
// Structure layouts are replicated from the vendor headers and pinned with static_assert:
//   FSR 2.1.2   references/OptiScaler/OptiScaler/include/fsr2_212/ffx_fsr2.h and ffx_types.h
//   FidelityFX API  vendor/fidelityfx/Kits/FidelityFX/{api,upscalers,framegeneration}/include
//   XeSS        vendor/xess/inc/xess/xess_d3d12.h
#include <dxgi1_6.h>

#include <cmath>

#include "wrap_generic.h"

namespace rsf_census {

namespace {

// ---- FSR 2.1.2 --------------------------------------------------------------------------------
struct Fsr2ResourceDescription {
    uint32_t type, format, width, height, depth, mip_count, flags;
};
struct Fsr2Resource {
    void* resource;
    wchar_t name[64];
    Fsr2ResourceDescription description;
    uint32_t state;
    bool is_depth;
    uint64_t descriptor_data;
};
struct Fsr2Dispatch {
    void* command_list;
    Fsr2Resource color, depth, motion_vectors, exposure, reactive, transparency_and_composition, output;
    float jitter_offset[2];
    float motion_vector_scale[2];
    uint32_t render_size[2];
    bool enable_sharpening;
    float sharpness;
    float frame_time_delta;
    float pre_exposure;
    bool reset;
    float camera_near, camera_far, camera_fov_angle_vertical;
};
static_assert(sizeof(Fsr2Resource) == 184, "FSR 2.1.2 FfxResource is 184 bytes");
static_assert(offsetof(Fsr2Resource, name) == 8 && offsetof(Fsr2Resource, description) == 136);
static_assert(offsetof(Fsr2Resource, state) == 164 && offsetof(Fsr2Resource, is_depth) == 168);
static_assert(offsetof(Fsr2Resource, descriptor_data) == 176);
static_assert(offsetof(Fsr2Dispatch, jitter_offset) == 1296 && sizeof(Fsr2Dispatch) == 1352);

bool ascii_name_ok(const wchar_t* n) noexcept {
    for (size_t i = 0; i < 64; ++i) {
        if (n[i] == 0) return true;
        if (n[i] < 0x20 || n[i] > 0x7E) return false;
    }
    return false;
}

bool finite_in(float v, float limit) noexcept { return std::isfinite(v) && std::fabs(v) <= limit; }

// Layout heuristics for a dispatch description whose size the API never states. A decode is refused
// unless all of these hold; the record then carries only the length that was read.
const char* fsr2_layout_check(const Fsr2Dispatch& d) noexcept {
    const Fsr2Resource* res[] = {&d.color, &d.depth, &d.motion_vectors, &d.exposure,
                                 &d.reactive, &d.transparency_and_composition, &d.output};
    int plausible = 0;
    for (const Fsr2Resource* r : res) {
        const bool name_ok = ascii_name_ok(r->name);
        const bool desc_ok = r->description.type <= 3 && r->description.format < 64 &&
                             r->description.width <= 16384 && r->description.height <= 16384 &&
                             (r->state >> 16) == 0 && static_cast<uint8_t>(r->is_depth) <= 1;
        if (name_ok && desc_ok) ++plausible;
    }
    if (plausible < 5) return "resource_names_or_descriptions_implausible";
    if (!d.color.resource || !d.output.resource) return "color_or_output_null";
    if (!finite_in(d.jitter_offset[0], 16) || !finite_in(d.jitter_offset[1], 16)) return "jitter_implausible";
    if (!finite_in(d.motion_vector_scale[0], 1e6f) || !finite_in(d.motion_vector_scale[1], 1e6f)) {
        return "motion_vector_scale_implausible";
    }
    if (d.render_size[0] == 0 || d.render_size[1] == 0 || d.render_size[0] > 16384 || d.render_size[1] > 16384) {
        return "render_size_implausible";
    }
    if (static_cast<uint8_t>(d.enable_sharpening) > 1 || static_cast<uint8_t>(d.reset) > 1) return "bool_implausible";
    if (!finite_in(d.sharpness, 8) || !finite_in(d.frame_time_delta, 1e6f) || !finite_in(d.pre_exposure, 1e9f)) {
        return "scalar_implausible";
    }
    if (!finite_in(d.camera_near, 1e9f) || !finite_in(d.camera_far, 1e12f) ||
        !finite_in(d.camera_fov_angle_vertical, 7)) {
        return "camera_implausible";
    }
    return nullptr;
}

void put_fsr2_resource(Json& j, const char* key, const Fsr2Resource& r) noexcept {
    j.obj(key);
    json_ptr_or_null(j, "ptr", r.resource);
    j.wstr("name", r.name);
    j.u64("type", r.description.type);
    j.u64("format", r.description.format);
    j.u64("width", r.description.width);
    j.u64("height", r.description.height);
    j.u64("depth", r.description.depth);
    j.u64("mips", r.description.mip_count);
    j.hex("flags", r.description.flags);
    j.hex("state", r.state);
    j.boolean("is_depth", r.is_depth);
    if (r.resource && vtable_slot(r.resource, 10)) json_d3d12_resource(j, "d3d12", r.resource);
    j.end_obj();
}

void dec_fsr2_dispatch(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    j.str("layout", "fsr2-2.1.2 (dispatch description 1352 bytes)");
    Fsr2Dispatch d;
    if (!plausible_ptr(reinterpret_cast<void*>(a[1])) ||
        !safe_read(reinterpret_cast<void*>(a[1]), &d, sizeof(d))) {
        j.str("layout_check", "unreadable");
        j.u64("raw_bytes_len", 0);
        return;
    }
    if (const char* why = fsr2_layout_check(d)) {
        j.str("layout_check", "failed");
        j.str("reason", why);
        j.u64("raw_bytes_len", sizeof(d));
        return;
    }
    j.str("layout_check", "ok");
    j.hex("command_list", reinterpret_cast<uint64_t>(d.command_list));
    j.obj("resources");
    put_fsr2_resource(j, "color", d.color);
    put_fsr2_resource(j, "depth", d.depth);
    put_fsr2_resource(j, "motion_vectors", d.motion_vectors);
    put_fsr2_resource(j, "exposure", d.exposure);
    put_fsr2_resource(j, "reactive", d.reactive);
    put_fsr2_resource(j, "transparency_and_composition", d.transparency_and_composition);
    put_fsr2_resource(j, "output", d.output);
    j.end_obj();
    j.obj("scalars");
    j.f32("jitter_x", d.jitter_offset[0]);
    j.f32("jitter_y", d.jitter_offset[1]);
    j.f32("mv_scale_x", d.motion_vector_scale[0]);
    j.f32("mv_scale_y", d.motion_vector_scale[1]);
    j.u64("render_width", d.render_size[0]);
    j.u64("render_height", d.render_size[1]);
    j.boolean("enable_sharpening", d.enable_sharpening);
    j.f32("sharpness", d.sharpness);
    j.f32("frame_time_delta_ms", d.frame_time_delta);
    j.f32("pre_exposure", d.pre_exposure);
    j.boolean("reset", d.reset);
    j.f32("camera_near", d.camera_near);
    j.f32("camera_far", d.camera_far);
    j.f32("camera_fov_vertical", d.camera_fov_angle_vertical);
    j.end_obj();
}

void dec_fsr2_create(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    uint32_t head[7]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), head, sizeof(head))) {
        static const struct {
            uint32_t bit;
            const char* name;
        } kBits[] = {{1, "HDR"},          {2, "DisplayResolutionMotionVectors"}, {4, "MotionVectorsJitterCancellation"},
                     {8, "DepthInverted"}, {16, "DepthInfinite"},                 {32, "AutoExposure"},
                     {64, "DynamicResolution"}, {128, "Texture1DUsage"}};
        j.hex("flags", head[0]);
        j.arr("flag_names");
        for (const auto& b : kBits) {
            if (head[0] & b.bit) j.str(nullptr, b.name);
        }
        j.end_arr();
        j.u64("max_render_width", head[1]);
        j.u64("max_render_height", head[2]);
        j.u64("display_width", head[3]);
        j.u64("display_height", head[4]);
    }
}

void dec_hex_args2(Json& j, const Q* a) noexcept { put_raw_args(j, a, 2); }
void dec_hex_args4(Json& j, const Q* a) noexcept { put_raw_args(j, a, 4); }
void dec_hex_args6(Json& j, const Q* a) noexcept { put_raw_args(j, a, 6); }

void dec_jitter_args(Json& j, const Q* a) noexcept {
    j.i64("index", static_cast<int32_t>(a[2]));
    j.i64("phase_count", static_cast<int32_t>(a[3]));
}
void post_jitter(Json& j, const Q* a, Q) noexcept {
    float x, y;
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) && rd(reinterpret_cast<void*>(a[0]), x)) j.f32("jitter_x", x);
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && rd(reinterpret_cast<void*>(a[1]), y)) j.f32("jitter_y", y);
}
void dec_phase_count(Json& j, const Q* a) noexcept {
    j.i64("render_width", static_cast<int32_t>(a[0]));
    j.i64("display_width", static_cast<int32_t>(a[1]));
}
void dec_render_res(Json& j, const Q* a) noexcept {
    j.u64("display_width", static_cast<uint32_t>(a[2]));
    j.u64("display_height", static_cast<uint32_t>(a[3]));
    j.i64("quality_mode", static_cast<int32_t>(a[4]));
}
void post_render_res(Json& j, const Q* a, Q) noexcept {
    uint32_t w, h;
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) && rd(reinterpret_cast<void*>(a[0]), w)) j.u64("render_width", w);
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && rd(reinterpret_cast<void*>(a[1]), h)) j.u64("render_height", h);
}

// ffxGetResourceDX12 returns a 184 byte FfxResource, so the hidden return pointer is the first
// argument. Which of the other arguments hold the resource and the name differs between SDK
// versions, so the record carries the raw arguments and the structure that came back.
void post_get_resource(Json& j, const Q* a, Q) noexcept {
    put_raw_args(j, a, 5);
    Fsr2Resource r;
    if (plausible_ptr(reinterpret_cast<void*>(a[0])) && safe_read(reinterpret_cast<void*>(a[0]), &r, sizeof(r)) &&
        ascii_name_ok(r.name) && r.description.type <= 3) {
        put_fsr2_resource(j, "returned_resource", r);
    }
}

// ---- FidelityFX API ---------------------------------------------------------------------------
struct ApiResourceDesc {
    uint32_t type, format, width, height, depth, mip_count, flags, usage;
};
struct ApiResource {
    void* resource;
    ApiResourceDesc description;
    uint32_t state;
};
struct ApiHeader {
    uint64_t type;
    ApiHeader* next;
};
struct ApiDispatchUpscale {
    ApiHeader header;
    void* command_list;
    ApiResource color, depth, motion_vectors, exposure, reactive, transparency_and_composition, output;
    float jitter[2];
    float mv_scale[2];
    uint32_t render_size[2];
    uint32_t upscale_size[2];
    bool enable_sharpening;
    float sharpness;
    float frame_time_delta;
    float pre_exposure;
    bool reset;
    float camera_near, camera_far, camera_fov, view_space_to_meters;
    uint32_t flags;
};
struct ApiRect {
    int32_t left, top, width, height;
};
struct ApiDispatchFrameGen {
    ApiHeader header;
    void* command_list;
    ApiResource present_color;
    ApiResource outputs[4];
    uint32_t num_generated_frames;
    bool reset;
    uint32_t backbuffer_transfer_function;
    float min_max_luminance[2];
    ApiRect generation_rect;
    uint64_t frame_id;
};
struct ApiConfigureFrameGen {
    ApiHeader header;
    void* swap_chain;
    void* present_callback;
    void* present_callback_user_context;
    void* frame_generation_callback;
    void* frame_generation_callback_user_context;
    bool frame_generation_enabled;
    bool allow_async_workloads;
    ApiResource hudless_color;
    uint32_t flags;
    bool only_present_generated;
    ApiRect generation_rect;
    uint64_t frame_id;
};
struct ApiPrepareV2 {
    ApiHeader header;
    uint64_t frame_id;
    uint32_t flags;
    void* command_list;
    uint32_t render_size[2];
    float jitter[2];
    float mv_scale[2];
    float frame_time_delta;
    bool reset;
    float camera_near, camera_far, camera_fov, view_space_to_meters;
    ApiResource depth, motion_vectors;
    float camera_position[3], camera_up[3], camera_right[3], camera_forward[3];
};
static_assert(sizeof(ApiResource) == 48, "FfxApiResource is 48 bytes");
static_assert(offsetof(ApiDispatchUpscale, jitter) == 360 && sizeof(ApiDispatchUpscale) == 432);
static_assert(offsetof(ApiDispatchFrameGen, frame_id) == 304 && sizeof(ApiDispatchFrameGen) == 312);
static_assert(offsetof(ApiConfigureFrameGen, frame_id) == 136 && sizeof(ApiConfigureFrameGen) == 144);
static_assert(offsetof(ApiPrepareV2, depth) == 88);

const char* api_type_name(uint64_t t) noexcept {
    switch (t) {
    case 0x00010000: return "create_upscale";
    case 0x00010001: return "dispatch_upscale";
    case 0x00010002: return "query_upscale_ratio";
    case 0x00010003: return "query_render_resolution";
    case 0x00010004: return "query_jitter_phase_count";
    case 0x00010005: return "query_jitter_offset";
    case 0x00010006: return "dispatch_generate_reactive_mask";
    case 0x00010007: return "configure_upscale_keyvalue";
    case 0x0001000b: return "create_upscale_version";
    case 0x00020001: return "create_framegeneration";
    case 0x00020002: return "configure_framegeneration";
    case 0x00020003: return "dispatch_framegeneration";
    case 0x00020004: return "dispatch_framegeneration_prepare";
    case 0x00020005: return "callback_framegeneration_present";
    case 0x00020006: return "configure_framegeneration_keyvalue";
    case 0x00020008: return "configure_register_distortion";
    case 0x00020009: return "create_framegeneration_hudless";
    case 0x0002000a: return "dispatch_framegeneration_prepare_camerainfo";
    case 0x0002000c: return "dispatch_framegeneration_prepare_v2";
    case 0x00000002: return "create_backend_dx12";
    case 0x00000003: return "create_backend_dx12_allocation_callbacks";
    case 0x00030001: return "create_swapchain_wrap_dx12";
    case 0x00030002: return "configure_swapchain_register_ui_dx12";
    case 0x00030005: return "create_swapchain_new_dx12";
    case 0x00030006: return "create_swapchain_for_hwnd_dx12";
    case 0x00030007: return "dispatch_swapchain_wait_for_presents_dx12";
    case 0x00030008: return "configure_swapchain_keyvalue_dx12";
    case 0x0003000b: return "create_swapchain_version_dx12";
    case 0x00000007: return "configure_global_debug";
    default: return nullptr;
    }
}

void put_api_resource(Json& j, const char* key, const ApiResource& r) noexcept {
    j.obj(key);
    json_ptr_or_null(j, "ptr", r.resource);
    j.u64("type", r.description.type);
    j.u64("format", r.description.format);
    j.u64("width", r.description.width);
    j.u64("height", r.description.height);
    j.u64("depth", r.description.depth);
    j.u64("mips", r.description.mip_count);
    j.hex("flags", r.description.flags);
    j.hex("usage", r.description.usage);
    j.hex("state", r.state);
    if (r.resource && vtable_slot(r.resource, 10)) json_d3d12_resource(j, "d3d12", r.resource);
    j.end_obj();
}

void put_swapchain_desc(Json& j, const char* key, const void* desc, bool v1) noexcept {
    if (!plausible_ptr(desc)) return;
    j.obj(key);
    if (v1) {
        DXGI_SWAP_CHAIN_DESC d;
        if (safe_read(desc, &d, sizeof(d))) {
            j.u64("width", d.BufferDesc.Width);
            j.u64("height", d.BufferDesc.Height);
            j.u64("format", d.BufferDesc.Format);
            j.str("format_name", dxgi_format_name(d.BufferDesc.Format));
            j.u64("buffer_count", d.BufferCount);
            j.u64("swap_effect", d.SwapEffect);
            j.hex("flags", d.Flags);
        }
    } else {
        DXGI_SWAP_CHAIN_DESC1 d;
        if (safe_read(desc, &d, sizeof(d))) {
            j.u64("width", d.Width);
            j.u64("height", d.Height);
            j.u64("format", d.Format);
            j.str("format_name", dxgi_format_name(d.Format));
            j.u64("buffer_count", d.BufferCount);
            j.u64("swap_effect", d.SwapEffect);
            j.hex("flags", d.Flags);
        }
    }
    j.end_obj();
}

// Walk a header chain. Dispatch and configure payloads that are understood are decoded in full.
void put_api_chain(Json& j, const void* head) noexcept {
    j.arr("chain");
    const ApiHeader* h = static_cast<const ApiHeader*>(head);
    for (int depth = 0; depth < 12 && plausible_ptr(h); ++depth) {
        ApiHeader cur;
        if (!rd(h, cur)) break;
        j.obj();
        j.hex("type", cur.type);
        if (const char* n = api_type_name(cur.type)) j.str("name", n);
        switch (cur.type) {
        case 0x00010000: {
            struct {
                ApiHeader h;
                uint32_t flags, max_render[2], max_upscale[2];
            } c;
            if (rd(h, c)) {
                j.hex("flags", c.flags);
                j.u64("max_render_width", c.max_render[0]);
                j.u64("max_render_height", c.max_render[1]);
                j.u64("max_upscale_width", c.max_upscale[0]);
                j.u64("max_upscale_height", c.max_upscale[1]);
            }
            break;
        }
        case 0x00020001: {
            struct {
                ApiHeader h;
                uint32_t flags, display[2], max_render[2], back_buffer_format;
            } c;
            if (rd(h, c)) {
                j.hex("flags", c.flags);
                j.u64("display_width", c.display[0]);
                j.u64("display_height", c.display[1]);
                j.u64("max_render_width", c.max_render[0]);
                j.u64("max_render_height", c.max_render[1]);
                j.u64("back_buffer_format", c.back_buffer_format);
            }
            break;
        }
        case 0x00030001: {
            struct {
                ApiHeader h;
                void** swapchain;
                void* game_queue;
            } c;
            if (rd(h, c)) {
                json_ptr_or_null(j, "game_queue", c.game_queue);
                put_out_ptr(j, "input_swapchain", reinterpret_cast<Q>(c.swapchain));
            }
            break;
        }
        case 0x00030005: {
            struct {
                ApiHeader h;
                void** swapchain;
                void* desc;
                void* factory;
                void* game_queue;
            } c;
            if (rd(h, c)) {
                json_ptr_or_null(j, "game_queue", c.game_queue);
                json_ptr_or_null(j, "factory", c.factory);
                put_swapchain_desc(j, "swapchain_desc", c.desc, true);
            }
            break;
        }
        case 0x00030006: {
            struct {
                ApiHeader h;
                void** swapchain;
                void* hwnd;
                void* desc;
                void* fullscreen_desc;
                void* factory;
                void* game_queue;
            } c;
            if (rd(h, c)) {
                json_ptr_or_null(j, "hwnd", c.hwnd);
                json_ptr_or_null(j, "game_queue", c.game_queue);
                json_ptr_or_null(j, "factory", c.factory);
                put_swapchain_desc(j, "swapchain_desc1", c.desc, false);
            }
            break;
        }
        case 0x00000002: {
            struct {
                ApiHeader h;
                void* device;
            } c;
            if (rd(h, c)) json_ptr_or_null(j, "device", c.device);
            break;
        }
        case 0x00010001: {
            ApiDispatchUpscale d;
            if (rd(h, d)) {
                j.hex("command_list", reinterpret_cast<uint64_t>(d.command_list));
                j.obj("resources");
                put_api_resource(j, "color", d.color);
                put_api_resource(j, "depth", d.depth);
                put_api_resource(j, "motion_vectors", d.motion_vectors);
                put_api_resource(j, "exposure", d.exposure);
                put_api_resource(j, "reactive", d.reactive);
                put_api_resource(j, "transparency_and_composition", d.transparency_and_composition);
                put_api_resource(j, "output", d.output);
                j.end_obj();
                j.obj("scalars");
                j.f32("jitter_x", d.jitter[0]);
                j.f32("jitter_y", d.jitter[1]);
                j.f32("mv_scale_x", d.mv_scale[0]);
                j.f32("mv_scale_y", d.mv_scale[1]);
                j.u64("render_width", d.render_size[0]);
                j.u64("render_height", d.render_size[1]);
                j.u64("upscale_width", d.upscale_size[0]);
                j.u64("upscale_height", d.upscale_size[1]);
                j.boolean("enable_sharpening", d.enable_sharpening);
                j.f32("sharpness", d.sharpness);
                j.f32("frame_time_delta_ms", d.frame_time_delta);
                j.f32("pre_exposure", d.pre_exposure);
                j.boolean("reset", d.reset);
                j.f32("camera_near", d.camera_near);
                j.f32("camera_far", d.camera_far);
                j.f32("camera_fov_vertical", d.camera_fov);
                j.f32("view_space_to_meters", d.view_space_to_meters);
                j.hex("flags", d.flags);
                j.end_obj();
            }
            break;
        }
        case 0x00020003: {
            ApiDispatchFrameGen d;
            if (rd(h, d)) {
                j.hex("command_list", reinterpret_cast<uint64_t>(d.command_list));
                put_api_resource(j, "present_color", d.present_color);
                put_api_resource(j, "output0", d.outputs[0]);
                j.u64("num_generated_frames", d.num_generated_frames);
                j.boolean("reset", d.reset);
                j.u64("backbuffer_transfer_function", d.backbuffer_transfer_function);
                j.f32("min_luminance", d.min_max_luminance[0]);
                j.f32("max_luminance", d.min_max_luminance[1]);
                j.u64("frame_id", d.frame_id);
            }
            break;
        }
        case 0x00020002: {
            ApiConfigureFrameGen d;
            if (rd(h, d)) {
                json_ptr_or_null(j, "swap_chain", d.swap_chain);
                json_ptr_or_null(j, "present_callback", d.present_callback);
                json_ptr_or_null(j, "frame_generation_callback", d.frame_generation_callback);
                j.boolean("frame_generation_enabled", d.frame_generation_enabled);
                j.boolean("allow_async_workloads", d.allow_async_workloads);
                put_api_resource(j, "hudless_color", d.hudless_color);
                j.hex("flags", d.flags);
                j.boolean("only_present_generated", d.only_present_generated);
                j.u64("frame_id", d.frame_id);
            }
            break;
        }
        case 0x0002000c: {
            ApiPrepareV2 d;
            if (rd(h, d)) {
                j.u64("frame_id", d.frame_id);
                j.hex("flags", d.flags);
                j.hex("command_list", reinterpret_cast<uint64_t>(d.command_list));
                j.u64("render_width", d.render_size[0]);
                j.u64("render_height", d.render_size[1]);
                j.f32("jitter_x", d.jitter[0]);
                j.f32("jitter_y", d.jitter[1]);
                j.f32("mv_scale_x", d.mv_scale[0]);
                j.f32("mv_scale_y", d.mv_scale[1]);
                j.f32("frame_time_delta_ms", d.frame_time_delta);
                j.boolean("reset", d.reset);
                j.f32("camera_near", d.camera_near);
                j.f32("camera_far", d.camera_far);
                j.f32("camera_fov_vertical", d.camera_fov);
                put_api_resource(j, "depth", d.depth);
                put_api_resource(j, "motion_vectors", d.motion_vectors);
            }
            break;
        }
        default: break;
        }
        j.end_obj();
        h = cur.next;
    }
    j.end_arr();
}

void dec_api_create(Json& j, const Q* a) noexcept {
    j.hex("context_out", a[0]);
    put_api_chain(j, reinterpret_cast<void*>(a[1]));
}
void post_api_create(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "context", a[0]); }
void dec_api_chain(Json& j, const Q* a) noexcept {
    j.hex("context", a[0] ? rd_or<uint64_t>(reinterpret_cast<void*>(a[0]), 0) : 0);
    put_api_chain(j, reinterpret_cast<void*>(a[1]));
}
void dec_api_destroy(Json& j, const Q* a) noexcept {
    j.hex("context", a[0] ? rd_or<uint64_t>(reinterpret_cast<void*>(a[0]), 0) : 0);
}

// ---- XeSS -------------------------------------------------------------------------------------
struct XessCoord {
    uint32_t x, y;
};
struct XessExecute {
    void *color, *velocity, *depth, *exposure_scale, *responsive_mask, *output;
    float jitter_x, jitter_y, exposure;
    uint32_t reset_history;
    uint32_t input_width, input_height;
    XessCoord color_base, mv_base, depth_base, mask_base, reserved0, output_base;
    void* descriptor_heap;
    uint32_t descriptor_heap_offset;
};
static_assert(offsetof(XessExecute, input_width) == 64 && offsetof(XessExecute, color_base) == 72);
static_assert(offsetof(XessExecute, descriptor_heap) == 120);

void dec_xess_create(Json& j, const Q* a) noexcept { j.hex("device", a[0]); }
void post_xess_create(Json& j, const Q* a, Q) noexcept { put_out_ptr(j, "context", a[1]); }

void dec_xess_init(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    uint32_t p[6]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), p, sizeof(p))) {
        j.u64("output_width", p[0]);
        j.u64("output_height", p[1]);
        j.u64("quality_setting", p[2]);
        j.hex("init_flags", p[3]);
        j.u64("creation_node_mask", p[4]);
        j.u64("visible_node_mask", p[5]);
    }
}

void dec_xess_execute(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    j.hex("command_list", a[1]);
    XessExecute e;
    if (!plausible_ptr(reinterpret_cast<void*>(a[2])) || !safe_read(reinterpret_cast<void*>(a[2]), &e, sizeof(e))) {
        j.boolean("params_unreadable", true);
        return;
    }
    j.obj("resources");
    json_d3d12_resource(j, "color", e.color);
    json_d3d12_resource(j, "velocity", e.velocity);
    json_d3d12_resource(j, "depth", e.depth);
    json_d3d12_resource(j, "exposure_scale", e.exposure_scale);
    json_d3d12_resource(j, "responsive_mask", e.responsive_mask);
    json_d3d12_resource(j, "output", e.output);
    j.end_obj();
    j.obj("scalars");
    j.f32("jitter_x", e.jitter_x);
    j.f32("jitter_y", e.jitter_y);
    j.f32("exposure_scale", e.exposure);
    j.u64("reset_history", e.reset_history);
    j.u64("input_width", e.input_width);
    j.u64("input_height", e.input_height);
    j.end_obj();
}

void dec_xell_sleep(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    j.u64("frame_id", static_cast<uint32_t>(a[1]));
}
void dec_xell_marker(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    j.u64("frame_id", static_cast<uint32_t>(a[1]));
    j.u64("marker", static_cast<uint32_t>(a[2]));
}
void dec_xell_mode(Json& j, const Q* a) noexcept {
    j.hex("context", a[0]);
    uint8_t raw[16]{};
    if (plausible_ptr(reinterpret_cast<void*>(a[1])) && safe_read(reinterpret_cast<void*>(a[1]), raw, sizeof(raw))) {
        j.bytes_hex("params_first_16_bytes", raw, sizeof(raw));
    }
}

// XeSS functions taking floats: exact signatures keep the xmm registers intact.
int32_t xess_set_jitter_scale(void* ctx, float x, float y) noexcept {
    using Target = int32_t (*)(void*, float, float);
    auto o = reinterpret_cast<Target>(orig<Fn::XessSetJitterScale>());
    Call c(Fn::XessSetJitterScale, _ReturnAddress());
    if (c.full()) {
        c.json().hex("context", reinterpret_cast<uint64_t>(ctx));
        c.json().f32("x", x);
        c.json().f32("y", y);
    }
    const int32_t r = o ? o(ctx, x, y) : 0;
    c.finish(static_cast<uint32_t>(r));
    return r;
}
int32_t xess_set_velocity_scale(void* ctx, float x, float y) noexcept {
    using Target = int32_t (*)(void*, float, float);
    auto o = reinterpret_cast<Target>(orig<Fn::XessSetVelocityScale>());
    Call c(Fn::XessSetVelocityScale, _ReturnAddress());
    if (c.full()) {
        c.json().hex("context", reinterpret_cast<uint64_t>(ctx));
        c.json().f32("x", x);
        c.json().f32("y", y);
    }
    const int32_t r = o ? o(ctx, x, y) : 0;
    c.finish(static_cast<uint32_t>(r));
    return r;
}
int32_t xess_set_exposure(void* ctx, float scale) noexcept {
    using Target = int32_t (*)(void*, float);
    auto o = reinterpret_cast<Target>(orig<Fn::XessSetExposureMultiplier>());
    Call c(Fn::XessSetExposureMultiplier, _ReturnAddress());
    if (c.full()) {
        c.json().hex("context", reinterpret_cast<uint64_t>(ctx));
        c.json().f32("scale", scale);
    }
    const int32_t r = o ? o(ctx, scale) : 0;
    c.finish(static_cast<uint32_t>(r));
    return r;
}
// The ratio function returns a float in xmm0, so it cannot use the integer wrapper.
float fsr2_upscale_ratio_exact(int32_t mode) noexcept {
    using Target = float (*)(int32_t);
    auto o = reinterpret_cast<Target>(orig<Fn::Fsr2GetUpscaleRatio>());
    Call c(Fn::Fsr2GetUpscaleRatio, _ReturnAddress());
    if (c.full()) c.json().i64("quality_mode", mode);
    const float r = o ? o(mode) : 0.0f;
    if (c.full()) c.json().f32("ratio", r);
    c.finish(0);
    return r;
}

} // namespace

void register_wrappers_fsr(void** t) noexcept {
    RSF_REG(t, Fsr2ContextCreate, dec_fsr2_create);
    RSF_REG(t, Fsr2ContextDispatch, dec_fsr2_dispatch);
    RSF_REG(t, Fsr2ContextGenerateReactiveMask, dec_hex_args2);
    RSF_REG(t, Fsr2ContextDestroy, dec_hex_args2);
    RSF_REG(t, Fsr2GetJitterOffset, dec_jitter_args, post_jitter);
    RSF_REG(t, Fsr2GetJitterPhaseCount, dec_phase_count);
    RSF_REG(t, Fsr2GetRenderResolution, dec_render_res, post_render_res);
    t[idx(Fn::Fsr2GetUpscaleRatio)] = reinterpret_cast<void*>(&fsr2_upscale_ratio_exact);
    RSF_REG(t, Fsr2GetInterfaceDX12, dec_hex_args4);
    RSF_REG(t, Fsr2GetScratchMemorySizeDX12);
    RSF_REG(t, FfxGetResourceDX12, nullptr, post_get_resource);
    RSF_REG(t, FfxGetCommandListDX12, dec_hex_args2);
    RSF_REG(t, FfxGetDeviceDX12, dec_hex_args2);

    RSF_REG(t, FfxCreateContext, dec_api_create, post_api_create);
    RSF_REG(t, FfxDestroyContext, dec_api_destroy);
    RSF_REG(t, FfxConfigure, dec_api_chain);
    RSF_REG(t, FfxQuery, dec_api_chain);
    RSF_REG(t, FfxDispatch, dec_api_chain);

    RSF_REG(t, XessD3D12CreateContext, dec_xess_create, post_xess_create);
    RSF_REG(t, XessD3D12Init, dec_xess_init);
    RSF_REG(t, XessD3D12Execute, dec_xess_execute);
    RSF_REG(t, XessD3D11CreateContext, dec_xess_create, post_xess_create);
    RSF_REG(t, XessD3D11Init, dec_hex_args4);
    RSF_REG(t, XessD3D11Execute, dec_hex_args4);
    RSF_REG(t, XessVKCreateContext, dec_hex_args4);
    RSF_REG(t, XessVKInit, dec_hex_args4);
    RSF_REG(t, XessVKExecute, dec_hex_args4);
    t[idx(Fn::XessSetJitterScale)] = reinterpret_cast<void*>(&xess_set_jitter_scale);
    t[idx(Fn::XessSetVelocityScale)] = reinterpret_cast<void*>(&xess_set_velocity_scale);
    t[idx(Fn::XessSetExposureMultiplier)] = reinterpret_cast<void*>(&xess_set_exposure);
    RSF_REG(t, XessDestroyContext, dec_hex_args2);

    RSF_REG(t, XefgCreateContext, dec_hex_args4);
    RSF_REG(t, XefgInitFromSwapChain, dec_hex_args6);
    RSF_REG(t, XefgInitFromSwapChainDesc, dec_hex_args6);
    RSF_REG(t, XefgTagFrameResource, dec_hex_args6);
    RSF_REG(t, XefgTagFrameConstants, dec_hex_args4);
    RSF_REG(t, XefgSetEnabled, dec_hex_args2);
    RSF_REG(t, XefgSetPresentId, dec_hex_args2);
    RSF_REG(t, XefgSetLatencyReduction, dec_hex_args2);
    RSF_REG(t, XefgSetNumInterpolatedFrames, dec_hex_args2);
    RSF_REG(t, XefgSetUiCompositionState, dec_hex_args6);
    RSF_REG(t, XefgDestroy, dec_hex_args2);

    RSF_REG(t, XellD3D12CreateContext, dec_hex_args2);
    RSF_REG(t, XellDestroyContext, dec_hex_args2);
    RSF_REG(t, XellSetSleepMode, dec_xell_mode);
    RSF_REG(t, XellGetSleepMode, dec_xell_mode);
    RSF_REG(t, XellSleep, dec_xell_sleep);
    RSF_REG(t, XellAddMarkerData, dec_xell_marker);
}

} // namespace rsf_census
