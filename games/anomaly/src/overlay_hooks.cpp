// SPDX-License-Identifier: MIT
// C++ callers supply the shared MinHook operations. The Rust hook owner installs
// the probes, implements the DirectInput/DXGI callbacks and owns the trampolines.
#include "overlay_hooks.h"
#include "native_hook_ops.h"

using rsf::anomaly::HookOps;
using namespace rsf::anomaly::overlay;
extern "C" {
bool rsf_anomaly_overlay_install_input(uintptr_t, const HookOps*, const char**) noexcept;
bool rsf_anomaly_overlay_install_present(decltype(PresentSink::before_present), decltype(PresentSink::resize),
    uintptr_t, const HookOps*, const char**) noexcept;
void rsf_anomaly_overlay_set_input_observer(InputObserver) noexcept;
}

namespace rsf::anomaly::overlay {
void set_input_observer(InputObserver observer) noexcept {
    rsf_anomaly_overlay_set_input_observer(observer);
}

bool install_input_hooks(InputGate& input, const char** reason) noexcept {
    const char* unused = nullptr;
    return rsf_anomaly_overlay_install_input(input.handle(), &hook_ops, reason ? reason : &unused);
}

bool install_present_hooks(const PresentSink& sink, const char** reason) noexcept {
    const char* unused = nullptr;
    return rsf_anomaly_overlay_install_present(sink.before_present, sink.resize,
        reinterpret_cast<uintptr_t>(sink.user), &hook_ops, reason ? reason : &unused);
}
}
