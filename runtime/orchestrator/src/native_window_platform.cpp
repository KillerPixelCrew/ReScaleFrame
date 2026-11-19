// SPDX-License-Identifier: MIT
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include <unknwn.h>
#include <wrl/client.h>

using CompareComIdentity = int (*)(void*, void*);
extern "C" int rsf_core_native_window_match(void*, rsf_game_render_pass*, CompareComIdentity);
extern "C" int rsf_core_native_window_present(const rsf_observer_present_event*, CompareComIdentity,
    decltype(&rsf_native_scene_for_window), decltype(&rsf_native_scene_present_path));

namespace {
int same_com_identity(void* observed, void* owned)
{
    Microsoft::WRL::ComPtr<IUnknown> observed_identity, owned_identity;
    return SUCCEEDED(static_cast<IUnknown*>(observed)->QueryInterface(IID_PPV_ARGS(&observed_identity))) &&
        SUCCEEDED(static_cast<IUnknown*>(owned)->QueryInterface(IID_PPV_ARGS(&owned_identity))) &&
        observed_identity.Get() == owned_identity.Get();
}
}

// Pass the platform callbacks at the native boundary so CPU-only users of the
// Rust archive do not acquire graphics linkage.
extern "C" RSF_RUNTIME_API int rsf_native_window_match(void* swapchain, rsf_game_render_pass* pass)
{
    return rsf_core_native_window_match(swapchain, pass, same_com_identity);
}

extern "C" RSF_RUNTIME_API int rsf_native_window_present(const rsf_observer_present_event* event)
{
    return rsf_core_native_window_present(event, same_com_identity,
        rsf_native_scene_for_window, rsf_native_scene_present_path);
}
