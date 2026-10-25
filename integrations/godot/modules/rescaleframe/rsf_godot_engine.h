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
}
