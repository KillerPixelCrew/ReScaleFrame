// SPDX-License-Identifier: MIT
#include "../../graphics/src/frame_tap_platform.h"

namespace {

// Existing frame-tap callbacks can be supplied by C++ clients. Keep their exceptions
// inside native code. Report callbacks are ignored on failure and decisions decline.
template <class Callback, class... Arguments>
void invoke(Callback callback, Arguments... arguments) noexcept
{
    try {
        callback(arguments...);
    } catch (...) {
    }
}

const rsf_frame_tap_platform callbacks = [] {
    rsf_frame_tap_platform result{};
    result.log = invoke<rsf_frame_tap_log_fn, void*, const char*>;
    result.pass = invoke<rsf_frame_tap_fn, void*, const rsf_frame_tap_pass*>;
    result.report = invoke<rsf_frame_tap_target_fn, void*, const rsf_frame_tap_target_draw*>;
    result.geometry = invoke<rsf_frame_tap_geometry_fn, void*, const rsf_frame_tap_geometry*>;
    result.compute =
        invoke<rsf_frame_tap_compute_fn, void*, void*, uint32_t, uint32_t, uint32_t, void*, uint32_t>;
    result.constants = invoke<rsf_frame_tap_constants_fn, void*, void*, void*, uint32_t>;
    result.gate = [](rsf_frame_tap_gate_fn fn, void* user, void* context, void* texture) -> int32_t {
        try {
            return fn(user, context, texture);
        } catch (...) {
            return 0;
        }
    };
    result.verdict = [](rsf_frame_tap_verdict_fn fn, void* user,
                        const rsf_frame_tap_target_draw* facts) -> rsf_frame_tap_verdict {
        try {
            return fn(user, facts);
        } catch (...) {
            return RSF_FRAME_TAP_LEAVE;
        }
    };
    result.override_constants = [](rsf_frame_tap_constant_override_fn fn, void* user,
                                    const rsf_frame_tap_target_draw* facts, uint32_t* slots,
                                    void** buffers) -> int32_t {
        try {
            return fn(user, facts, slots, buffers);
        } catch (...) {
            return 0;
        }
    };
    return result;
}();
}

extern "C" const rsf_frame_tap_platform* rsf_frame_tap_foreign_callbacks(void)
{
    return &callbacks;
}

extern "C" void rsf_frame_tap_call_constant_observer(
    rsf_frame_tap_constant_observer_fn fn, void* user, void* buffer, const void* contents,
    uint32_t bytes)
{
    invoke(fn, user, buffer, contents, bytes);
}
