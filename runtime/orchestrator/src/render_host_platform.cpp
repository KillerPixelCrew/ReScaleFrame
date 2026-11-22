// SPDX-License-Identifier: MIT
#include "render_host_foreign.h"
#include <bit>

namespace sl { enum class Result; }

using StreamlineNativeInterface = sl::Result (*)(void*, void**);

// The engine owns the Streamline DLL. Keep any foreign C++ exception on
// this side of the Rust call and let the host retain the original interface.
extern "C" int32_t rsf_render_host_call_native_interface(
    rsf_render_host_native_interface_fn entry, void* proxy, void** native) noexcept
{
    try {
        return static_cast<int32_t>(std::bit_cast<StreamlineNativeInterface>(entry)(proxy, native));
    } catch (...) {
        return -1;
    }
}
