// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_registry_platform.h"
#include "../temporal/history.h"
#include <exception>

namespace rsf::anomaly::geometry {
// Keep any exception from the C++ history owner on this side of the C boundary.
struct RetirementCall {
    void (*apply)(void*, temporal::InstanceKey);
    void* user;
    std::exception_ptr exception{};

    static void visit(void* context, const rsf_anomaly_geometry_key* key) noexcept {
        auto& call = *static_cast<RetirementCall*>(context);
        if (call.exception) return;
        try {
            call.apply(call.user, {key->owner, key->visual, key->generation});
        } catch (...) {
            call.exception = std::current_exception();
        }
    }
};
}
