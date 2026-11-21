// SPDX-License-Identifier: MIT
#include "velocity_shader.h"
#include "shader_bytes.h"
#include "velocity_container.h"
#include <rescaleframe/shader_compile.h>
#include <atomic>
#include <string>
#include <utility>

namespace {
std::atomic<uint32_t> geometry_compiles{0};
using Semantic=rsf_ac7_velocity_semantic;
bool append_semantic(void* user, const rsf_ac7_velocity_semantic_view* view) noexcept
try {
    auto& semantics = *static_cast<std::vector<Semantic>*>(user);
    semantics.push_back({std::string(view->name, view->name_size),
        view->index, view->system, view->type, view->reg, view->mask, view->used});
    return true;
} catch (...) { return false; }

std::vector<rsf_ac7_velocity_semantic_view> semantic_views(const std::vector<Semantic>& semantics)
{
    std::vector<rsf_ac7_velocity_semantic_view> views;
    views.reserve(semantics.size());
    for (const auto& semantic : semantics)
        views.push_back({semantic.name.data(), semantic.name.size(), semantic.index,
            semantic.system, semantic.type, semantic.reg, semantic.mask, semantic.used});
    return views;
}

// D3DCompiler's blobs and module remain native. The cached copy owns the returned
// bytecode for the lifetime of this module, including throughout every Rust call.
bool compile_pixel_helper(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto bytecode = [source, length] {
        std::vector<uint8_t> result;
        rsf::compile_shader(source, length, "ac7-translucent-velocity", "main", "ps_5_0", result);
        return result;
    }();
    *bytes = bytecode.data();
    *size = bytecode.size();
    return !bytecode.empty();
} catch (...) { return false; }

struct VertexShaderSource {
    const char* text;
    size_t length;
};
struct VertexMetadata {
    uint32_t particle;
    uint32_t position_register;
};
static_assert(sizeof(VertexShaderSource) == 16);
static_assert(sizeof(VertexMetadata) == 8);

bool compile_vertex_helper(const VertexShaderSource* sources, uint32_t particle,
    const uint8_t** bytes, size_t* size) noexcept
try {
    if (particle >= 4) return false;
    const auto compile = [sources](size_t index, const char* name) {
        const auto& source = sources[index];
        std::vector<uint8_t> result;
        rsf::compile_shader(source.text, source.length, name, "main", "vs_5_0", result);
        return result;
    };
    // Keep the original initialization order and lend bytecode owned by this module.
    static const auto generic = compile(0, "ac7-geometry-key");
    static const auto sprite = compile(1, "ac7-particle-key");
    static const auto mesh = compile(2, "ac7-mesh-particle-pose-key");
    static const auto beam = compile(3, "ac7-beam-sample-key");
    const std::vector<uint8_t>* const helpers[]{&generic, &sprite, &mesh, &beam};
    const auto& helper = *helpers[particle];
    *bytes = helper.data();
    *size = helper.size();
    return !helper.empty();
} catch (...) { return false; }
}

extern "C" bool rsf_core_ac7_velocity_pixel_shader(const uint8_t*, size_t, uint32_t blend,
    uint32_t previous, uint32_t current, bool (*compile_helper)(const char*, size_t, const uint8_t**, size_t*),
    void* user, uint8_t* (*resize)(void*, size_t)) noexcept;

