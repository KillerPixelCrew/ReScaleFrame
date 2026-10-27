// SPDX-License-Identifier: GPL-3.0-only
// A deliberately blocked GPU queue fills all UI slots. The fourth frame must
// retain its overlay and safely reuse a slot after the prior work completes.
#include <rescaleframe/overlay_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstdio>
#include <thread>
using Microsoft::WRL::ComPtr;
int main(int argc,char** argv)
{
    if (argc!=2) return 1;
    if (!SetEnvironmentVariableA("RSF_OVERLAY_DLL",argv[1])) return 1;
    ComPtr<ID3D12Debug> debug;
    if (FAILED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) return 77;
    debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;ComPtr<IDXGIAdapter> adapter;ComPtr<ID3D12Device> device;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter))) ||
        FAILED(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)))) return 77;
    D3D12_COMMAND_QUEUE_DESC desc{};ComPtr<ID3D12CommandQueue> queue;
    if (FAILED(device->CreateCommandQueue(&desc,IID_PPV_ARGS(&queue)))) return 1;
    const auto window=CreateWindowW(L"STATIC",L"RSF UI backpressure",WS_OVERLAPPEDWINDOW,0,0,100,100,
        nullptr,nullptr,GetModuleHandleW(nullptr),nullptr);
    if (!window) return 1;
    DXGI_SWAP_CHAIN_DESC1 swap{};swap.Width=80;swap.Height=80;swap.BufferCount=2;
    swap.Format=DXGI_FORMAT_B8G8R8A8_UNORM;swap.SampleDesc.Count=1;
    swap.BufferUsage=DXGI_USAGE_RENDER_TARGET_OUTPUT;swap.SwapEffect=DXGI_SWAP_EFFECT_FLIP_DISCARD;
    ComPtr<IDXGISwapChain1> chain;
    if (FAILED(factory->CreateSwapChainForHwnd(queue.Get(),window,&swap,nullptr,nullptr,&chain))) return 1;
    rsf_overlay_stats stats{};stats.struct_size=sizeof(stats);stats.show_performance_hud=1;
    rsf_overlay_intent intent{};intent.struct_size=sizeof(intent);
    ComPtr<ID3D12Fence> blocked;
    if (FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&blocked))) ||
        FAILED(queue->Wait(blocked.Get(),1))) return 1;
    for (unsigned i=0;i<3;++i)
        if (!rsf_overlay_native12_present(device.Get(),queue.Get(),chain.Get(),&stats,&intent,nullptr,nullptr)) {
            blocked->Signal(1);return 1;
        }
    // This CPU Signal releases only test-owned work. Production waits on the
    // private fence of an overlay list already submitted before Present.
    std::thread release([&]{Sleep(100);blocked->Signal(1);});
    const auto drawn=rsf_overlay_native12_present(device.Get(),queue.Get(),chain.Get(),&stats,&intent,nullptr,nullptr);
    release.join();
    ComPtr<ID3D12Fence> drained;
    if (FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&drained))) ||
        FAILED(queue->Signal(drained.Get(),1))) return 1;
    const auto event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
    if (!event) return 1;
    const auto armed=drained->SetEventOnCompletion(1,event);
    const auto waited=SUCCEEDED(armed)?WaitForSingleObject(event,5000):WAIT_FAILED;CloseHandle(event);
    if (!drawn || waited!=WAIT_OBJECT_0) return 1;
    ComPtr<ID3D12InfoQueue> info;if (FAILED(device.As(&info))) return 1;
    for (UINT64 i=0;i<info->GetNumStoredMessages();++i) {
        SIZE_T size{};info->GetMessage(i,nullptr,&size);
        auto* storage=new unsigned char[size];auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage);
        const auto result=info->GetMessage(i,message,&size);
        const bool bad=SUCCEEDED(result)&&message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR;
        if (bad) std::fprintf(stderr,"%s\n",message->pDescription);
        delete[] storage;if (bad) return 1;
    }
    std::puts("Four UI submissions retained across a full GPU ring; D3D12 validation passed.");
    return 0;
}
