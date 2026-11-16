// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/orchestrator/src/auxiliary_sr.h"
#include "../runtime/orchestrator/src/native_regions.h"
#include <wrl/client.h>
#include <array>
#include <vector>
#include <cmath>
#include <algorithm>
#include <cstring>
#include <cstdio>
#include <limits>

namespace {
using Microsoft::WRL::ComPtr;
int failures{};
void check(bool value, const char* label) { if (!value) { ++failures; std::fprintf(stderr, "FAIL %s\n", label); } }
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, DXGI_FORMAT format, const void* pixels, UINT stride, UINT bind,
    D3D11_USAGE usage = D3D11_USAGE_DEFAULT) {
    D3D11_TEXTURE2D_DESC d{}; d.Width = 8; d.Height = 6; d.Format = format; d.MipLevels = d.ArraySize = d.SampleDesc.Count = 1;
    d.BindFlags = bind; d.Usage = usage; const D3D11_SUBRESOURCE_DATA data{pixels, 8 * stride, 0};
    ComPtr<ID3D11Texture2D> result; check(SUCCEEDED(device->CreateTexture2D(&d, &data, &result)), "fixture texture"); return result;
}
std::vector<unsigned char> read(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* source, UINT stride) {
    D3D11_TEXTURE2D_DESC d{}; source->GetDesc(&d); d.Usage = D3D11_USAGE_STAGING; d.BindFlags = d.MiscFlags = 0; d.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging; if (FAILED(device->CreateTexture2D(&d, nullptr, &staging))) return {};
    context->CopyResource(staging.Get(), source); D3D11_MAPPED_SUBRESOURCE m{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &m))) return {};
    std::vector<unsigned char> bytes(size_t(d.Width) * d.Height * stride);
    for (UINT y = 0; y < d.Height; ++y) std::memcpy(bytes.data() + size_t(y) * d.Width * stride,
        static_cast<const unsigned char*>(m.pData) + size_t(y) * m.RowPitch, size_t(d.Width) * stride);
    context->Unmap(staging.Get(), 0); return bytes;
}
}
int main() {
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context; const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, &level, 1, D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    std::array<uint32_t, 48> depths{}, pixels{}; std::array<float, 192> colour{}; colour.fill(.25f);
    for (uint32_t i = 0; i < depths.size(); ++i) {
        depths[i] = (((i * 17) & 255) << 24) | ((i * 237113) & 0xffffff);
        pixels[i] = i | ((i * 3) << 8) | ((i * 5) << 16) | ((255 - i * 2) << 24);
    }
    auto depth = texture(device.Get(), DXGI_FORMAT_R24G8_TYPELESS, depths.data(), 4, D3D11_BIND_DEPTH_STENCIL);
    auto aux = texture(device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, pixels.data(), 4, D3D11_BIND_RENDER_TARGET);
    auto main = texture(device.Get(), DXGI_FORMAT_R32G32B32A32_FLOAT, colour.data(), 16, D3D11_BIND_RENDER_TARGET);
    auto immutable = texture(device.Get(), DXGI_FORMAT_R32G32B32A32_FLOAT, colour.data(), 16, D3D11_BIND_SHADER_RESOURCE, D3D11_USAGE_IMMUTABLE);
    if (!depth || !aux || !main || !immutable) return 1;
    const auto depth_before = read(device.Get(), context.Get(), depth.Get(), 4), aux_before = read(device.Get(), context.Get(), aux.Get(), 4);
    const auto colour_before = read(device.Get(), context.Get(), main.Get(), 16);
    rsf_game_render_pass pass{}; pass.color_output = main.Get(); pass.session_id = 1; pass.native_frame = 7; pass.view_key = 91;
    std::array<rsf_d3d11_auxiliary_surface, 2> surfaces{};
    surfaces[0] = {sizeof(surfaces[0]), RSF_D3D11_AUXILIARY_DEPTH_STENCIL, depth.Get(), {0,0,4,3}, {0,0,8,6}, {.25f,-.125f}};
    surfaces[1] = {sizeof(surfaces[1]), RSF_D3D11_AUXILIARY_COLOUR, aux.Get(), {.8f,1.2f,5.75f,4.8f}, {1,0,8,6}, {.35f,-.4f}};
    surfaces[0].flags = surfaces[1].flags = RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
    const rsf_d3d11_auxiliary_pass request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION, &pass, 2, surfaces.data()};
    AuxiliarySurfaces transaction;
    const D3D11_VIEWPORT viewport{2,1,3,4,0,1}; context->RSSetViewports(1, &viewport);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
    check(transaction.reserve(context.Get(), pass, request, nullptr, nullptr), "reserve depth and fractional colour allocations");
    check(read(device.Get(),context.Get(),depth.Get(),4)==depth_before && read(device.Get(),context.Get(),main.Get(),16)==colour_before,
        "allocation-only auxiliary reserve leaves native contents untouched");
    D3D11_VIEWPORT reserve_saved{}; UINT reserve_count=1; context->RSGetViewports(&reserve_count,&reserve_saved);
    check(!std::memcmp(&reserve_saved,&viewport,sizeof(viewport)),"reserve does not alter native viewport bindings");
    check(transaction.seal(), "seal depth and fractional colour region using reserved allocations");
    check(transaction.pending() && !transaction.prepare(context.Get(), pass, request, nullptr, nullptr), "overlap refused without cancelling the lease");
    check(read(device.Get(), context.Get(), depth.Get(), 4) == depth_before && read(device.Get(), context.Get(), aux.Get(), 4) == aux_before &&
        read(device.Get(), context.Get(), main.Get(), 16) == colour_before, "preparation leaves every native resource untouched");
    D3D11_VIEWPORT saved{}; UINT count = 1; context->RSGetViewports(&count, &saved);
    D3D11_PRIMITIVE_TOPOLOGY topology{}; context->IAGetPrimitiveTopology(&topology);
    check(!std::memcmp(&saved, &viewport, sizeof(saved)) && topology == D3D11_PRIMITIVE_TOPOLOGY_POINTLIST, "preparation restores native bindings");
    rsf_d3d11_auxiliary_scope scope{sizeof(scope), RSF_D3D11_HOST_ABI_VERSION, 1, 7, 91};
    check(transaction.matches(scope), "lease identity matches"); ++scope.native_frame;
    check(!transaction.matches(scope), "different native frame refused"); --scope.native_frame;
    ComPtr<ID3D11RenderTargetView> target; check(SUCCEEDED(device->CreateRenderTargetView(main.Get(), nullptr, &target)), "SR destination RTV");
    const float reconstructed[]{7,3,2,.5f}; context->ClearRenderTargetView(target.Get(), reconstructed);
    check(transaction.commit(), "commit complete SR auxiliaries");
    const auto scaled_depth = read(device.Get(), context.Get(), depth.Get(), 4), scaled_aux = read(device.Get(), context.Get(), aux.Get(), 4);
    check(scaled_depth.size() == 192 && scaled_aux.size() == 192, "committed readbacks");
    if (scaled_depth.size() == 192 && scaled_aux.size() == 192) for (int y = 0; y < 6; ++y) for (int x = 0; x < 8; ++x) {
        const int dx = std::clamp(int(std::floor((float(x)+.5f)/8*4-.25f)), 0, 3);
        const int dy = std::clamp(int(std::floor((float(y)+.5f)/6*3+.125f)), 0, 2);
        uint32_t actual_depth{}, actual_aux{};
        std::memcpy(&actual_depth, scaled_depth.data() + (y*8+x)*4, 4); std::memcpy(&actual_aux, scaled_aux.data() + (y*8+x)*4, 4);
        check(actual_depth == depths[size_t(dy)*8+dx], "depth and stencil point mapping");
        uint32_t expected = pixels[size_t(y)*8+x];
        if (x >= 1) {
            const int sx = std::clamp(int(std::floor(.8f+(float(x)+.5f-1)/7*4.95f-.35f)), 1, 5);
            const int sy = std::clamp(int(std::floor(1.2f+(float(y)+.5f)/6*3.6f+.4f)), 1, 4);
            expected = pixels[size_t(sy)*8+sx];
        }
        check(actual_aux == expected, "fractional viewport, jitter, alpha and outside pixels");
    }
    const auto sr_colour = read(device.Get(), context.Get(), main.Get(), 16);
    check(transaction.restore() && !transaction.pending(), "restore lease after final consumer");
    check(read(device.Get(), context.Get(), depth.Get(), 4) == depth_before && read(device.Get(), context.Get(), aux.Get(), 4) == aux_before,
        "every original auxiliary restored bit for bit");
    check(read(device.Get(), context.Get(), main.Get(), 16) == sr_colour, "successful SR colour survives auxiliary restoration");
    check(transaction.prepare(context.Get(), pass, request, nullptr, nullptr), "prepare failed-SR rollback");
    const float failed_colour[]{1,0,0,0}; context->ClearRenderTargetView(target.Get(), failed_colour);
    check(transaction.restore() && read(device.Get(), context.Get(), main.Get(), 16) == sr_colour, "failed SR restores original colour");
    pass.color_output_readable = immutable.Get();
    check(!transaction.prepare(context.Get(), pass, request, nullptr, nullptr) &&
        read(device.Get(), context.Get(), main.Get(), 16) == sr_colour, "invalid second destination cannot copy an uninitialized snapshot over native colour");
    pass.color_output_readable = nullptr; surfaces[1].flags = 0;
    check(transaction.prepare(context.Get(), pass, request, nullptr, nullptr) && transaction.commit(), "prepare current-frame auxiliary retirement");
    ComPtr<ID3D11RenderTargetView> aux_target; device->CreateRenderTargetView(aux.Get(), nullptr, &aux_target);
    const float cleared[]{0,0,0,0}; context->ClearRenderTargetView(aux_target.Get(), cleared);
    check(transaction.restore() && read(device.Get(), context.Get(), aux.Get(), 4) == std::vector<unsigned char>(192),
        "later native consumer clear survives auxiliary retirement");
    // Packed positive finite HDR channels must retain numeric nearest semantics.
    // These are normal unsigned-float values, not colour-space declarations.
    std::array<uint32_t,48> hdr_pixels{};
    for (uint32_t i=0;i<hdr_pixels.size();++i) {
        const uint32_t r=(((i%9)+11)<<6) | ((i*7)&63);
        const uint32_t g=(((i%7)+12)<<6) | ((i*11)&63);
        const uint32_t b=(((i%5)+13)<<5) | ((i*3)&31);
        hdr_pixels[i]=r | (g<<11) | (b<<22);
    }
    UINT hdr_support{};
    constexpr UINT hdr_required=D3D11_FORMAT_SUPPORT_TEXTURE2D | D3D11_FORMAT_SUPPORT_SHADER_LOAD |
        D3D11_FORMAT_SUPPORT_RENDER_TARGET;
    const bool hdr_capable=SUCCEEDED(device->CheckFormatSupport(DXGI_FORMAT_R11G11B10_FLOAT,&hdr_support)) &&
        (hdr_support&hdr_required)==hdr_required;
    check(hdr_capable,"WARP supports numeric R11G11B10 texture load and render target");
    auto hdr=texture(device.Get(),DXGI_FORMAT_R11G11B10_FLOAT,hdr_pixels.data(),4,D3D11_BIND_RENDER_TARGET);
    if (!hdr) return 1;
    D3D11_TEXTURE2D_DESC hdr_desc{}; hdr->GetDesc(&hdr_desc);
    check(hdr_desc.Format==DXGI_FORMAT_R11G11B10_FLOAT && hdr_desc.Width==8 && hdr_desc.Height==6 &&
        hdr_desc.SampleDesc.Count==1,"actual packed HDR source descriptor");
    const auto hdr_before=read(device.Get(),context.Get(),hdr.Get(),4);
    const auto hdr_main_before=read(device.Get(),context.Get(),main.Get(),16);
    rsf_d3d11_auxiliary_surface hdr_surface{sizeof(hdr_surface),RSF_D3D11_AUXILIARY_COLOUR,hdr.Get(),
        {1.25f,.75f,5.5f,4.75f},{2,1,8,6},{.25f,-.125f}};
    hdr_surface.flags=RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
    const rsf_d3d11_auxiliary_pass hdr_request{sizeof(hdr_request),RSF_D3D11_HOST_ABI_VERSION,&pass,1,&hdr_surface};
    const bool hdr_prepared=transaction.prepare(context.Get(),pass,hdr_request,nullptr,nullptr);
    check(hdr_prepared==hdr_capable,"packed HDR admission follows actual load/RTV capability");
    if (hdr_prepared) {
        check(read(device.Get(),context.Get(),hdr.Get(),4)==hdr_before &&
            read(device.Get(),context.Get(),main.Get(),16)==hdr_main_before,"packed HDR prepare leaves engine resources untouched");
        context->ClearRenderTargetView(target.Get(),reconstructed);
        check(transaction.commit(),"packed HDR nearest transaction commits");
        const auto expanded=read(device.Get(),context.Get(),hdr.Get(),4);
        check(expanded.size()==192,"packed HDR expanded readback");
        const auto channel=[](uint32_t packed,unsigned shift,unsigned mantissa) {
            const uint32_t code=packed>>shift;
            return std::ldexp(1.f+float(code&((1u<<mantissa)-1))/float(1u<<mantissa),int((code>>mantissa)&31)-15);
        };
        for (int y=0;y<6;++y) for (int x=0;x<8;++x) if (expanded.size()==192) {
            uint32_t expected=hdr_pixels[size_t(y)*8+x], actual{};
            if (x>=2 && y>=1) {
                const int sx=std::clamp(int(std::floor(1.25f+(float(x)+.5f-2)/6*4.25f-.25f)),1,4);
                const int sy=std::clamp(int(std::floor(.75f+(float(y)+.5f-1)/5*4+.125f)),1,4);
                expected=hdr_pixels[size_t(sy)*8+sx];
            }
            std::memcpy(&actual,expanded.data()+(size_t(y)*8+x)*4,4);
            check(actual==expected,"packed HDR nearest crop/expand preserves exact channels and outside texels");
            for (unsigned component=0;component<3;++component) {
                const unsigned shift=component==0 ? 0u : component==1 ? 11u : 22u;
                const unsigned mantissa=component==2 ? 5u : 6u;
                const float value=channel(actual,shift,mantissa);
                check(std::isfinite(value) && value>0 && value==channel(expected,shift,mantissa),
                    "positive finite R11G11B10 values retain native numeric magnitude");
            }
        }
        const auto hdr_sr_colour=read(device.Get(),context.Get(),main.Get(),16);
        check(transaction.restore() && !transaction.pending() && read(device.Get(),context.Get(),hdr.Get(),4)==hdr_before,
            "packed HDR RESTORE_CONTENTS restores every native source bit");
        check(read(device.Get(),context.Get(),main.Get(),16)==hdr_sr_colour,"packed HDR restore retains accepted SR colour");
        check(transaction.prepare(context.Get(),pass,hdr_request,nullptr,nullptr),"packed HDR prepare failed-SR rollback");
        context->ClearRenderTargetView(target.Get(),failed_colour);
        check(transaction.restore() && !transaction.pending() && read(device.Get(),context.Get(),main.Get(),16)==hdr_sr_colour &&
            read(device.Get(),context.Get(),hdr.Get(),4)==hdr_before,"failed SR cancels packed HDR transaction and restores native output");
        pass.color_output_readable=immutable.Get();
        check(!transaction.prepare(context.Get(),pass,hdr_request,nullptr,nullptr) && !transaction.pending() &&
            read(device.Get(),context.Get(),main.Get(),16)==hdr_sr_colour && read(device.Get(),context.Get(),hdr.Get(),4)==hdr_before,
            "packed HDR preparation failure preserves all source/destination bytes and clears transaction");
        pass.color_output_readable=nullptr;
        hdr_surface.flags=0;
        check(transaction.prepare(context.Get(),pass,hdr_request,nullptr,nullptr) && transaction.commit(),
            "packed HDR without RESTORE_CONTENTS commits");
        ComPtr<ID3D11RenderTargetView> hdr_target;
        check(SUCCEEDED(device->CreateRenderTargetView(hdr.Get(),nullptr,&hdr_target)),"packed HDR native consumer RTV");
        if (hdr_target) context->ClearRenderTargetView(hdr_target.Get(),cleared);
        check(transaction.restore() && read(device.Get(),context.Get(),hdr.Get(),4)==std::vector<unsigned char>(192),
            "packed HDR later native clear survives non-restoring retirement");
    } else check(!transaction.pending() && read(device.Get(),context.Get(),hdr.Get(),4)==hdr_before &&
        read(device.Get(),context.Get(),main.Get(),16)==hdr_main_before,"unsupported packed HDR leaves transaction and native contents unchanged");
    // Recovery uses one immutable snapshot across an abandoned temporal mapping and
    // the integer raw-consumer mapping. No temporal camera is manufactured.
    {
        std::array<float,192> values{};
        std::array<uint32_t,48> vectors{};
        for (size_t i=0;i<48;++i) {
            values[i*4]=float(i+1); values[i*4+1]=float(i+2); values[i*4+2]=float(i+3); values[i*4+3]=1;
            vectors[i]=uint32_t(0x3000+i) | (uint32_t(0xb000+i)<<16);
        }
        auto scene=texture(device.Get(),DXGI_FORMAT_R32G32B32A32_FLOAT,values.data(),16,D3D11_BIND_RENDER_TARGET);
        auto raw_motion=texture(device.Get(),DXGI_FORMAT_R16G16_FLOAT,vectors.data(),4,D3D11_BIND_RENDER_TARGET);
        auto native_depth=texture(device.Get(),DXGI_FORMAT_R24G8_TYPELESS,depths.data(),4,D3D11_BIND_DEPTH_STENCIL);
        auto content=texture(device.Get(),DXGI_FORMAT_R8G8B8A8_UNORM,pixels.data(),4,D3D11_BIND_RENDER_TARGET);
        if (!scene || !raw_motion || !native_depth || !content) return 1;
        rsf_game_render_pass raster{}; raster.color_input=raster.color_output=scene.Get();
        raster.session_id=1; raster.native_frame=81; raster.view_key=91;
        std::array<rsf_d3d11_auxiliary_surface,4> raw{};
        raw[0]={sizeof(raw[0]),RSF_D3D11_AUXILIARY_DEPTH_STENCIL,native_depth.Get(),{1,1,5,4},{1,0,8,6},{0,0}};
        raw[1]={sizeof(raw[1]),RSF_D3D11_AUXILIARY_COLOUR,raw_motion.Get(),{1,1,5,4},{1,0,8,6},{0,0}};
        raw[2]={sizeof(raw[2]),RSF_D3D11_AUXILIARY_COLOUR,content.Get(),{1,1,5,4},{1,0,8,6},{0,0}};
        raw[2].flags=RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
        raw[3]={sizeof(raw[3]),RSF_D3D11_AUXILIARY_COLOUR,scene.Get(),{1,1,5,4},{1,0,8,6},{0,0}};
        const rsf_d3d11_auxiliary_pass recovery{sizeof(recovery),RSF_D3D11_HOST_ABI_VERSION,&raster,4,raw.data()};
        AuxiliarySurfaces pending;
        const auto original_scene=read(device.Get(),context.Get(),scene.Get(),16);
        const auto original_content=read(device.Get(),context.Get(),content.Get(),4);
        check(pending.reserve(context.Get(),raster,recovery,nullptr,nullptr,true),"recovery preflight includes Main and raw consumers without camera");
        check(read(device.Get(),context.Get(),scene.Get(),16)==original_scene,"reserve never snapshots or writes native content");
        check(pending.cancel() && !pending.pending(),"unused reservation cancels before producer");
        check(pending.reserve(context.Get(),raster,recovery,nullptr,nullptr,true),"second recovery reservation");
        for (float& v : values) v*=2;
        context->UpdateSubresource(scene.Get(),0,nullptr,values.data(),8*16,0);
        check(pending.seal(),"seal observes actual native production after allocation-only reserve");
        check(!pending.cancel(),"sealed producer cannot cancel its output obligation");
        auto fractional=raw; fractional[1].source_rect[0]=.8f; fractional[1].jitter_pixels[0]=.25f;
        const rsf_d3d11_auxiliary_pass temporal_mapping{sizeof(temporal_mapping),RSF_D3D11_HOST_ABI_VERSION,&raster,3,fractional.data()};
        check(pending.temporal_prepare(temporal_mapping),"actual temporal fractional mapping reuses sealed originals");
        // Simulate a later native Main write only after sealing, without corrupting recovery source.
        ComPtr<ID3D11RenderTargetView> scene_target, content_target;
        check(SUCCEEDED(device->CreateRenderTargetView(scene.Get(),nullptr,&scene_target)) &&
            SUCCEEDED(device->CreateRenderTargetView(content.Get(),nullptr,&content_target)),"recovery consumer views");
        const float overwritten[]{99,98,97,1}; context->ClearRenderTargetView(scene_target.Get(),overwritten);
        check(pending.spatial_prepare() && pending.commit(),"failed/skipped temporal recovers from immutable original products");
        auto output_scene=read(device.Get(),context.Get(),scene.Get(),16);
        auto output_depth=read(device.Get(),context.Get(),native_depth.Get(),4);
        auto output_motion=read(device.Get(),context.Get(),raw_motion.Get(),4);
        for (int y=0;y<6;++y) for (int x=0;x<8;++x) {
            const int sx=x ? std::clamp(int(std::floor(1+(float(x)+.5f-1)/7*4)),1,4) : x;
            const int sy=x ? std::clamp(int(std::floor(1+(float(y)+.5f)/6*3)),1,3) : y;
            const size_t dst=size_t(y)*8+size_t(x), src=size_t(sy)*8+size_t(sx);
            check(output_scene.size()==768 && !std::memcmp(output_scene.data()+dst*16,values.data()+src*4,16),
                "Main nearest crop mapping and untouched outside output rectangle use sealed source");
            uint32_t actual_depth{},actual_motion{};
            if (output_depth.size()==192) std::memcpy(&actual_depth,output_depth.data()+dst*4,4);
            if (output_motion.size()==192) std::memcpy(&actual_motion,output_motion.data()+dst*4,4);
            check(output_depth.size()==192 && actual_depth==depths[src],"raw D24 partition and all stencil bits retained");
            check(output_motion.size()==192 && actual_motion==vectors[src],"raw motion stored values not rescaled or replaced");
        }
        context->ClearRenderTargetView(content_target.Get(),overwritten);
        ComPtr<ID3D11RenderTargetView> motion_target;
        ComPtr<ID3D11DepthStencilView> native_depth_target;
        D3D11_DEPTH_STENCIL_VIEW_DESC ds{}; ds.Format=DXGI_FORMAT_D24_UNORM_S8_UINT; ds.ViewDimension=D3D11_DSV_DIMENSION_TEXTURE2D;
        check(SUCCEEDED(device->CreateRenderTargetView(raw_motion.Get(),nullptr,&motion_target)) &&
            SUCCEEDED(device->CreateDepthStencilView(native_depth.Get(),&ds,&native_depth_target)),"later raw consumer views");
        context->ClearRenderTargetView(motion_target.Get(),overwritten);
        context->ClearDepthStencilView(native_depth_target.Get(),D3D11_CLEAR_DEPTH|D3D11_CLEAR_STENCIL,.2f,213);
        const auto later_depth=read(device.Get(),context.Get(),native_depth.Get(),4);
        const auto later_motion=read(device.Get(),context.Get(),raw_motion.Get(),4);
        check(pending.restore() && !pending.pending(),"explicit content obligations drain");
        check(read(device.Get(),context.Get(),content.Get(),4)==original_content &&
            read(device.Get(),context.Get(),scene.Get(),16)==output_scene &&
            read(device.Get(),context.Get(),raw_motion.Get(),4)==later_motion &&
            read(device.Get(),context.Get(),native_depth.Get(),4)==later_depth,
            "restore-content consumer restores original while accepted Main and flags-zero later contents survive");
        auto malformed=raw; malformed[0].source_rect[2]=9;
        const rsf_d3d11_auxiliary_pass invalid{sizeof(invalid),RSF_D3D11_HOST_ABI_VERSION,&raster,4,malformed.data()};
        check(!pending.reserve(context.Get(),raster,invalid,nullptr,nullptr,true) && !pending.pending() &&
            read(device.Get(),context.Get(),scene.Get(),16)==output_scene,"failed complete preflight cannot mutate Main");
    }
    if (hdr_capable) {
        auto native_main=texture(device.Get(),DXGI_FORMAT_R11G11B10_FLOAT,hdr_pixels.data(),4,D3D11_BIND_RENDER_TARGET);
        if (!native_main) return 1;
        D3D11_TEXTURE2D_DESC native_desc{}; native_main->GetDesc(&native_desc);
        check(native_desc.Format==DXGI_FORMAT_R11G11B10_FLOAT,"recovery uses actual packed numeric Main descriptor");
        rsf_game_render_pass owner{}; owner.session_id=1; owner.native_frame=82; owner.view_key=91;
        owner.color_input=owner.color_output=native_main.Get();
        rsf_d3d11_auxiliary_surface colour_only{sizeof(colour_only),RSF_D3D11_AUXILIARY_COLOUR,native_main.Get(),{1,1,5,4},{1,0,8,6},{0,0}};
        const rsf_d3d11_auxiliary_pass inputs{sizeof(inputs),RSF_D3D11_HOST_ABI_VERSION,&owner,1,&colour_only};
        AuxiliarySurfaces recovery;
        check(recovery.reserve(context.Get(),owner,inputs,nullptr,nullptr,true) && recovery.seal() && recovery.commit(),
            "packed numeric Main reconstructs without camera or temporal inputs");
        const auto output=read(device.Get(),context.Get(),native_main.Get(),4);
        for (int y=0;y<6;++y) for (int x=0;x<8;++x) {
            const int sx=x ? std::clamp(int(std::floor(1+(float(x)+.5f-1)/7*4)),1,4) : x;
            const int sy=x ? std::clamp(int(std::floor(1+(float(y)+.5f)/6*3)),1,3) : y;
            uint32_t actual{}; if (output.size()==192) std::memcpy(&actual,output.data()+(size_t(y)*8+size_t(x))*4,4);
            check(output.size()==192 && actual==hdr_pixels[size_t(sy)*8+size_t(sx)],"packed Main exact positive numeric nearest and outside pixels");
        }
        check(recovery.restore() && read(device.Get(),context.Get(),native_main.Get(),4)==output,"retirement never restores successful packed Main");
    }
    // A non-origin rectangle in a two-channel UNORM mask must become local R32,
    // with adapter weights and existing independent reactive/bias hints preserved.
    std::array<uint8_t, 96> mask_values{};
    for (size_t i = 0; i < 48; ++i) {
        mask_values[i*2] = uint8_t((i*19)%256);
        mask_values[i*2+1] = uint8_t((i*7)%256);
    }
    auto temporal = texture(device.Get(), DXGI_FORMAT_R8G8_UNORM, mask_values.data(), 2, 0);
    if (!temporal) return 1;
    const int32_t mask_rect[]{2,1,6,4}; const float mask_weights[]{.1f,1,0,0};
    ID3D11Texture2D *reactive = nullptr, *bias = nullptr;
    const bool normalized = rsf_native_regions_temporal_mask(context.Get(), temporal.Get(), mask_rect, mask_weights,
        nullptr, nullptr, &reactive, &bias);
    check(normalized, "normalize native temporal mask");
    if (normalized) {
        D3D11_TEXTURE2D_DESC d{}; reactive->GetDesc(&d);
        check(d.Width == 4 && d.Height == 3 && d.Format == DXGI_FORMAT_R32_FLOAT, "mask render-local R32 format and size");
        auto values = read(device.Get(), context.Get(), reactive, 4);
        check(values == read(device.Get(), context.Get(), bias, 4) && values.size() == 48, "same explicit hint feeds reactive and bias");
        for (size_t y = 0; y < 3; ++y) for (size_t x = 0; x < 4; ++x) {
            const auto native = ((y+1)*8+x+2)*2;
            const float expected = std::min(1.f, .1f*float(mask_values[native])/255.f+float(mask_values[native+1])/255.f);
            float value{};
            if (values.size() == 48) std::memcpy(&value, values.data()+(y*4+x)*4, 4);
            check(std::abs(value-expected) < 1e-5f, "mask channels and crop origin have native numeric semantics");
        }
        // Independent hints must not disappear when an explicit mask is added.
        D3D11_TEXTURE2D_DESC scalar{}; scalar.Width = 4; scalar.Height = 3;
        scalar.MipLevels = scalar.ArraySize = scalar.SampleDesc.Count = 1;
        scalar.Format = DXGI_FORMAT_R32_FLOAT; scalar.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        std::array<float,12> previous_reactive{}, previous_bias{}; previous_reactive.fill(.8f); previous_bias.fill(.6f);
        const D3D11_SUBRESOURCE_DATA rdata{previous_reactive.data(),16,0}, bdata{previous_bias.data(),16,0};
        ComPtr<ID3D11Texture2D> existing_reactive, existing_bias;
        check(SUCCEEDED(device->CreateTexture2D(&scalar,&rdata,&existing_reactive)) &&
            SUCCEEDED(device->CreateTexture2D(&scalar,&bdata,&existing_bias)), "existing temporal hint fixtures");
        const bool combined = rsf_native_regions_temporal_mask(context.Get(), temporal.Get(), mask_rect, mask_weights,
            existing_reactive.Get(),existing_bias.Get(), &reactive,&bias);
        check(combined, "compose explicit and existing temporal hints");
        if (combined) {
            const auto combined_reactive = read(device.Get(),context.Get(),reactive,4);
            const auto combined_bias = read(device.Get(),context.Get(),bias,4);
            check(combined_reactive.size() == 48 && combined_bias.size() == 48, "combined hints readable");
            for (size_t i = 0; i < 12; ++i) if (values.size() == 48 && combined_reactive.size() == 48 && combined_bias.size() == 48) {
                float original{}, rvalue{}, bvalue{};
                std::memcpy(&original,values.data()+i*4,4); std::memcpy(&rvalue,combined_reactive.data()+i*4,4);
                std::memcpy(&bvalue,combined_bias.data()+i*4,4);
                check(std::abs(rvalue-std::max(original,.8f)) < 1e-5f && std::abs(bvalue-std::max(original,.6f)) < 1e-5f,
                    "existing reactive and bias compose independently by maximum");
            }
        }
    }
    const int32_t outside[]{2,1,9,4};
    check(!rsf_native_regions_temporal_mask(context.Get(),temporal.Get(),outside,mask_weights,nullptr,nullptr,&reactive,&bias),
        "out-of-resource temporal mask rectangle refused");
    ComPtr<ID3D11Device> other_device;
    const bool second_device = SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0,
        &level, 1, D3D11_SDK_VERSION, &other_device, nullptr, nullptr));
    check(second_device, "second device for foreign temporal resource checks");
    if (second_device) {
        auto foreign_mask = texture(other_device.Get(), DXGI_FORMAT_R8G8_UNORM, mask_values.data(), 2, 0);
        D3D11_TEXTURE2D_DESC scalar{}; scalar.Width = 4; scalar.Height = 3;
        scalar.MipLevels = scalar.ArraySize = scalar.SampleDesc.Count = 1;
        scalar.Format = DXGI_FORMAT_R32_FLOAT; scalar.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        ComPtr<ID3D11Texture2D> foreign_hint;
        check(SUCCEEDED(other_device->CreateTexture2D(&scalar,nullptr,&foreign_hint)), "foreign scalar hint fixture");
        auto* bound = aux_target.Get(); context->OMSetRenderTargets(1,&bound,nullptr);
        auto* saved_reactive = reactive; auto* saved_bias = bias;
        if (foreign_mask && foreign_hint) {
            check(!rsf_native_regions_temporal_mask(context.Get(),foreign_mask.Get(),mask_rect,mask_weights,
                nullptr,nullptr,&reactive,&bias), "foreign non-SRV mask refused before copy");
            check(!rsf_native_regions_temporal_mask(context.Get(),temporal.Get(),mask_rect,mask_weights,
                foreign_hint.Get(),nullptr,&reactive,&bias), "foreign reactive hint refused");
            check(!rsf_native_regions_temporal_mask(context.Get(),temporal.Get(),mask_rect,mask_weights,
                nullptr,foreign_hint.Get(),&reactive,&bias), "foreign bias hint refused");
        }
        ComPtr<ID3D11RenderTargetView> retained; context->OMGetRenderTargets(1,&retained,nullptr);
        check(retained.Get() == bound && reactive == saved_reactive && bias == saved_bias,
            "foreign resources leave bindings and output pointers unchanged");
        context->OMSetRenderTargets(0,nullptr,nullptr);
    }
    // Depth domains reuse the production crop/normalization path, including an
    // output-only D24 allocation. No vendor runtime is needed for these values.
    std::array<uint32_t,48> domain_pixels{}, packed_vectors{};
    const uint32_t codes[]{0,32767,32768,65535,32769,65534,16384,49152};
    for (size_t i=0;i<48;++i) domain_pixels[i]=codes[(i+2*(i/8)+6)%8];
    // R16 UNORM makes the split's exact decoded boundary observable.
    std::array<uint16_t,48> depth16{};
    for (size_t i=0;i<48;++i) depth16[i]=uint16_t(domain_pixels[i]);
    auto region_depth=texture(device.Get(),DXGI_FORMAT_R16_TYPELESS,depth16.data(),2,D3D11_BIND_DEPTH_STENCIL);
    auto region_motion=texture(device.Get(),DXGI_FORMAT_R16G16_FLOAT,packed_vectors.data(),4,D3D11_BIND_SHADER_RESOURCE);
    if (!region_depth || !region_motion) return 1;
    const auto region_before=read(device.Get(),context.Get(),region_depth.Get(),2);
    const int32_t region_rect[]{2,1,6,5};
    rsf_game_depth_domains domains{}; domains.struct_size=sizeof(domains); domains.version=RSF_DEPTH_DOMAINS_VERSION;
    domains.domain_count=2; domains.clear_enabled=1; domains.clear_value=1;
    const float split=float(32768)/65535.f;
    for (unsigned i=0;i<2;++i) {
        auto& d=domains.domains[i];
        d.classify_min=d.viewport_min_depth=i ? split : 0;
        d.classify_max=d.viewport_max_depth=i ? 1.f : split;
        d.endpoint_flags=i ? RSF_DEPTH_DOMAIN_INCLUDE_MAX : 3u;
        d.viewport_xywh[2]=8; d.viewport_xywh[3]=6;
        d.source_clip_to_target_clip[0]=d.source_clip_to_target_clip[5]=d.source_clip_to_target_clip[10]=d.source_clip_to_target_clip[15]=1;
    }
    // X changes deliberately: this operation writes target Z/W at the same sample.
    domains.domains[0].source_clip_to_target_clip[12]=.3f;
    domains.domains[0].source_clip_to_target_clip[10]=.5f;
    domains.domains[0].source_clip_to_target_clip[11]=.5f;
    domains.domains[1].source_clip_to_target_clip[10]=2;
    domains.domains[1].source_clip_to_target_clip[14]=-.5f;
    ID3D11Texture2D *local_color{}, *local_depth{}, *local_motion{};
    const auto normalize=[&](const rsf_game_depth_domains& request, ID3D11Texture2D* source=nullptr) {
        return rsf_native_regions_prepare(context.Get(),immutable.Get(),source ? source : region_depth.Get(),
            region_motion.Get(),region_rect,&local_color,&local_depth,&local_motion,&request,false);
    };
    const bool mapped=region_depth && region_motion && normalize(domains);
    check(mapped,"normalize exhaustive UNORM depth domains without readback in production");
    if (mapped) {
        D3D11_TEXTURE2D_DESC d{}; local_depth->GetDesc(&d);
        check(d.Width==4 && d.Height==4 && d.Format==DXGI_FORMAT_R32_FLOAT,"domain depth crop is render-local R32");
        const auto values=read(device.Get(),context.Get(),local_depth,4);
        check(values.size()==64,"mapped depth readback");
        for (size_t y=0;y<4;++y) for (size_t x=0;x<4;++x) {
            const double stored=double(depth16[(y+1)*8+x+2])/65535;
            double expected=1;
            if (stored!=1) {
                const bool first_domain= float(stored)<=split;
                const auto& domain=domains.domains[first_domain ? 0 : 1];
                const double z=(stored-domain.viewport_min_depth)/(domain.viewport_max_depth-domain.viewport_min_depth);
                expected=first_domain ? (.5*z)/(1+.5*z) : std::clamp(2*z-.5,0.0,1.0);
            }
            float actual{}; if (values.size()==64) std::memcpy(&actual,values.data()+(y*4+x)*4,4);
            check(std::abs(actual-expected)<2e-5,"UNORM split, offset crop, homogeneous divide, changed XY and explicit clipping");
        }
        auto boundary_clear=domains; boundary_clear.clear_value=split;
        boundary_clear.domains[0].endpoint_flags=RSF_DEPTH_DOMAIN_INCLUDE_MIN;
        check(normalize(boundary_clear),"explicit clear owns otherwise excluded shared endpoint");
        const auto clear_values=read(device.Get(),context.Get(),local_depth,4);
        for (size_t y=0;y<4;++y) for (size_t x=0;x<4;++x) if (depth16[(y+1)*8+x+2]==32768) {
            float actual{}; if (clear_values.size()==64) std::memcpy(&actual,clear_values.data()+(y*4+x)*4,4);
            check(clear_values.size()==64 && actual==1,"clear takes precedence over homogeneous projection");
        }
        auto fractional=domains;
        for (auto& domain:fractional.domains) {
            domain.viewport_xywh[0]=2.25f; domain.viewport_xywh[1]=1.25f;
            domain.viewport_xywh[2]=domain.viewport_xywh[3]=3.5f;
        }
        fractional.domains[0].source_clip_to_target_clip[2]=.1f;
        fractional.domains[0].source_clip_to_target_clip[6]=.1f;
        check(normalize(fractional),"fractional viewport covers sample centers without covering outer texel edges");
        const auto fractional_values=read(device.Get(),context.Get(),local_depth,4);
        for (size_t y=0;y<4;++y) for (size_t x=0;x<4;++x) {
            const double stored=double(depth16[(y+1)*8+x+2])/65535;
            const bool first_domain=float(stored)<=split;
            const double z=first_domain ? stored/split : (stored-split)/(1-split);
            const double nx=2*(double(x)+2.5-2.25)/3.5-1;
            const double ny=1-2*(double(y)+1.5-1.25)/3.5;
            const double expected=stored==1 ? 1 : first_domain ? std::clamp((.5*z+.1*nx+.1*ny)/(1+.5*z),0.0,1.0) : std::clamp(2*z-.5,0.0,1.0);
            float actual{}; if (fractional_values.size()==64) std::memcpy(&actual,fractional_values.data()+(y*4+x)*4,4);
            check(fractional_values.size()==64 && std::abs(actual-expected)<2e-5,"fractional viewport unmaps actual sample-center NDC");
        }
        auto uncovered=fractional; uncovered.domains[0].viewport_xywh[2]=3.25f;
        check(!normalize(uncovered),"viewport right edge at last sample center is uncovered");
        uncovered=fractional; uncovered.domains[0].viewport_xywh[3]=3.25f;
        check(!normalize(uncovered),"viewport bottom edge at last sample center is uncovered");
        auto invalid=domains; --invalid.struct_size; check(!normalize(invalid),"short depth descriptor refused");
        invalid=domains; ++invalid.version; check(!normalize(invalid),"unknown depth version refused");
        invalid=domains; invalid.domain_count=3; check(!normalize(invalid),"excess depth domains refused");
        invalid=domains; invalid.reserved[0]=1; check(!normalize(invalid),"reserved depth field refused");
        invalid=domains; invalid.domains[1].classify_min+=.01f; check(!normalize(invalid),"partial-domain gap refused");
        invalid=domains; invalid.domains[1].endpoint_flags|=1; check(!normalize(invalid),"ambiguous included boundary refused");
        invalid=domains; invalid.domains[0].viewport_max_depth=split-.001f; check(!normalize(invalid),"unmapping outside quantization tolerance refused");
        invalid=domains; invalid.domains[1].viewport_min_depth=std::nextafter(split,1.f);
        check(normalize(invalid),"one UNORM step endpoint tolerance accepted");
        invalid=domains; invalid.domains[0].source_clip_to_target_clip[11]=-1; check(!normalize(invalid),"possible zero W at box corner refused");
        invalid=domains; invalid.domains[0].source_clip_to_target_clip[15]=1e-8f; check(!normalize(invalid),"near-zero W refused");
        invalid=domains; invalid.domains[0].source_clip_to_target_clip[15]=-1; check(!normalize(invalid),"negative W refused");
        invalid=domains; invalid.domains[0].source_clip_to_target_clip[0]=std::numeric_limits<float>::infinity();
        check(!normalize(invalid),"nonfinite matrix refused");
        invalid=domains; invalid.domains[0].source_clip_to_target_clip[0]=0; check(!normalize(invalid),"singular matrix refused");
        invalid=domains; invalid.domains[0].source_clip_to_target_clip[10]=1e30f; check(!normalize(invalid),"overflow-risk matrix refused");
        invalid=domains; invalid.domains[0].viewport_xywh[0]=3; check(!normalize(invalid),"viewport missing part of crop refused");
        std::array<float,48> floats{}; floats.fill(.25f);
        auto float_depth=texture(device.Get(),DXGI_FORMAT_R32_FLOAT,floats.data(),4,D3D11_BIND_SHADER_RESOURCE);
        if (!float_depth) return 1;
        check(!normalize(domains,float_depth.Get()),"mapped float depth has no finite-sample contract");
        const rsf_game_depth_domains zero{};
        check(normalize(zero,float_depth.Get()),"zero descriptor retains legacy R32 exact-copy path");
        if (second_device) {
            auto foreign=texture(other_device.Get(),DXGI_FORMAT_R16_TYPELESS,depth16.data(),2,D3D11_BIND_DEPTH_STENCIL);
            if (!foreign) return 1;
            auto* bound=aux_target.Get(); context->OMSetRenderTargets(1,&bound,nullptr);
            auto* retained_depth=local_depth;
            check(!normalize(domains,foreign.Get()),"foreign depth refused before readable copy or cache mutation");
            auto foreign_color=texture(other_device.Get(),DXGI_FORMAT_R32G32B32A32_FLOAT,colour.data(),16,D3D11_BIND_SHADER_RESOURCE);
            auto foreign_motion=texture(other_device.Get(),DXGI_FORMAT_R16G16_FLOAT,packed_vectors.data(),4,D3D11_BIND_SHADER_RESOURCE);
            if (!foreign_color || !foreign_motion) return 1;
            check(!rsf_native_regions_prepare(context.Get(),foreign_color.Get(),region_depth.Get(),region_motion.Get(),region_rect,
                &local_color,&local_depth,&local_motion,&domains,false),"foreign region colour refused");
            check(!rsf_native_regions_prepare(context.Get(),immutable.Get(),region_depth.Get(),foreign_motion.Get(),region_rect,
                &local_color,&local_depth,&local_motion,&domains,false),"foreign region motion refused");
            ComPtr<ID3D11RenderTargetView> retained; context->OMGetRenderTargets(1,&retained,nullptr);
            check(retained.Get()==bound && local_depth==retained_depth,"foreign depth preserves outputs and native bindings");
            context->OMSetRenderTargets(0,nullptr,nullptr);
        }
        // Exercise D24 quantized values and one-step endpoint tolerance separately.
        std::array<uint32_t,48> depth24{};
        for (size_t i=0;i<48;++i) depth24[i]=uint32_t(i%5)+0x7ffffeu;
        auto d24=texture(device.Get(),DXGI_FORMAT_R24G8_TYPELESS,depth24.data(),4,D3D11_BIND_DEPTH_STENCIL);
        if (!d24) return 1;
        auto mapped24=domains;
        const float split24=float(0x800000)/16777215.f;
        mapped24.domains[0].classify_max=mapped24.domains[0].viewport_max_depth=split24;
        mapped24.domains[1].classify_min=mapped24.domains[1].viewport_min_depth=split24;
        check(normalize(mapped24,d24.Get()),"D24 exact decoded split accepted");
        const auto values24=read(device.Get(),context.Get(),local_depth,4);
        for (size_t y=0;y<4;++y) for (size_t x=0;x<4;++x) {
            const double stored=double(depth24[(y+1)*8+x+2])/16777215;
            const bool first_domain=float(stored)<=split24;
            const double z=first_domain ? stored/split24 : (stored-split24)/(1-split24);
            const double expected=first_domain ? (.5*z)/(1+.5*z) : std::clamp(2*z-.5,0.0,1.0);
            float actual{}; if (values24.size()==64) std::memcpy(&actual,values24.data()+(y*4+x)*4,4);
            check(values24.size()==64 && std::abs(actual-expected)<2e-5,"D24 quantization retains correct split-side depth");
        }
        check(read(device.Get(),context.Get(),d24.Get(),4)==std::vector<unsigned char>(
            reinterpret_cast<const unsigned char*>(depth24.data()),reinterpret_cast<const unsigned char*>(depth24.data()+48)),
            "native D24 bits unchanged");
    }
    check(read(device.Get(),context.Get(),region_depth.Get(),2)==region_before,"native partitioned depth unchanged");
    rsf_native_regions_release();
    return failures ? 1 : 0;
}
