/* SPDX-License-Identifier: MIT */
/* Pinned Unity SDK calls and D3D12 resource-state record marshalling. */
#include "bridge_d3d12.h"

#if RSF_HAVE_UNITY_NATIVE
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <d3d12.h>
#include <dxgi.h>
#define RSF_UNITY_SDK_LEAF rsf_unity_d3d12_sdk
#include "unity_sdk.h"
/* The pinned D3D12 header names these tags without typedefs, which only C++ accepts. */
typedef enum UnityD3D12GraphicsQueueAccess UnityD3D12GraphicsQueueAccess;
typedef struct UnityGraphicsD3D12RecordingState UnityGraphicsD3D12RecordingState;
typedef struct UnityD3D12PluginEventConfig UnityD3D12PluginEventConfig;
#include <IUnityGraphicsD3D12.h>

static IUnityGraphics* graphics = NULL;
static IUnityGraphicsD3D12v7* unity = NULL;

_Static_assert(RSF_UNITY_D3D12_EVENT_SYNC == kUnityD3D12EventConfigFlag_SyncWorkerThreads, "sync flag");
_Static_assert(RSF_UNITY_D3D12_EVENT_FLUSH == kUnityD3D12EventConfigFlag_FlushCommandBuffers, "flush flag");
_Static_assert(RSF_UNITY_D3D12_EVENT_MODIFIES == kUnityD3D12EventConfigFlag_ModifiesCommandBuffersState,
    "modifies flag");
_Static_assert(sizeof(rsf_unity_d3d12_engine) == 7 * sizeof(void*), "engine table");

static void* engine_device(void) { return unity ? unity->GetDevice() : NULL; }
static void* engine_queue(void) { return unity->GetCommandQueue(); }
static void* engine_swapchain(void) { return unity->GetSwapChain(); }
static void* engine_frame_fence(void) { return unity->GetFrameFence(); }
static uint64_t engine_next_fence_value(void) { return unity->GetNextFrameFenceValue(); }

static void* engine_recording_list(void)
{
    UnityGraphicsD3D12RecordingState recording = {0};
    return unity->CommandRecordingState(&recording) ? recording.commandList : NULL;
}

/* bridge_d3d12.rs passes at most the packet's four resources. */
static uint64_t engine_execute(void* list, uint32_t count, void* const* resources, const int32_t* states)
{
    UnityGraphicsD3D12ResourceState native[4] = {0};
    for (uint32_t i = 0; i < count && i < 4; ++i) {
        const D3D12_RESOURCE_STATES state = (D3D12_RESOURCE_STATES)states[i];
        native[i].resource = (ID3D12Resource*)resources[i];
        native[i].expected = state;
        native[i].current = state;
    }
    return unity->ExecuteCommandList((ID3D12GraphicsCommandList*)list, (int)count, native);
}

static const rsf_unity_d3d12_engine engine = {
    engine_device,
    engine_queue,
    engine_swapchain,
    engine_frame_fence,
    engine_next_fence_value,
    engine_recording_list,
    engine_execute,
};

const rsf_unity_d3d12_engine* rsf_unity_d3d12_sdk_engine(void)
{
    return unity ? &engine : NULL;
}

bool rsf_unity_d3d12_sdk_available(void) { return true; }

typedef void* (UNITY_INTERFACE_API* rsf_unity_interface_lookup)(unsigned long long, unsigned long long);

bool rsf_unity_d3d12_sdk_lookup(void* address)
{
    const rsf_unity_interface_lookup lookup = (rsf_unity_interface_lookup)address;
    const UnityInterfaceGUID g = UNITY_GET_INTERFACE_GUID(IUnityGraphics);
    const UnityInterfaceGUID d = UNITY_GET_INTERFACE_GUID(IUnityGraphicsD3D12v7);
    graphics = (IUnityGraphics*)lookup(g.m_GUIDHigh, g.m_GUIDLow);
    unity = (IUnityGraphicsD3D12v7*)lookup(d.m_GUIDHigh, d.m_GUIDLow);
    if (!graphics || !unity || graphics->GetRenderer() != kUnityGfxRendererD3D12) {
        unity = NULL;
        return false;
    }
    return true;
}

void rsf_unity_d3d12_sdk_load(void* pointer, bool package)
{
    IUnityInterfaces* interfaces = (IUnityInterfaces*)pointer;
    if (!interfaces) return;
    graphics = RSF_UNITY_GET(interfaces, IUnityGraphics);
    if (!graphics || graphics->GetRenderer() != kUnityGfxRendererD3D12) return;
    unity = RSF_UNITY_GET(interfaces, IUnityGraphicsD3D12v7);
    if (unity) rsf_unity_d3d12_configure_events(package);
}

void rsf_unity_d3d12_sdk_unload(void)
{
    unity = NULL;
    graphics = NULL;
}

int rsf_unity_d3d12_sdk_reserve_events(void) { return graphics->ReserveEventIDRange(3); }

void rsf_unity_d3d12_sdk_configure_event(int id, bool allow_queue, uint32_t flags, bool bind_texture)
{
    UnityD3D12PluginEventConfig event = {0};
    event.graphicsQueueAccess = allow_queue ? kUnityD3D12GraphicsQueueAccess_Allow
                                            : kUnityD3D12GraphicsQueueAccess_DontCare;
    event.flags = flags;
    event.ensureActiveRenderTextureIsBound = bind_texture;
    unity->ConfigureEvent(id, &event);
}

#ifndef RSF_UNITY_PACKAGE_BRIDGE
void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    rsf_unity_d3d12_sdk_load(interfaces, false);
}

void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginUnload(void)
{
    rsf_unity_frames_activate(0);
    rsf_unity_d3d12_sdk_unload();
}
#endif

#else
const rsf_unity_d3d12_engine* rsf_unity_d3d12_sdk_engine(void) { return NULL; }
bool rsf_unity_d3d12_sdk_available(void) { return false; }
bool rsf_unity_d3d12_sdk_lookup(void* address)
{
    (void)address;
    return false;
}
int rsf_unity_d3d12_sdk_reserve_events(void) { return 0; }
void rsf_unity_d3d12_sdk_configure_event(int id, bool allow_queue, uint32_t flags, bool bind_texture)
{
    (void)id;
    (void)allow_queue;
    (void)flags;
    (void)bind_texture;
}
#endif
