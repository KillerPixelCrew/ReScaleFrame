// SPDX-License-Identifier: MIT
#pragma once
#include <d3d12.h>
#include <rescaleframe/unity_bridge.h>
#include <IUnityInterface.h>
#include <wrl/client.h>
void rsf_uvk_preload(IUnityInterfaces*);
void rsf_uvk_load(IUnityInterfaces*, int event_id);
void rsf_uvk_unload() noexcept;
bool rsf_uvk_render(int event_id, const rsf_unity_packet&, const rsf_game_host_services&, bool reconfigure);
bool rsf_uvk_create_window();
bool rsf_uvk_update_window(uint32_t width, uint32_t height);
bool rsf_uvk_owns_window(void*);
bool rsf_uvk_graphics(Microsoft::WRL::ComPtr<ID3D12Device>& device,
    Microsoft::WRL::ComPtr<ID3D12CommandQueue>& queue) noexcept;
