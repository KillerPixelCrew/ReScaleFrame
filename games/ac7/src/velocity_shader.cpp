// SPDX-License-Identifier: MIT
#include "velocity_shader.h"
#include "shader_bytes.h"
#include <rescaleframe/shader_compile.h>
#include <string>
#include <utility>

namespace {
struct rsf_ac7_velocity_semantic_view {
    const char* name;
    size_t name_size;
    uint32_t index, system, type, reg;
    uint8_t mask, used;
};
static_assert(sizeof(rsf_ac7_velocity_semantic_view) == 40);

using Semantic = rsf_ac7_velocity_semantic;
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

using CompileShaderHelper = decltype(&rsf_compile_system_shader);

struct VertexMetadata {
    uint32_t particle;
    uint32_t position_register;
};
static_assert(sizeof(VertexMetadata) == 8);
}

extern "C" bool rsf_core_ac7_velocity_pixel_shader(const uint8_t*, size_t, uint32_t blend,
    uint32_t previous, uint32_t current, CompileShaderHelper,
    void* user, uint8_t* (*resize)(void*, size_t)) noexcept;

extern "C" bool rsf_core_ac7_velocity_vertex_shader(const uint8_t*, size_t, bool tessellation, bool beam,
    CompileShaderHelper, void* user,
    uint8_t* (*resize)(void*, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    VertexMetadata*) noexcept;

rsf_ac7_velocity_shader_result rsf_ac7_velocity_pixel_shader(const void* bytes, size_t size,
    uint32_t blend, std::vector<uint8_t>& output, uint32_t previous_register, uint32_t current_register) noexcept
{
    // The input may point into output. Keep it alive until Rust has finished borrowing it.
    std::vector<uint8_t> patched;
    if (rsf_core_ac7_velocity_pixel_shader(static_cast<const uint8_t*>(bytes), size, blend,
            previous_register, current_register, rsf_compile_system_shader, &patched, rsf::ac7::resize_shader_bytes)) {
        output.swap(patched);
        return rsf_ac7_velocity_shader_result::patched;
    }
    output.clear();
    return rsf_ac7_velocity_shader_result::refused;
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
            beam, rsf_compile_system_shader, &patched,
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

}

extern "C" bool rsf_core_ac7_velocity_tessellation(const uint8_t*, size_t, const uint8_t*, size_t,
    const rsf_ac7_velocity_semantic_view*, size_t, CompileShaderHelper, void* user,
    uint8_t* (*resize)(void*, uint32_t, size_t), bool (*append)(void*, const rsf_ac7_velocity_semantic_view*),
    uint32_t* input_points, const DomainRegisters*) noexcept;

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
        if (rsf_core_ac7_velocity_geometry_shader(inputs.data(), inputs.size(), rsf_compile_system_shader,
                &bytecode, rsf::ac7::resize_shader_bytes, &registers))
            return true;
    } catch (...) {}
    bytecode.clear();
    return false;
}

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
                inputs.data(), inputs.size(), rsf_compile_system_shader, &patched,
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

rsf_ac7_velocity_shader_result rsf_ac7_velocity_tessellation(const void* hull, size_t hull_size,
    const void* domain, size_t domain_size, const rsf_ac7_velocity_vertex_info& vertex,
    std::vector<uint8_t>& ordinary, std::vector<uint8_t>& corners, std::vector<uint8_t>& evaluated,
    rsf_ac7_velocity_vertex_info& info, uint32_t& input_points, uint32_t& previous, uint32_t& current) noexcept
{
    input_points = 0;
    try {
        struct Destination {
            std::vector<uint8_t> code[3];
            rsf_ac7_velocity_vertex_info info;
        } patched;
        const auto inputs = semantic_views(vertex.outputs);
        const DomainRegisters registers{&previous, &current};
        if (rsf_core_ac7_velocity_tessellation(static_cast<const uint8_t*>(hull), hull_size,
                static_cast<const uint8_t*>(domain), domain_size, inputs.data(), inputs.size(),
                rsf_compile_system_shader, &patched,
                [](void* user, uint32_t stage, size_t count) noexcept -> uint8_t* {
                    auto& destination = *static_cast<Destination*>(user);
                    return stage < 3 ? rsf::ac7::resize_shader_bytes(&destination.code[stage], count) : nullptr;
                },
                [](void* user, const rsf_ac7_velocity_semantic_view* view) noexcept {
                    return append_semantic(&static_cast<Destination*>(user)->info.outputs, view);
                }, &input_points, &registers)) {
            patched.info.particle = vertex.particle;
            ordinary.swap(patched.code[0]);
            corners.swap(patched.code[1]);
            evaluated.swap(patched.code[2]);
            info = std::move(patched.info);
            return rsf_ac7_velocity_shader_result::patched;
        }
    } catch (...) {}
    ordinary.clear();
    corners.clear();
    evaluated.clear();
    info = {};
    return rsf_ac7_velocity_shader_result::refused;
}
