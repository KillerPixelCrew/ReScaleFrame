// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>

struct rsf_ac7_velocity_device;

struct rsf_ac7_velocity_variant {
    void *vertex, *pixel, *capture, *geometry, *hull, *corners, *domain, *layout;
    uint32_t maximum_vertices, history_slot, geometry_hash, input_points;
    bool particle;
};

struct rsf_ac7_velocity_variant_request {
    void* shaders[5];
    void* layout;
    uint32_t material_blend, stride;
};

struct rsf_ac7_velocity_kernels {
    void *hash, *merge, *fullscreen, *validate_indirect;
};

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*rsf_ac7_velocity_refusal_fn)(const char*);
// The caller keeps the render lease around all borrowed interface pointers.
struct rsf_ac7_velocity_device* rsf_ac7_velocity_device_create(void* device,
    struct rsf_ac7_velocity_kernels* kernels);
void rsf_ac7_velocity_device_destroy(struct rsf_ac7_velocity_device* owner);
bool rsf_ac7_velocity_device_variant(struct rsf_ac7_velocity_device* owner,
    const struct rsf_ac7_velocity_variant_request* request, rsf_ac7_velocity_refusal_fn refusal,
    uint32_t* vertex_hash, struct rsf_ac7_velocity_variant* variant);
uint32_t rsf_ac7_velocity_device_count(const struct rsf_ac7_velocity_device* owner);

#ifdef __cplusplus
}
#endif
