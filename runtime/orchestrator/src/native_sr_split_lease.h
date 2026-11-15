// SPDX-License-Identifier: GPL-3.0-only
#pragma once
namespace rsf::sr {
// CPU admission only. Successful submission never substitutes for GPU completion.
struct SplitLease {
    bool tracked = false;
    void* producer = nullptr;
    void* continuation = nullptr;
    bool pending() const { return tracked && (producer || continuation); }
    void begin(void* commands) { if (tracked) producer = commands; }
    bool submitted(void* commands) {
        if (!tracked || !commands) return false;
        if (commands == producer) { producer = nullptr; return true; }
        // A continuation cannot run until its producer has been submitted.
        if (!producer && commands == continuation) { continuation = nullptr; return true; }
        return false;
    }
};
}
