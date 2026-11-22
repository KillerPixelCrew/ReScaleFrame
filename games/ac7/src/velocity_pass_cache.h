// SPDX-License-Identifier: MIT
#pragma once

#include <rescaleframe/d3d11_observer.h>
#include <cstddef>
#include <cstdint>

// velocity_pass_cache.rs owns the observed shader sources and input layouts and the variant
// admission table. velocity_pass.cpp keeps the built D3D11 variants at the slots it reports.

// The draw's shaders in observer stage order (vertex, pixel, geometry, hull, domain), absent
// stages null, its input layout, material blend and first stream stride.
struct VelocityVariantRequest {
    void* shaders[5];
    void* layout;
    uint32_t material_blend, stride;
};
// Lent to the builder for its call: retained bytecode in the same stage order,
// empty for absent stages, the layout descriptors (none when unobserved), the history identity of
// generated geometry and whether the layout is the beam/trail factory declaration.
struct VelocityVariantSources {
    const uint8_t* bytes[5];
    size_t sizes[5];
    const rsf_observer_layout_element* elements;
    uint32_t element_count, geometry_hash;
    bool beam_layout;
};
// Refused is remembered for the key; unstored is retried on a later draw.
enum : int32_t { VELOCITY_VARIANT_REFUSED = 0, VELOCITY_VARIANT_BUILT = 1, VELOCITY_VARIANT_UNSTORED = 2 };
constexpr uint32_t VELOCITY_NO_VARIANT = UINT32_MAX;
// A built variant writes its slot. The builder must not throw.
using VelocityBuildVariant = int32_t (*)(void* user, const VelocityVariantSources* sources, uint32_t* slot);

extern "C" {
// The slot of a reusable variant, or VELOCITY_NO_VARIANT. Writes the vertex bytecode hash once
// the vertex and pixel sources are admitted. The builder runs without the cache lock.
uint32_t rsf_ac7_velocity_admit_variant(const VelocityVariantRequest* request, void* user,
    VelocityBuildVariant build, uint32_t* vertex_hash);
size_t rsf_ac7_velocity_variant_count(void);
// velocity_renderer.rs resets the table when it publishes a renderer and releases the cache.
}

static_assert(sizeof(VelocityVariantRequest) == 56 && offsetof(VelocityVariantRequest, stride) == 52);
static_assert(sizeof(VelocityVariantSources) == 104 && offsetof(VelocityVariantSources, elements) == 80 &&
    offsetof(VelocityVariantSources, beam_layout) == 96);
