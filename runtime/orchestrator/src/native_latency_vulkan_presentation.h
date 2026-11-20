// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

extern "C" {
rsf_backend_result rsf_core_fgvk_presentation_attach(rsf_fgvk*);
rsf_backend_result rsf_core_fgvk_presentation_unbind();
rsf_fgvk* rsf_core_fgvk_presentation_borrow(uint32_t* closing);
void rsf_core_fgvk_presentation_return();
rsf_backend_result rsf_core_fgvk_presentation_shutdown_begin();
void rsf_core_fgvk_presentation_shutdown_end(uint32_t success);
}

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
