// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "core/object/object.h"
#include "core/variant/dictionary.h"
#include "core/math/vector2i.h"
#include "core/math/projection.h"
#include "core/object/ref_counted.h"
class RenderDataRD;
class RenderSceneBuffersRD;

class ReScaleFrameBridge : public Object {
    GDCLASS(ReScaleFrameBridge, Object);
protected:
    static void _bind_methods();
public:
    bool configure(const Dictionary &options, RID viewport);
    Dictionary get_status() const;
    void reset_history();
};

namespace RSFGodot {
void boot();
bool create_swapchain(void *factory, void *queue, void *window, const void *description, void **out);
void shutdown();
void cpu_begin();
void cpu_simulation_begin();
void queue_frame(bool present);
void cpu_end();
void input_event(uint32_t kind);
void upscale(const RenderDataRD *data, double delta);
void hudless(const RenderDataRD *data);
void submitted(void *queue, void *const *commands, uint32_t count);
void present(void *swapchain);
void render_size(RID target, const Vector2i &output, Vector2i &internal);
void camera_changed(RID target);
RID resolve_motion(const Ref<RenderSceneBuffersRD> &buffers, RID depth, RID motion,
    const Projection &to_previous, const Vector2 &jitter);
void release_motion();
bool vulkan_enabled();
uint64_t vk_resolver();
bool vk_present_marker(uint64_t window, uint32_t begin);
int32_t vk_native_present(uint64_t callback, void* queue, const void* info);
int32_t vk_wsi_call(int32_t (*callback)(void*), void* user);
bool vk_create(const void *setup, void **owner, uint64_t *chain);
int32_t vk_images(void *owner, uint32_t *count, uint64_t *images);
int32_t vk_acquire(void *owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t *index);
int32_t vk_present(void *owner, const void *info);
void vk_submitted(void *const *commands, uint32_t count);
bool vk_destroy(void *owner);
bool vk_drain(void *owner);
}