extern "C" bool rsf_core_ac7_velocity_vertex_shader(const uint8_t*, size_t, bool tessellation, bool beam,
    bool (*compile_helper)(const VertexShaderSource*, uint32_t, const uint8_t**, size_t*), void* user,
    uint8_t* (*resize)(void*, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    VertexMetadata*) noexcept;

rsf_ac7_velocity_shader_result rsf_ac7_velocity_pixel_shader(const void* bytes, size_t size,
    uint32_t blend, std::vector<uint8_t>& output, uint32_t previous_register, uint32_t current_register) noexcept
{
    // The input may point into output. Keep it alive until Rust has finished borrowing it.
    std::vector<uint8_t> patched;
    if (rsf_core_ac7_velocity_pixel_shader(static_cast<const uint8_t*>(bytes), size, blend,
            previous_register, current_register, compile_pixel_helper, &patched, rsf::ac7::resize_shader_bytes)) {
        output.swap(patched);
        return rsf_ac7_velocity_shader_result::patched;
    }
    output.clear();
    return rsf_ac7_velocity_shader_result::refused;
}

bool rsf_ac7_velocity_signature(const void* bytes,size_t size,bool output,
    std::vector<rsf_ac7_velocity_semantic>& semantics) noexcept
{
    std::vector<rsf_ac7_velocity_semantic> result;
    const bool ok = rsf_core_ac7_velocity_signature(static_cast<const uint8_t*>(bytes), size,
        output, &result, append_semantic);
    semantics.swap(result);
    return ok;
}

rsf_ac7_velocity_shader_result rsf_ac7_velocity_vertex_shader(const void* bytes,size_t size,
    std::vector<uint8_t>& output,rsf_ac7_velocity_vertex_info& info,bool tessellation,bool beam) noexcept
{
    // Keep caller-owned input live even when it points into the destination vector.
    struct Destination {
        std::vector<uint8_t> bytes;
        rsf_ac7_velocity_vertex_info info;
    } patched;
    VertexMetadata metadata{};
    if (rsf_core_ac7_velocity_vertex_shader(static_cast<const uint8_t*>(bytes), size, tessellation,
            beam, compile_vertex_helper, &patched,
            [](void* user, size_t count) noexcept {
                return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->bytes, count);
            },
            [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
            }, &metadata)) {
        patched.info.particle = metadata.particle;
        patched.info.position_register = metadata.position_register;
        output.swap(patched.bytes);
        info = std::move(patched.info);
        return rsf_ac7_velocity_shader_result::patched;
    }
    output.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}

namespace {
struct GeometryRegisters {
    uint32_t* previous;
    uint32_t* current;
    uint32_t* maximum_vertices;
};
static_assert(sizeof(GeometryRegisters) == 24);

struct DomainRegisters {
    uint32_t* previous;
    uint32_t* current;
};
static_assert(sizeof(DomainRegisters) == 16);

using CompileShaderHelper = bool (*)(const char*, size_t, const uint8_t**, size_t*);

// Each typed compiler leaf lends its immutable bytecode for the lifetime of the module.
bool compile_history_geometry(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto code = [source, length] {
        std::vector<uint8_t> bytecode;
        ++geometry_compiles;
        try {
            rsf::compile_shader(source, length, "ac7-previous-geometry", "main", "gs_5_0", bytecode);
        } catch (...) { bytecode.clear(); }
        return bytecode;
    }();
    *bytes = code.data();
    *size = code.size();
    return !code.empty();
} catch (...) { return false; }

bool compile_native_geometry_helper(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto code = [source, length] {
        std::vector<uint8_t> bytecode;
        rsf::compile_shader(source, length, "ac7-native-generated-geometry", "main", "vs_5_0", bytecode);
        return bytecode;
    }();
    *bytes = code.data();
    *size = code.size();
    return !code.empty();
} catch (...) { return false; }

bool compile_domain_helper(const char* source, size_t length, const uint8_t** bytes, size_t* size) noexcept
try {
    static const auto code = [source, length] {
        std::vector<uint8_t> bytecode;
        rsf::compile_shader(source, length, "ac7-tessellated-control-motion", "main", "vs_5_0", bytecode);
        return bytecode;
    }();
    *bytes = code.data();
    *size = code.size();
    return !code.empty();
} catch (...) { return false; }
}

extern "C" bool rsf_core_ac7_velocity_hull_shader(const uint8_t*, size_t,
    const rsf_ac7_velocity_semantic_view*, size_t, void* user,
    uint8_t* (*resize_ordinary)(void*, size_t), uint8_t* (*resize_corners)(void*, size_t),
    bool (*append)(void*, const rsf_ac7_velocity_semantic_view*), uint32_t* input_points) noexcept;

