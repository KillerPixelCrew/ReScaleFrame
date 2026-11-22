// SPDX-License-Identifier: MIT
// Pinned Unity SDK calls and D3D12 resource-state record marshalling.
#include "bridge_d3d12.h"

#if RSF_HAVE_UNITY_NATIVE
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#include <array>

namespace {
IUnityGraphics* graphics = nullptr;
IUnityGraphicsD3D12v7* unity = nullptr;

static_assert(RSF_UNITY_D3D12_EVENT_SYNC == kUnityD3D12EventConfigFlag_SyncWorkerThreads);
static_assert(RSF_UNITY_D3D12_EVENT_FLUSH == kUnityD3D12EventConfigFlag_FlushCommandBuffers);
static_assert(RSF_UNITY_D3D12_EVENT_MODIFIES == kUnityD3D12EventConfigFlag_ModifiesCommandBuffersState);
static_assert(sizeof(rsf_unity_d3d12_engine) == 7 * sizeof(void*));
constexpr rsf_unity_d3d12_engine engine{
    []() -> void* { return unity ? unity->GetDevice() : nullptr; },
    []() -> void* { return unity->GetCommandQueue(); },
    []() -> void* { return unity->GetSwapChain(); },
    []() -> void* { return unity->GetFrameFence(); },
    []() -> uint64_t { return unity->GetNextFrameFenceValue(); },
    []() -> void* {
        UnityGraphicsD3D12RecordingState recording{};
        return unity->CommandRecordingState(&recording) ? recording.commandList : nullptr;
    },
    [](void* list, uint32_t count, void* const* resources, const int32_t* states) -> uint64_t {
        std::array<UnityGraphicsD3D12ResourceState, 4> native{};
        for (uint32_t i = 0; i < count; ++i) {
            const auto state = static_cast<D3D12_RESOURCE_STATES>(states[i]);
            native[i] = {static_cast<ID3D12Resource*>(resources[i]), state, state};
        }
        return unity->ExecuteCommandList(static_cast<ID3D12GraphicsCommandList*>(list),
            static_cast<int>(count), native.data());
    },
};
}

extern "C" const rsf_unity_d3d12_engine* rsf_unity_d3d12_sdk_engine(void)
{
    return unity ? &engine : nullptr;
}

extern "C" bool rsf_unity_d3d12_sdk_available(void) { return true; }

extern "C" bool rsf_unity_d3d12_sdk_lookup(void* address)
{
    using Lookup = IUnityInterface* (UNITY_INTERFACE_API*)(uint64_t, uint64_t);
    auto lookup = reinterpret_cast<Lookup>(address);
    const auto g = GetUnityInterfaceGUID<IUnityGraphics>();
    const auto d = GetUnityInterfaceGUID<IUnityGraphicsD3D12v7>();
    graphics = static_cast<IUnityGraphics*>(lookup(g.m_GUIDHigh, g.m_GUIDLow));
    unity = static_cast<IUnityGraphicsD3D12v7*>(lookup(d.m_GUIDHigh, d.m_GUIDLow));
    if (!graphics || !unity || graphics->GetRenderer() != kUnityGfxRendererD3D12) {
        unity = nullptr;
        return false;
    }
    return true;
}
extern "C" void rsf_unity_d3d12_sdk_load(void* pointer, bool package)
{
    auto* interfaces = static_cast<IUnityInterfaces*>(pointer);
    if (!interfaces) return;
    graphics = interfaces->Get<IUnityGraphics>();
    if (!graphics || graphics->GetRenderer() != kUnityGfxRendererD3D12) return;
    unity = interfaces->Get<IUnityGraphicsD3D12v7>();
    if (unity) rsf_unity_d3d12_configure_events(package);
}
extern "C" void rsf_unity_d3d12_sdk_unload(void) { unity = nullptr; graphics = nullptr; }
extern "C" int rsf_unity_d3d12_sdk_reserve_events(void) { return graphics->ReserveEventIDRange(3); }
extern "C" void rsf_unity_d3d12_sdk_configure_event(int id, bool allow_queue, uint32_t flags, bool bind_texture)
{
    UnityD3D12PluginEventConfig event{};
    event.graphicsQueueAccess = allow_queue ? kUnityD3D12GraphicsQueueAccess_Allow : kUnityD3D12GraphicsQueueAccess_DontCare;
    event.flags = flags;
    event.ensureActiveRenderTextureIsBound = bind_texture;
    unity->ConfigureEvent(id, &event);
}

#ifndef RSF_UNITY_PACKAGE_BRIDGE
extern "C" void UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    rsf_unity_d3d12_sdk_load(interfaces, false);
}
extern "C" void UNITY_INTERFACE_API UnityPluginUnload()
{
    rsf_unity_frames_activate(0);
    rsf_unity_d3d12_sdk_unload();
}
#endif

#else
extern "C" const rsf_unity_d3d12_engine* rsf_unity_d3d12_sdk_engine(void) { return nullptr; }
extern "C" bool rsf_unity_d3d12_sdk_available(void) { return false; }
extern "C" bool rsf_unity_d3d12_sdk_lookup(void*) { return false; }
extern "C" int rsf_unity_d3d12_sdk_reserve_events(void) { return 0; }
extern "C" void rsf_unity_d3d12_sdk_configure_event(int, bool, uint32_t, bool) {}
#endif
