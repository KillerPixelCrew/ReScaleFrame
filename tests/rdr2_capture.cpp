// SPDX-License-Identifier: GPL-3.0-only
// Real D3D12 readbacks with simulated NGX execution. This is not RDR2 camera/game acceptance.
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <d3dcompiler.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <rescaleframe/vendor_census.h>
#include "vendor_census_ngx_params.h"
#include "../games/rdr2/src/native_capture.h"
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>
#include <stdexcept>

using Microsoft::WRL::ComPtr;
void rsf_rdr2_request_camera_capture() noexcept {} // No game TLS exists in this fixture.
void rsf_rdr2_capture_test_arm(void*) noexcept;
void rsf_rdr2_capture_test_stop() noexcept;
namespace {
void check(bool value, const char* message) { if (!value) throw std::runtime_error(message); }
void hr(HRESULT value, const char* message) { if (FAILED(value)) { std::printf("HRESULT=%08lx\n",static_cast<unsigned long>(value)); throw std::runtime_error(message); } }
template<class T> struct Payload { D3D12_PIPELINE_STATE_SUBOBJECT_TYPE tag; T value; };
template<class T> union Subobject {
    Payload<T> item;
    void* alignment;
    Subobject(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE tag,T value) : item{tag,value} {}
};
std::string read(const std::filesystem::path& path) {
    std::ifstream file(path,std::ios::binary); return {std::istreambuf_iterator<char>(file),{}};
}
void transition(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES from,D3D12_RESOURCE_STATES to) {
    D3D12_RESOURCE_BARRIER barrier{}; barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,from,to}; list->ResourceBarrier(1,&barrier);
}
}
int main(int argc,char** argv) try {
    setvbuf(stdout,nullptr,_IONBF,0);setvbuf(stderr,nullptr,_IONBF,0);
    check(argc==2 || argc==3,"output directory required");
    const bool rgb10=argc==3 && std::strcmp(argv[2],"rgb10")==0;
    const auto root=std::filesystem::absolute(argv[1]); std::filesystem::create_directories(root);
    check(rsf_rdr2_install_native_capture(root.c_str()),"probe install");
    ComPtr<ID3D12Debug> debug;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    const auto census_dir=(root/L"census").string();
    rsf_census_options options{}; options.struct_size=sizeof(options); options.abi_version=RSF_CENSUS_ABI_VERSION;
    options.output_dir_utf8=census_dir.c_str(); options.flags=RSF_CENSUS_FLAG_NO_STATIC_SCAN|RSF_CENSUS_FLAG_NO_VULKAN;
    check(rsf_census_install(&options)==RSF_CENSUS_OK,"census install");
    ComPtr<IDXGIFactory4> factory; hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)),"factory");
    ComPtr<IDXGIAdapter> adapter; hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)),"WARP");
    ComPtr<ID3D12Device> device; hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)),"device");
    ComPtr<ID3D12InfoQueue> info; device.As(&info);
    D3D12_COMMAND_QUEUE_DESC qdesc{};
    ComPtr<ID3D12CommandQueue> queue; hr(device->CreateCommandQueue(&qdesc,IID_PPV_ARGS(&queue)),"queue");
    const auto window=CreateWindowExW(0,L"STATIC",L"RSF capture fixture",WS_OVERLAPPEDWINDOW,0,0,64,48,nullptr,nullptr,GetModuleHandleW(nullptr),nullptr);
    check(window!=nullptr,"hidden window");
    DXGI_SWAP_CHAIN_DESC1 scdesc{}; scdesc.Width=64; scdesc.Height=48;
    scdesc.Format=rgb10?DXGI_FORMAT_R10G10B10A2_UNORM:DXGI_FORMAT_B8G8R8A8_UNORM;
    scdesc.SampleDesc.Count=1; scdesc.BufferCount=2; scdesc.BufferUsage=DXGI_USAGE_RENDER_TARGET_OUTPUT;
    scdesc.SwapEffect=DXGI_SWAP_EFFECT_FLIP_DISCARD;
    ComPtr<IDXGISwapChain1> chain1; hr(factory->CreateSwapChainForHwnd(queue.Get(),window,&scdesc,nullptr,nullptr,&chain1),"swapchain");
    ComPtr<IDXGISwapChain3> chain; hr(chain1.As(&chain),"swapchain3");
    hr(chain->SetColorSpace1(DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709),"SDR color space");
    rsf_rdr2_capture_test_arm(chain1.Get());
    hr(chain->Present(0,0),"arm Present");
    ComPtr<ID3D12Resource> back; hr(chain->GetBuffer(chain->GetCurrentBackBufferIndex(),IID_PPV_ARGS(&back)),"backbuffer");
    D3D12_DESCRIPTOR_HEAP_DESC heaps{}; heaps.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV; heaps.NumDescriptors=4;
    ComPtr<ID3D12DescriptorHeap> rtvs; hr(device->CreateDescriptorHeap(&heaps,IID_PPV_ARGS(&rtvs)),"RTV heap");
    const auto rtv_start=rtvs->GetCPUDescriptorHandleForHeapStart();
    const auto rtv_step=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    auto rtv=[&](UINT n){return D3D12_CPU_DESCRIPTOR_HANDLE{rtv_start.ptr+SIZE_T(n)*rtv_step};};
    device->CreateRenderTargetView(back.Get(),nullptr,rtv(0));
    auto texture=[&](DXGI_FORMAT format,D3D12_RESOURCE_FLAGS flags,D3D12_RESOURCE_STATES state){
        D3D12_RESOURCE_DESC desc{}; desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D; desc.Width=32;desc.Height=24;
        desc.DepthOrArraySize=1;desc.MipLevels=1;desc.Format=format;desc.SampleDesc.Count=1;desc.Flags=flags;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        ComPtr<ID3D12Resource> result;hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(&result)),"input texture");return result;
    };
    auto color=texture(DXGI_FORMAT_R16G16B16A16_FLOAT,D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,D3D12_RESOURCE_STATE_RENDER_TARGET);
    auto motion=texture(DXGI_FORMAT_R16G16_FLOAT,D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,D3D12_RESOURCE_STATE_RENDER_TARGET);
    auto output=texture(DXGI_FORMAT_R16G16B16A16_FLOAT,D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET|D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_RENDER_TARGET);
    auto depth=texture(DXGI_FORMAT_R32G8X24_TYPELESS,D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,D3D12_RESOURCE_STATE_DEPTH_WRITE);
    device->CreateRenderTargetView(color.Get(),nullptr,rtv(1));device->CreateRenderTargetView(motion.Get(),nullptr,rtv(2));device->CreateRenderTargetView(output.Get(),nullptr,rtv(3));
    heaps.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;heaps.NumDescriptors=1;
    ComPtr<ID3D12DescriptorHeap> dsvs;hr(device->CreateDescriptorHeap(&heaps,IID_PPV_ARGS(&dsvs)),"DSV heap");
    D3D12_DEPTH_STENCIL_VIEW_DESC dsv{};dsv.Format=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;dsv.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2D;
    device->CreateDepthStencilView(depth.Get(),&dsv,dsvs->GetCPUDescriptorHandleForHeapStart());
    const char shader[]="float4 vs(uint id:SV_VertexID):SV_Position { float2 p=float2((id<<1)&2,id&2); return float4(p*float2(2,-2)+float2(-1,1),0,1); } float4 ps():SV_Target { return float4(0.25,0.5,0.75,1); }";
    ComPtr<ID3DBlob> vs,ps,errors;
    hr(D3DCompile(shader,sizeof(shader),nullptr,nullptr,nullptr,"vs","vs_5_0",0,0,&vs,&errors),"VS compile");
    hr(D3DCompile(shader,sizeof(shader),nullptr,nullptr,nullptr,"ps","ps_5_0",0,0,&ps,&errors),"PS compile");
    D3D12_ROOT_SIGNATURE_DESC rootdesc{};ComPtr<ID3DBlob> rootblob;
    hr(D3D12SerializeRootSignature(&rootdesc,D3D_ROOT_SIGNATURE_VERSION_1,&rootblob,&errors),"root serialize");
    ComPtr<ID3D12RootSignature> signature;hr(device->CreateRootSignature(0,rootblob->GetBufferPointer(),rootblob->GetBufferSize(),IID_PPV_ARGS(&signature)),"root signature");
    D3D12_BLEND_DESC blend{};blend.RenderTarget[0].RenderTargetWriteMask=D3D12_COLOR_WRITE_ENABLE_ALL;
    D3D12_RT_FORMAT_ARRAY formats{};formats.NumRenderTargets=1;formats.RTFormats[0]=scdesc.Format;
    D3D12_RASTERIZER_DESC raster{};raster.FillMode=D3D12_FILL_MODE_SOLID;raster.CullMode=D3D12_CULL_MODE_NONE;raster.DepthClipEnable=TRUE;
    struct Stream {
        Subobject<ID3D12RootSignature*> root;
        Subobject<D3D12_SHADER_BYTECODE> vs,ps;
        Subobject<D3D12_BLEND_DESC> blend;
        Subobject<D3D12_RT_FORMAT_ARRAY> formats;
        Subobject<D3D12_RASTERIZER_DESC> raster;
        Subobject<D3D12_DEPTH_STENCIL_DESC1> depth;
        Subobject<D3D12_PRIMITIVE_TOPOLOGY_TYPE> topology;
    } stream{{D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,signature.Get()},
        {D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS,{vs->GetBufferPointer(),vs->GetBufferSize()}},
        {D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS,{ps->GetBufferPointer(),ps->GetBufferSize()}},
        {D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND,blend},{D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS,formats},
        {D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER,raster},{D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1,{}},
        {D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY,D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE}};
    ComPtr<ID3D12Device2> device2;hr(device.As(&device2),"device2");
    D3D12_PIPELINE_STATE_STREAM_DESC streamdesc{sizeof(stream),&stream};ComPtr<ID3D12PipelineState> pso;
    hr(device2->CreatePipelineState(&streamdesc,IID_PPV_ARGS(&pso)),"stream PSO");
    ComPtr<ID3D12CommandAllocator> allocator;hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)),"allocator");
    ComPtr<ID3D12GraphicsCommandList> list;hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),pso.Get(),IID_PPV_ARGS(&list)),"list");
    const float c[]{0.25f,0.5f,0.75f,1},mv[]{0.125f,-0.25f,0,0};
    list->ClearRenderTargetView(rtv(1),c,0,nullptr);list->ClearRenderTargetView(rtv(2),mv,0,nullptr);list->ClearRenderTargetView(rtv(3),c,0,nullptr);
    list->ClearDepthStencilView(dsvs->GetCPUDescriptorHandleForHeapStart(),D3D12_CLEAR_FLAG_DEPTH|D3D12_CLEAR_FLAG_STENCIL,0.25f,0,0,nullptr);
    transition(list.Get(),color.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    transition(list.Get(),motion.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    transition(list.Get(),depth.Get(),D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    transition(list.Get(),output.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    wchar_t exe[MAX_PATH]{};GetModuleFileNameW(nullptr,exe,MAX_PATH);
    const auto ngx=LoadLibraryW((std::filesystem::path(exe).parent_path()/L"_nvngx.dll").c_str());check(ngx!=nullptr,"NGX fixture load");
    auto get=reinterpret_cast<int(*)(NgxParam**)>(GetProcAddress(ngx,"NVSDK_NGX_D3D12_GetParameters"));
    auto create=reinterpret_cast<int(*)(void*,unsigned,NgxParam*,void**)>(GetProcAddress(ngx,"NVSDK_NGX_D3D12_CreateFeature"));
    auto evaluate=reinterpret_cast<int(*)(void*,void*,NgxParam*,void*)>(GetProcAddress(ngx,"NVSDK_NGX_D3D12_EvaluateFeature"));
    check(get&&create&&evaluate,"NGX fixture exports");NgxParam* params{};check(get(&params)==1,"NGX params");
    params->Set("DLSS.Feature.Create.Flags",1|2|8);void* handle{};check(create(list.Get(),1,params,&handle)==1,"NGX create");
    params->Set("Color",color.Get());params->Set("Depth",depth.Get());params->Set("MotionVectors",motion.Get());params->Set("Output",output.Get());
    params->Set("Jitter.Offset.X",0.25f);params->Set("Jitter.Offset.Y",-0.3888889f);params->Set("MV.Scale.X",1.f);params->Set("MV.Scale.Y",1.f);
    params->Set("DLSS.Render.Subrect.Dimensions.Width",32u);params->Set("DLSS.Render.Subrect.Dimensions.Height",24u);params->Set("Reset",0);
    for (unsigned i=0;i<4;++i)check(evaluate(list.Get(),handle,params,nullptr)==1,"NGX evaluate");
    transition(list.Get(),back.Get(),D3D12_RESOURCE_STATE_PRESENT,D3D12_RESOURCE_STATE_RENDER_TARGET);
    const auto back_rtv=rtv(0);list->OMSetRenderTargets(1,&back_rtv,FALSE,nullptr);list->SetGraphicsRootSignature(signature.Get());
    D3D12_VIEWPORT viewport{0,0,64,48,0,1};D3D12_RECT scissor{0,0,64,48};list->RSSetViewports(1,&viewport);list->RSSetScissorRects(1,&scissor);
    list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    for (unsigned i=0;i<75;++i)list->DrawInstanced(3,1,0,0);
    transition(list.Get(),back.Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PRESENT);
    hr(list->Close(),"Close with restored resource states");ID3D12CommandList* submitted[]{list.Get()};queue->ExecuteCommandLists(1,submitted);
    ComPtr<ID3D12CommandAllocator> reused;hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&reused)),"reuse allocator");
    hr(list->Reset(reused.Get(),pso.Get()),"command-list reuse after submission");hr(list->Close(),"reused list close");
    hr(chain->Present(0,0),"captured Present");
    // The probe's copy fence precedes DXGI's later presentation work. The fixture owns teardown
    // and must also drain that work before releasing the swapchain's buffers.
    ComPtr<ID3D12Fence> drained;hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&drained)),"teardown fence");
    const auto done=CreateEventW(nullptr,FALSE,FALSE,nullptr);check(done!=nullptr,"teardown event");
    hr(queue->Signal(drained.Get(),1),"teardown Signal");hr(drained->SetEventOnCompletion(1,done),"teardown completion");
    const auto wait=WaitForSingleObject(done,5000);CloseHandle(done);check(wait==WAIT_OBJECT_0,"presentation work drained");
    const auto manifest=root/(L"bundle-"+std::to_wstring(GetCurrentProcessId())+L"-001.json");
    for (unsigned i=0;i<100 && !std::filesystem::exists(manifest);++i)Sleep(100);
    const auto document=read(manifest);check(!document.empty(),"capture manifest written");
    std::printf("Checking capture manifest and raw pixels.\n");
    check(document.find("\"draw_count\":75")!=std::string::npos,"full draw sequence");
    check(document.find("\"index\":3")!=std::string::npos,"four temporal input groups");
    check(document.find("\"write_failed\":false")!=std::string::npos,"complete writes");
    check(document.find("\"capture_timeout\":false")!=std::string::npos,"capture completed without timeout");
    check(document.find("\"color_space_set\":true")!=std::string::npos,"observed color space");
    check(document.find("\"ps_crc32\":\"00000000\"")==std::string::npos,"stream shader identity captured");
    const auto prefix=L"native-"+std::to_wstring(GetCurrentProcessId())+L"-001-";
    const auto final=read(root/(prefix+L"074.bin"));check(final.size()==256u*48u,"final draw retained past 64");
    if(rgb10){uint32_t packed{};std::memcpy(&packed,final.data(),sizeof(packed));
        check((packed&1023)==256 && ((packed>>10)&1023)==512 && ((packed>>20)&1023)==767 && (packed>>30)==3,"actual final RGB10 pixels");}
    else check(static_cast<unsigned char>(final[0])==191 && static_cast<unsigned char>(final[1])==128 && static_cast<unsigned char>(final[2])==64,"actual final BGRA pixels");
    const auto inputprefix=L"input-"+std::to_wstring(GetCurrentProcessId())+L"-001-000-";
    const auto motionbytes=read(root/(inputprefix+L"motion.bin"));check(motionbytes.size()==256u*24u,"motion readback dimensions");
    check(static_cast<unsigned char>(motionbytes[0])==0 && static_cast<unsigned char>(motionbytes[1])==0x30 &&
        static_cast<unsigned char>(motionbytes[2])==0 && static_cast<unsigned char>(motionbytes[3])==0xb4,"raw RG16F motion preserved");
    const auto depthbytes=read(root/(inputprefix+L"depth.bin"));check(depthbytes.size()>=256u*24u,"depth plane readback");
    float depthvalue{};std::memcpy(&depthvalue,depthbytes.data(),sizeof(depthvalue));check(depthvalue==0.25f,"raw depth preserved");
    std::printf("Checking D3D12 debug messages.\n");
    if(info)for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T size{};info->GetMessage(i,nullptr,&size);std::vector<unsigned char> bytes(size);
        auto* message=reinterpret_cast<D3D12_MESSAGE*>(bytes.data());hr(info->GetMessage(i,message,&size),"debug message");
        if(message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::printf("D3D12 error: %s\n",message->pDescription);throw std::runtime_error("D3D12 debug validation");}}
    std::printf("RDR2 capture fixture passed: 75 draws, tail readback, stream PSO, 4 depth/MV/color/output groups; no game-camera claim.\n");
    rsf_rdr2_capture_test_stop();
    check(rsf_census_shutdown()==RSF_CENSUS_OK,"census shutdown");
    DestroyWindow(window);return 0;
} catch(const std::exception& error){std::fprintf(stderr,"RDR2 capture fixture failed: %s\n",error.what());return 1;}