extern "C" bool rsf_core_ac7_velocity_domain_shader(const uint8_t*, size_t,
    const rsf_ac7_velocity_semantic_view*, size_t, CompileShaderHelper, void* user,
    uint8_t* (*resize)(void*, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    const DomainRegisters*) noexcept;

extern "C" bool rsf_core_ac7_velocity_geometry_shader(const rsf_ac7_velocity_semantic_view*, size_t,
    CompileShaderHelper, void* user, uint8_t* (*resize)(void*, size_t), const GeometryRegisters*) noexcept;

extern "C" bool rsf_core_ac7_velocity_native_geometry_shader(const uint8_t*, size_t,
    const rsf_ac7_velocity_semantic_view*, size_t, CompileShaderHelper, void* user,
    uint8_t* (*resize)(void*, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    const GeometryRegisters*) noexcept;

bool rsf_ac7_velocity_geometry_shader(const rsf_ac7_velocity_vertex_info& info,
    std::vector<uint8_t>& bytecode, uint32_t& previous_register, uint32_t& current_register) noexcept
{
    bytecode.clear();
    previous_register = current_register = 0;
    try {
        const auto inputs = semantic_views(info.outputs);
        const GeometryRegisters registers{&previous_register, &current_register, nullptr};
        if (rsf_core_ac7_velocity_geometry_shader(inputs.data(), inputs.size(), compile_history_geometry,
                &bytecode, rsf::ac7::resize_shader_bytes, &registers))
            return true;
    } catch (...) {}
    bytecode.clear();
    return false;
}

uint32_t rsf_ac7_velocity_geometry_compile_count() noexcept { return geometry_compiles.load(); }

rsf_ac7_velocity_shader_result rsf_ac7_velocity_native_geometry_shader(const void* bytes, size_t size,
    const rsf_ac7_velocity_vertex_info& vertex, std::vector<uint8_t>& output,
    rsf_ac7_velocity_vertex_info& info, uint32_t& maximum_vertices,
    uint32_t& previous_register, uint32_t& current_register) noexcept
{
    maximum_vertices = 0;
    try {
        // The original DXBC and upstream semantics may live in the caller's destinations.
        struct Destination {
            std::vector<uint8_t> bytes;
            rsf_ac7_velocity_vertex_info info;
        } patched;
        const auto inputs = semantic_views(vertex.outputs);
        const GeometryRegisters registers{&previous_register, &current_register, &maximum_vertices};
        if (rsf_core_ac7_velocity_native_geometry_shader(static_cast<const uint8_t*>(bytes), size,
                inputs.data(), inputs.size(), compile_native_geometry_helper, &patched,
                [](void* user, size_t count) noexcept {
                    return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->bytes, count);
                },
                [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                    return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
                }, &registers)) {
            patched.info.particle = vertex.particle;
            output.swap(patched.bytes);
            info = std::move(patched.info);
            return rsf_ac7_velocity_shader_result::patched;
        }
    } catch (...) {}
    output.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}

rsf_ac7_velocity_shader_result rsf_ac7_velocity_hull_shader(const void* bytes, size_t size,
    const rsf_ac7_velocity_vertex_info& vertex, std::vector<uint8_t>& output,
    std::vector<uint8_t>& corners, rsf_ac7_velocity_vertex_info& info, uint32_t& input_points) noexcept
{
    input_points = 0;
    try {
        // The original DXBC and upstream semantic names may live in the destinations.
        struct Destination {
            std::vector<uint8_t> ordinary;
            std::vector<uint8_t> corners;
            rsf_ac7_velocity_vertex_info info;
        } patched;
        const auto inputs = semantic_views(vertex.outputs);
        if (rsf_core_ac7_velocity_hull_shader(static_cast<const uint8_t*>(bytes), size,
                inputs.data(), inputs.size(), &patched,
                [](void* user, size_t count) noexcept {
                    return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->ordinary, count);
                },
                [](void* user, size_t count) noexcept {
                    return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->corners, count);
                },
                [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                    return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
                }, &input_points)) {
            patched.info.particle = vertex.particle;
            output.swap(patched.ordinary);
            corners.swap(patched.corners);
            info = std::move(patched.info);
            return rsf_ac7_velocity_shader_result::patched;
        }
    } catch (...) {}
    output.clear();
    corners.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}

rsf_ac7_velocity_shader_result rsf_ac7_velocity_domain_shader(const void* bytes, size_t size,
    const rsf_ac7_velocity_vertex_info& hull, std::vector<uint8_t>& output,
    rsf_ac7_velocity_vertex_info& info, uint32_t& previous_register, uint32_t& current_register) noexcept
{
    try {
        // Publish only after Rust has finished borrowing the original DXBC and hull metadata.
        struct Destination {
            std::vector<uint8_t> bytes;
            rsf_ac7_velocity_vertex_info info;
        } patched;
        const auto inputs = semantic_views(hull.outputs);
        const DomainRegisters registers{&previous_register, &current_register};
        if (rsf_core_ac7_velocity_domain_shader(static_cast<const uint8_t*>(bytes), size,
                inputs.data(), inputs.size(), compile_domain_helper, &patched,
                [](void* user, size_t count) noexcept {
                    return rsf::ac7::resize_shader_bytes(&static_cast<Destination*>(user)->bytes, count);
                },
                [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                    return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
                }, &registers)) {
            patched.info.particle = hull.particle;
            output.swap(patched.bytes);
            info = std::move(patched.info);
            return rsf_ac7_velocity_shader_result::patched;
        }
    } catch (...) {}
    output.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}
