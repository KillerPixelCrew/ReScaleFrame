// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_vulkan.h>
#include <mutex>
namespace rsf::latency {
// Context lifetime only. No SDK or host mutex is held for the scoped operation.
struct PresentationBinding {
    std::mutex mutex;
    rsf_fgvk* context{};
    unsigned borrowers{};
    bool retiring{}, destroying{};
};
inline PresentationBinding& presentation_binding() { static PresentationBinding state; return state; }
class PresentationLease {
    rsf_fgvk* context_{};
    bool closing_{};
public:
    PresentationLease() {
        auto& state=presentation_binding(); std::lock_guard lock(state.mutex);
        closing_=state.retiring;
        if (!closing_ && state.context) {context_=state.context; ++state.borrowers;}
    }
    ~PresentationLease() {
        if(context_) {auto& state=presentation_binding();std::lock_guard lock(state.mutex);--state.borrowers;}
    }
    PresentationLease(const PresentationLease&)=delete;
    PresentationLease& operator=(const PresentationLease&)=delete;
    rsf_fgvk* get() const {return context_;}
    bool closing() const {return closing_;}
};
inline rsf_backend_result presentation_attach(rsf_fgvk* context) {
    if(!context) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& state=presentation_binding();std::lock_guard lock(state.mutex);
    if(state.retiring || (state.context && state.context!=context)) return RSF_BACKEND_ERROR_NOT_READY;
    state.context=context;return RSF_BACKEND_OK;
}
// Exact-owner retirement stops new borrows immediately and never waits for a
// sleeping CPU borrower that may itself need the host's Present lock to finish.
inline rsf_backend_result presentation_retire(rsf_fgvk* context) {
    auto& state=presentation_binding();std::lock_guard lock(state.mutex);
    if(state.context!=context || !context) return RSF_BACKEND_OK;
    state.retiring=state.destroying=true;
    return state.borrowers ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK;
}
inline void presentation_release(rsf_fgvk* context) {
    auto& state=presentation_binding();std::lock_guard lock(state.mutex);
    if(state.context==context && !state.borrowers) {state.context=nullptr;state.retiring=state.destroying=false;}
}
inline rsf_backend_result presentation_unbind() {
    auto& state=presentation_binding();std::lock_guard lock(state.mutex);
    if(!state.context) return RSF_BACKEND_OK;
    state.retiring=state.destroying=true;
    if(state.borrowers) return RSF_BACKEND_ERROR_NOT_READY;
    state.context=nullptr;state.retiring=state.destroying=false;return RSF_BACKEND_OK;
}
inline rsf_backend_result presentation_drain_begin(rsf_fgvk* context) {
    auto& state=presentation_binding();std::lock_guard lock(state.mutex);
    if(state.context!=context || !context) return RSF_BACKEND_OK;
    state.retiring=true;
    return state.borrowers ? RSF_BACKEND_ERROR_NOT_READY : RSF_BACKEND_OK;
}
inline void presentation_drain_end(rsf_fgvk* context) {
    auto& state=presentation_binding();std::lock_guard lock(state.mutex);
    if(state.context==context && !state.destroying) state.retiring=false;
}

// Checked runtime shutdown retains the binding if SDK shutdown fails. The
// control borrow also prevents a concurrent provider destroy during that call.
class PresentationShutdown {
    bool valid_{}, committed_{}, previous_retiring_{}, previous_destroying_{};
public:
    PresentationShutdown() {
        auto& state=presentation_binding();std::lock_guard lock(state.mutex);
        previous_retiring_=state.retiring;previous_destroying_=state.destroying;
        state.retiring=true;
        if(state.borrowers) return;
        ++state.borrowers;valid_=true;
    }
    ~PresentationShutdown() {
        if(!valid_) return;
        auto& state=presentation_binding();std::lock_guard lock(state.mutex);--state.borrowers;
        if(committed_) {state.context=nullptr;state.retiring=state.destroying=false;}
        else if(!state.destroying || previous_destroying_) state.retiring=previous_retiring_;
    }
    explicit operator bool() const {return valid_;}
    void commit() {committed_=true;}
    PresentationShutdown(const PresentationShutdown&)=delete;
    PresentationShutdown& operator=(const PresentationShutdown&)=delete;
};

}
