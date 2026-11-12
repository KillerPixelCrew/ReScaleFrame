// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/graphics/src/sharpening/d3d12.h"
#include "../runtime/backends/common/d3d12_helpers.h"
#include <dxgi1_4.h>
#include <d3d12sdklayers.h>
#include <wrl/client.h>
#include <array>
#include <vector>
#include <cstdio>
#include <cstring>
#include <cmath>
using Microsoft::WRL::ComPtr;
using namespace rsf::sharpening;
namespace {
constexpr uint32_t width=33,height=17,count=width*height;
using Image=std::array<float,count*4>;
int failures=0;
void check(bool value,const char* label) { if (!value) { ++failures; std::printf("FAIL: %s\n",label); } }
struct Fixture {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12CommandAllocator> allocators[3];
    ComPtr<ID3D12GraphicsCommandList> lists[3];
    std::vector<ComPtr<ID3D12Resource>> retained[3];
    HANDLE event=nullptr; uint64_t tick=0;
    D3D12 pass;
    ~Fixture() { if (event) CloseHandle(event); }
    bool start() {
        ComPtr<ID3D12Debug> debug;
        if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
        ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter> warp;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp))) ||
            FAILED(D3D12CreateDevice(warp.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)))) return false;
        D3D12_COMMAND_QUEUE_DESC q{};
        if (FAILED(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue))) ||
            FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))) return false;
        event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
        for (unsigned i=0;i<3;++i)
            if (!rsf::create_command_list(device.Get(),D3D12_COMMAND_LIST_TYPE_DIRECT,&allocators[i],&lists[i])) return false;
        return event!=nullptr;
    }
    bool begin(unsigned slot) {
        retained[slot].clear();
        return SUCCEEDED(allocators[slot]->Reset()) && SUCCEEDED(lists[slot]->Reset(allocators[slot].Get(),nullptr));
    }
    bool submit(unsigned slots) {
        ID3D12CommandList* commands[]={lists[0].Get(),lists[1].Get(),lists[2].Get()};
        for (unsigned i=0;i<slots;++i) if (FAILED(lists[i]->Close())) return false;
        queue->ExecuteCommandLists(slots,commands);
        return SUCCEEDED(queue->Signal(fence.Get(),++tick)) && SUCCEEDED(rsf::wait_fence(fence.Get(),tick,event));
    }
    ComPtr<ID3D12Resource> buffer(uint64_t size,D3D12_HEAP_TYPE type) {
        D3D12_HEAP_PROPERTIES heap{}; heap.Type=type;
        D3D12_RESOURCE_DESC desc{}; desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER; desc.Width=size;
        desc.Height=1; desc.DepthOrArraySize=1; desc.MipLevels=1; desc.SampleDesc.Count=1; desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> result;
        check(SUCCEEDED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
            type==D3D12_HEAP_TYPE_READBACK?D3D12_RESOURCE_STATE_COPY_DEST:D3D12_RESOURCE_STATE_GENERIC_READ,
            nullptr,IID_PPV_ARGS(&result))),"buffer allocation"); return result;
    }
    ComPtr<ID3D12Resource> texture(unsigned slot,uint32_t w,uint32_t h,DXGI_FORMAT format,
        const void* pixels,uint32_t stride,D3D12_RESOURCE_STATES state) {
        ComPtr<ID3D12Resource> result;
        if (!rsf::create_uav_texture(device.Get(),w,h,format,&result)) { check(false,"texture allocation"); return result; }
        const auto desc=result->GetDesc(); D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; uint64_t size=0;
        device->GetCopyableFootprints(&desc,0,1,0,&fp,nullptr,nullptr,&size);
        auto upload=buffer(size,D3D12_HEAP_TYPE_UPLOAD); void* mapped=nullptr; D3D12_RANGE no_read{0,0};
        if (!upload || FAILED(upload->Map(0,&no_read,&mapped))) { check(false,"upload map"); return {}; }
        for (uint32_t y=0;y<h;++y) std::memcpy(static_cast<char*>(mapped)+y*fp.Footprint.RowPitch,
            static_cast<const char*>(pixels)+y*w*stride,w*stride);
        upload->Unmap(0,nullptr); auto* list=lists[slot].Get();
        rsf::transition(list,result.Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_DEST);
        D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource=upload.Get(); source.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; source.PlacedFootprint=fp;
        D3D12_TEXTURE_COPY_LOCATION target{}; target.pResource=result.Get(); target.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&target,0,0,0,&source,nullptr);
        rsf::transition(list,result.Get(),D3D12_RESOURCE_STATE_COPY_DEST,state);
        retained[slot].push_back(upload); retained[slot].push_back(result); return result;
    }
    struct Readback { ComPtr<ID3D12Resource> data; D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{}; };
    Readback capture(unsigned slot,ID3D12Resource* texture) {
        Readback out; uint64_t size=0; auto d=texture->GetDesc();
        device->GetCopyableFootprints(&d,0,1,0,&out.fp,nullptr,nullptr,&size); out.data=buffer(size,D3D12_HEAP_TYPE_READBACK);
        auto* list=lists[slot].Get(); rsf::transition(list,texture,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION source{}; source.pResource=texture; source.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        D3D12_TEXTURE_COPY_LOCATION target{}; target.pResource=out.data.Get(); target.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT; target.PlacedFootprint=out.fp;
        list->CopyTextureRegion(&target,0,0,0,&source,nullptr);
        rsf::transition(list,texture,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS); return out;
    }
    Image read(const Readback& out) {
        Image result{}; void* mapped=nullptr;
        if (!out.data || FAILED(out.data->Map(0,nullptr,&mapped))) { check(false,"readback map"); return result; }
        for (uint32_t y=0;y<height;++y) std::memcpy(result.data()+y*width*4,
            static_cast<char*>(mapped)+y*out.fp.Footprint.RowPitch,width*16);
        D3D12_RANGE no_write{0,0}; out.data->Unmap(0,&no_write); return result;
    }
    Readback record(unsigned slot,const Image& image,Options options,bool depth=true,bool hdr=true) {
        check(begin(slot),"begin command slot");
        auto output=texture(slot,width,height,DXGI_FORMAT_R32G32B32A32_FLOAT,image.data(),16,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        std::array<float,count*2> motion{}; std::array<float,count> z{}; z.fill(0.5f);
        const float exposure=2;
        auto mv=texture(slot,width,height,DXGI_FORMAT_R32G32_FLOAT,motion.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        auto depth_image=texture(slot,width,height,DXGI_FORMAT_R32_FLOAT,z.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        auto e=texture(slot,1,1,DXGI_FORMAT_R32_FLOAT,&exposure,4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        Frame frame{}; frame.hdr=hdr; frame.depth_inverted=1;
        check(pass.run(lists[slot].Get(),output.Get(),mv.Get(),depth?depth_image.Get():nullptr,e.Get(),options,frame,slot),"sharpen recording");
        return capture(slot,output.Get());
    }
    Image run(const Image& image,Options options,bool depth=true,bool hdr=true) {
        auto recorded=record(0,image,options,depth,hdr); check(submit(1),"submit and retire"); return read(recorded);
    }
};
Image flat(float value) {
    Image image{}; for (uint32_t i=0;i<count;++i) { for (uint32_t c=0;c<3;++c) image[i*4+c]=value; image[i*4+3]=float(i)/count; } return image;
}
Image edge(bool hdr=true) {
    auto image=flat(0);
    for (uint32_t y=0;y<height;++y) for (uint32_t x=0;x<width;++x) {
        const float value=hdr?(x<width/2?10.0f+0.2f*x:80.0f+0.2f*x):(x<width/2?0.1f+0.002f*x:0.8f+0.002f*x);
        for (uint32_t c=0;c<3;++c) image[(y*width+x)*4+c]=value;
    }
    return image;
}
}
int main() {
    D3D12 disabled; Options options;
    check(disabled.run(nullptr,nullptr,nullptr,nullptr,nullptr,options,{},0) && !disabled.dispatches(),"Off needs no device or GPU work");
    Fixture f; if (!f.start()) return 77;
    auto source=edge(); auto before=f.pass.dispatches(); auto off=f.run(source,options);
    check(!std::memcmp(source.data(),off.data(),sizeof(source)) && f.pass.dispatches()==before,"Off bit identical and no dispatch");
    options.mode=Mode::vendor; options.sharpness=1;
    auto vendor=f.run(source,options); check(vendor==source && f.pass.dispatches()==before,"vendor mode adds no custom pass");
    options.mode=Mode::rcas; options.sharpness=0;
    auto zero=f.run(source,options); check(zero==source && f.pass.dispatches()==before,"zero strength no dispatch");
    for (Mode mode:{Mode::rcas,Mode::depth_rcas,Mode::depth_das}) {
        options=Options{}; options.mode=mode; options.sharpness=1;
        const auto constant=flat(10); check(f.run(constant,options)==constant,"flat HDR byte identity and alpha");
        const auto actual=f.run(source,options); bool alpha=true,bounded=true,changed=false;
        for (uint32_t i=0;i<count;++i) {
            alpha &= actual[i*4+3]==source[i*4+3];
            for (uint32_t c=0;c<3;++c) { const float value=actual[i*4+c];
                float lower=source[i*4+c],upper=lower;
                for (int dy=-1;dy<=1;++dy) for (int dx=-1;dx<=1;++dx) {
                    const auto y=std::clamp(static_cast<int>(i/width)+dy,0,static_cast<int>(height)-1);
                    const auto x=std::clamp(static_cast<int>(i%width)+dx,0,static_cast<int>(width)-1);
                    const float sample=source[(static_cast<uint32_t>(y)*width+static_cast<uint32_t>(x))*4+c];
                    lower=std::min(lower,sample); upper=std::max(upper,sample);
                }
                bounded &= std::isfinite(value) && value>=lower-1e-3f && value<=upper+1e-3f;
                changed |= std::fabs(value-source[i*4+c])>1e-3f;
            }
        }
        check(alpha && bounded && changed,"HDR edge sharpened/bounded, alpha retained");
        const auto sdr=edge(false); const auto sdr_result=f.run(sdr,options,true,false);
        check(sdr_result!=sdr,"SDR edge sharpened without HDR encoding");
        check(f.run(flat(1),options,true,false)==flat(1),"SDR white flat");
        const auto fallback=f.run(source,options,false);
        Options plain=options; plain.mode=Mode::rcas;
        check(fallback==f.run(source,plain,false),"missing depth falls back to RCAS");
    }
    // Three recordings before any submit: no slot may see a later frame's constants/descriptors.
    std::array<Fixture::Readback,3> records;
    for (uint32_t slot=0;slot<3;++slot) { options.mode=Mode::rcas; options.sharpness=0.2f+0.4f*slot; records[slot]=f.record(slot,source,options); }
    check(f.submit(3),"submit three in-flight slots");
    const auto a=f.read(records[0]),b=f.read(records[1]),c=f.read(records[2]);
    double da=0,db=0,dc=0;
    for (uint32_t i=0;i<count*4;++i) if (i%4!=3) { da+=std::fabs(a[i]-source[i]); db+=std::fabs(b[i]-source[i]); dc+=std::fabs(c[i]-source[i]); }
    check(da>0 && db>da && dc>db,"per-slot constants and monotonic edge response");
    ComPtr<ID3D12InfoQueue> validation;
    if (SUCCEEDED(f.device.As(&validation))) {
        const auto messages=validation->GetNumStoredMessagesAllowedByRetrievalFilter();
        for (uint64_t i=0;i<messages;++i) {
            SIZE_T bytes=0; validation->GetMessage(i,nullptr,&bytes); std::vector<uint8_t> storage(bytes);
            auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage.data());
            if (SUCCEEDED(validation->GetMessage(i,message,&bytes)) && message->Severity<=D3D12_MESSAGE_SEVERITY_WARNING) {
                std::printf("D3D12 validation: %s\n",message->pDescription); check(false,"D3D12 validation warning/error");
            }
        }
        std::printf("D3D12 debug validation inspected %llu messages\n",static_cast<unsigned long long>(messages));
    }
    std::printf("sharpening_d3d12: %d failures, %llu dispatches, edge deltas %.6f %.6f %.6f\n",failures,
        static_cast<unsigned long long>(f.pass.dispatches()),da,db,dc);
    return failures?1:0;
}
