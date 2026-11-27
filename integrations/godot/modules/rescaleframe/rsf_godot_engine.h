// SPDX-License-Identifier: MIT
#pragma once
#include "core/object/object.h"
#include "core/variant/dictionary.h"
#include "core/math/vector2i.h"
#include "core/object/ref_counted.h"
#include "native.h"
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

// Every RSFGodot name below is called from engine source inserted by eng/patch-godot.py.
// The engine is C++, so its call sites name C++ functions; the forwarders defer to native.rs.
namespace RSFGodot {
void boot(bool headless);
inline bool create_swapchain(void *factory, void *queue, void *window, const void *description, void **out) {
    return rsf_godot_native_create_swapchain(factory, queue, window, description, out) != 0;
}
void cpu_begin();
void cpu_sample();
void cpu_simulation_begin();
void queue_frame(bool present);
void cpu_end();
void input_event(uint32_t kind);
void upscale(const RenderDataRD *data, double delta);
void hudless(const RenderDataRD *data);
inline void submitted(void *queue, void *const *commands, uint32_t count) { rsf_godot_native_submitted(queue, commands, count); }
inline void present(void *swapchain) { rsf_godot_native_present(swapchain); }
void render_size(RID target, const Vector2i &output, Vector2i &internal);
void camera_changed(RID target);
RID resolve_motion(const Ref<RenderSceneBuffersRD> &buffers, RID depth, RID motion,
    const float *to_previous, const Vector2 &jitter, bool normalize_depth, RID &resolved_depth);
void release_motion();
inline bool vulkan_enabled() { return rsf_godot_native_vulkan_enabled() != 0; }
inline uint64_t vk_resolver() { return rsf_godot_native_vk_resolver(); }

struct ExecutionReceipt {
    uint64_t queue, fence, session, source;
    uint32_t submitted;
};

// One RenderingDevice::_execute_frame call. A successful final submit owns the receipt
// across either the same driver call or Godot's separate presentation queue call.
class VulkanExecutionScope {
    VulkanExecutionScope *previous;
    ExecutionReceipt receipt{};

public:
    VulkanExecutionScope(uint64_t queue, uint64_t fence, bool eligible);
    ~VulkanExecutionScope();
    VulkanExecutionScope(const VulkanExecutionScope &) = delete;
    VulkanExecutionScope &operator=(const VulkanExecutionScope &) = delete;
    static void submit(uint64_t queue, uint64_t fence, bool begin);
    static bool present(uint64_t window, bool begin);
};
inline int32_t vk_foreign_call(int32_t (*callback)(void *), void *user) { return rsf_godot_native_vk_foreign_call(callback, user); }
inline uint32_t vk_image_layout(void *owner) { return rsf_godot_native_vk_image_layout(owner); }
inline void vk_recording_begin(void *commands) { rsf_godot_native_vk_recording_begin(commands); }
inline void vk_submitted(void *const *commands, uint32_t count) { rsf_godot_native_vk_submitted(commands, count); }
}
