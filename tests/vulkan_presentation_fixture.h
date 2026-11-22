// SPDX-License-Identifier: MIT
#pragma once
#include "../runtime/orchestrator/src/native_latency_vulkan_presentation.h"

namespace rsf::latency {
inline auto presentation_attach(rsf_fgvk* context) { return rsf_core_fgvk_presentation_attach(context); }
inline auto presentation_unbind() { return rsf_core_fgvk_presentation_unbind(); }

// Retain the Rust context lease across SDK calls without keeping its state lock.
class PresentationLease {
    uint32_t closing_{};
    rsf_fgvk* context_ = rsf_core_fgvk_presentation_borrow(&closing_);
public:
    PresentationLease() = default;
    ~PresentationLease() { if (context_) rsf_core_fgvk_presentation_return(); }
    PresentationLease(const PresentationLease&) = delete;
    PresentationLease& operator=(const PresentationLease&) = delete;
    rsf_fgvk* get() const { return context_; }
    bool closing() const { return closing_ != 0; }
};

class PresentationShutdown {
    bool valid_ = rsf_core_fgvk_presentation_shutdown_begin() == RSF_BACKEND_OK;
    bool committed_{};
public:
    PresentationShutdown() = default;
    ~PresentationShutdown() { if (valid_) rsf_core_fgvk_presentation_shutdown_end(committed_); }
    explicit operator bool() const { return valid_; }
    void commit() { committed_ = true; }
    PresentationShutdown(const PresentationShutdown&) = delete;
    PresentationShutdown& operator=(const PresentationShutdown&) = delete;
};
}
