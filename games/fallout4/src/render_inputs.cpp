// SPDX-License-Identifier: GPL-3.0-only
#include "render_inputs.h"
#include "camera.h"
#include "depth_domains.h"
#include "native_creation.h"
#include "native_input_state.h"
#include <windows.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <cstdio>
#include <utility>

namespace rsf::fallout4 {
namespace {
using Microsoft::WRL::ComPtr;
constexpr uintptr_t renderer_pointer = 0x38e13a0, state_rva = 0x3d871a0, targets_rva = 0x2f4d750;
constexpr uintptr_t world_graph_pointer = 0x32dd338;
std::atomic<bool> verified{};
uintptr_t image() noexcept { return uintptr_t(GetModuleHandleW(nullptr)); }
bool copy(uintptr_t address, void* destination, size_t size) noexcept {
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),destination,size,&copied) && copied==size;
}
template<class T> bool read(uintptr_t address, T& value) noexcept { return copy(address,&value,sizeof(value)); }
template<size_t N> bool guard(uintptr_t rva, const char (&bytes)[N]) noexcept {
    std::array<char,N-1> actual{};
    return copy(image()+rva,actual.data(),actual.size()) && !std::memcmp(actual.data(),bytes,actual.size());
}
bool same_object(IUnknown* a, IUnknown* b) noexcept {
    if (!a || !b) return false;
    ComPtr<IUnknown> left,right;
    return SUCCEEDED(a->QueryInterface(IID_PPV_ARGS(&left))) && SUCCEEDED(b->QueryInterface(IID_PPV_ARGS(&right))) && left.Get()==right.Get();
}
struct NativeTarget { ID3D11Texture2D* texture; void* copy_texture; ID3D11RenderTargetView* rtv; ID3D11ShaderResourceView* srv; void* copy_srv; void* uav; };
static_assert(sizeof(NativeTarget)==0x30);
using detail::NativeState;
enum class ReadStage : unsigned {
    unverified, owner, dimensions, delta, camera_owner, cache, temporal_flags,
    viewport_read, viewport_bounds, ratio, colour_map, motion_map, depth_map,
    cache_read, cache_duplicate, cache_missing, camera_read, target_read, context,
    device, colour_srv, motion_srv, depth_srv, swap, colour_rtv, extent_format,
    camera_math, state_changed, camera_changed, history_changed, accepted, exception, count
};
constexpr const char* stage_names[]{
    "unverified","owner","dimensions","delta","camera-owner","cache","temporal-flags",
    "viewport-read","viewport-bounds","ratio","colour-map","motion-map","depth-map",
    "cache-read","cache-duplicate","cache-missing","camera-read","target-read","context",
    "device","colour-srv","motion-srv","depth-srv","swap","colour-rtv","extent-format",
    "camera-math","state-changed","camera-changed","history-changed","accepted","exception"
};
static_assert(std::size(stage_names)==unsigned(ReadStage::count) && unsigned(ReadStage::count)<=64);
std::atomic<uint64_t> reader_reported{};
bool owner(NativeState& out, ReadStage* failure = nullptr) noexcept {
    const auto no=[&](ReadStage stage) { if (failure) *failure=stage; return false; };
    const auto base=image(), graphics=base+state_rva;
    if (!read(base+renderer_pointer,out.renderer) || out.renderer!=base+0x3a25d10 ||
        !read(out.renderer+0x48,out.device) || !out.device || !read(out.renderer+0x50,out.context) || !out.context ||
        !read(out.renderer+0x58,out.window) || !out.window || !read(out.renderer+0x70,out.chain) || !out.chain) return no(ReadStage::owner);
    if (!read(graphics+0x98,out.frame) || !read(graphics+0x78,out.width) || !read(graphics+0x7c,out.height) ||
        !out.width || !out.height || out.width>16384 || out.height>16384) return no(ReadStage::dimensions);
    return true;
}
bool state(NativeState& out, ReadStage* failure = nullptr) noexcept {
    const auto no=[&](ReadStage stage) { if (failure) *failure=stage; return false; };
    const auto base=image(), graphics=base+state_rva, targets=base+targets_rva;
    uintptr_t graph{};
    if (!owner(out,failure)) return false;
    if (!read(base+0x339f430,out.delta_seconds) || !std::isfinite(out.delta_seconds) || out.delta_seconds<0) return no(ReadStage::delta);
    if (!read(base+world_graph_pointer,graph) || !graph || !read(graph+0x140,out.camera) || !out.camera) return no(ReadStage::camera_owner);
    if (!read(graphics+0x140,out.cache) || !out.cache || !read(graphics+0x150,out.count) || !out.count || out.count>256) return no(ReadStage::cache);
    if (!read(graphics+0xac,out.taa) || !out.taa || !read(graphics+0xb0,out.disable) || out.disable) return no(ReadStage::temporal_flags);
    if (!read(graphics+0x88,out.viewport) || !read(targets+0xfb8,out.ratio) ||
        !read(targets+0xfe4,out.default_viewport) || !read(targets+0xfe5,out.dynamic_viewport)) return no(ReadStage::viewport_read);
    if (!read(targets+0xdf4+4,out.colour) || out.colour>=101) return no(ReadStage::colour_map);
    if (!read(targets+0xdf4+32*4,out.motion) || out.motion>=101) return no(ReadStage::motion_map);
    if (!read(targets+0xf84+4,out.depth) || out.depth>=13) return no(ReadStage::depth_map);
    // State+88 is metadata, not the Context viewport submitted by the renderer.
    // Reduced raster still needs its separate producer/consumer contract.
    for (float ratio : out.ratio) if (!std::isfinite(ratio) || ratio<=0 || ratio>1) return no(ReadStage::ratio);
    return true;
}
bool active_full_viewport(ID3D11DeviceContext* context, UINT width, UINT height,
    UINT& count, D3D11_VIEWPORT& viewport, bool full=true) noexcept {
    count=0; context->RSGetViewports(&count,nullptr);
    if (count!=1) return false;
    UINT capacity=1; context->RSGetViewports(&capacity,&viewport); count=capacity;
    return count==1 && std::isfinite(viewport.TopLeftX) && std::isfinite(viewport.TopLeftY) &&
        std::isfinite(viewport.Width) && std::isfinite(viewport.Height) &&
        std::isfinite(viewport.MinDepth) && std::isfinite(viewport.MaxDepth) &&
        viewport.TopLeftX>=0 && viewport.TopLeftY>=0 && viewport.Width>0 && viewport.Height>0 &&
        double(viewport.TopLeftX)+viewport.Width<=width && double(viewport.TopLeftY)+viewport.Height<=height &&
        (!full || (viewport.TopLeftX==0 && viewport.TopLeftY==0 && viewport.Width==float(width) && viewport.Height==float(height))) &&
        // Depth endpoints describe the last native draw, not the projection or
        // encoding of every pixel already stored in the scene-depth texture.
        viewport.MinDepth>=0 && viewport.MinDepth<viewport.MaxDepth && viewport.MaxDepth<=1;
}
bool camera(uintptr_t entry, NativeCamera& out) noexcept {
    return read(entry+0x50,out.view) && read(entry+0x90,out.projection) &&
        read(entry+0x110,out.view_projection_unjittered) && read(entry+0x190,out.previous_view_projection_unjittered) &&
        read(entry+0x210,out.position_adjust) && read(entry+0x228,out.previous_position_adjust) &&
        read(image()+state_rva+4,out.jitter_ndc);
}
struct ViewFacts {
    D3D11_SHADER_RESOURCE_VIEW_DESC description{};
    bool read{}, resource_match{}, device_match{};
};
bool view_texture(ID3D11ShaderResourceView* view, ID3D11Texture2D* expected, ID3D11Device* owner, DXGI_FORMAT format,
    ComPtr<ID3D11Texture2D>& texture, ViewFacts& facts) noexcept {
    if (!view || !expected) return false;
    view->GetDesc(&facts.description); facts.read=true;
    ComPtr<ID3D11Resource> resource; view->GetResource(&resource);
    if (!resource || FAILED(resource.As(&texture))) return false;
    facts.resource_match=same_object(texture.Get(),expected);
    if (!facts.resource_match) return false;
    ComPtr<ID3D11Device> device; texture->GetDevice(&device);
    const auto& description=facts.description;
    facts.device_match=same_object(device.Get(),owner);
    return facts.device_match && description.ViewDimension==D3D11_SRV_DIMENSION_TEXTURE2D &&
        !description.Texture2D.MostDetailedMip && description.Texture2D.MipLevels==1 &&
        (format==DXGI_FORMAT_UNKNOWN ? description.Format==DXGI_FORMAT_R24_UNORM_X8_TYPELESS || description.Format==DXGI_FORMAT_R32_FLOAT : description.Format==format);
}
bool extent(ID3D11Texture2D* texture, uint32_t width, uint32_t height, D3D11_TEXTURE2D_DESC& out) noexcept {
    texture->GetDesc(&out);
    return out.Width==width && out.Height==height && out.MipLevels==1 && out.ArraySize==1 && out.SampleDesc.Count==1;
}
void auxiliary(const NativeState& native, ID3D11Device* device, uint32_t logical,
    ComPtr<ID3D11Texture2D>& output) noexcept {
    output.Reset();
    const auto map=image()+targets_rva+0xdf4+size_t(logical)*4;
    uint32_t platform{},confirmed_platform{};
    NativeTarget target{},confirmed_target{};
    if (!read(map,platform) || platform>=101 ||
        !read(native.renderer+0xa58+size_t(platform)*0x30,target) || !target.texture || !target.rtv) return;
    ComPtr<ID3D11Resource> resource; target.rtv->GetResource(&resource);
    ComPtr<ID3D11Texture2D> texture;
    if (!resource || FAILED(resource.As(&texture)) || !same_object(texture.Get(),target.texture)) return;
    ComPtr<ID3D11Device> texture_device; texture->GetDevice(&texture_device);
    D3D11_RENDER_TARGET_VIEW_DESC rtv{}; target.rtv->GetDesc(&rtv);
    D3D11_TEXTURE2D_DESC description{};
    // Only full-output allocations share the verified Main viewport/crop.
    // The native format is deliberately not inferred from the logical role.
    if (!same_object(texture_device.Get(),device) || !extent(texture.Get(),native.width,native.height,description) ||
        rtv.ViewDimension!=D3D11_RTV_DIMENSION_TEXTURE2D || rtv.Texture2D.MipSlice ||
        !read(map,confirmed_platform) || platform!=confirmed_platform ||
        !read(native.renderer+0xa58+size_t(platform)*0x30,confirmed_target) ||
        std::memcmp(&target,&confirmed_target,sizeof(target))) return;
    output=std::move(texture);
}
bool close_matrix(const float* a, const float* b) noexcept {
    for (unsigned i=0;i<16;++i) if (!std::isfinite(a[i]) || !std::isfinite(b[i]) || std::abs(a[i]-b[i])>0.001f) return false;
    return true;
}
struct History {
    bool seen{}, reset{};
    uintptr_t camera{};
    uint32_t frame{}, width{}, height{};
    NativeCamera native{};
    CameraRaster raster{}, previous_raster{};
    bool previous_raster_valid{};
    float jitter[2]{}, previous_jitter[2]{};
};
thread_local History history;
}
bool verify_render_inputs() noexcept try {
    verified.store(false,std::memory_order_release);
    const bool match=verify_executable() &&
        guard(0x18a246,"\x48\x8d\x1d\xc3\xba\x89\x03") &&
        guard(0x18a28c,"\x48\x89\x1d\x0d\x71\x75\x03") &&
        guard(0xc37dd0,"\x48\x8b\x05\x61\x55\x6a\x02\x48\x8b\x80\x40\x01\x00\x00\xc3") &&
        guard(0x21f0e3d,"\x48\x8d\x0d\x5c\x63\xb9\x01") &&
        guard(0x1839be2,"\x8b\xb4\x81\xf4\x0d\x00\x00") &&
        guard(0x1839cbb,"\x8b\x8c\x81\x84\x0f\x00\x00") &&
        guard(0x183a4a0,"\x48\x63\x8c\x81\x84\x0f\x00\x00\x48\x69\xc1\x98\x00\x00\x00") &&
        guard(0x182bf4a,"\x48\x8d\xb1\x40\x01\x00\x00\x8b\x46\x10\x85\xc0\x74\x39") &&
        guard(0x182bf6a,"\x49\x39\xb8\x38\x02\x00\x00\x75\x09\x41\x38\x98\x40\x02\x00\x00\x74\x09") &&
        guard(0x182da63,"\xf3\x41\x0f\x59\x5e\x04\x0f\x57\xc0\xf3\x41\x0f\x59\x56\x08") &&
        guard(0x182e226,"\x8b\x05\x0c\x90\x55\x02") &&
        guard(0xc33a60,"\x48\x8d\x0d\xb9\xb9\x76\x02") &&
        guard(0x165ba9f,"\xf3\x41\x0f\x11\x50\x10") &&
        guard(0x165bab0,"\xf3\x41\x0f\x11\x40\x14") &&
        guard(0x165baba,"\xf3\x0f\x59\x05\x76\xff\x8e\x01") &&
        guard(0x165bac6,"\xf3\x41\x0f\x11\x40\x10") &&
        guard(0x29293f8,"\x6f\x12\x83\x3a") &&
        guard(0x1839e34,"\x80\xb9\xe4\x0f\x00\x00\x00") &&
        guard(0x1817fd1,"\xf3\x0f\x59\x81\xd0\x06\x00\x00") &&
        guard(0x182430d,"\x4c\x8d\x87\x90\x00\x00\x00") &&
        guard(0x182431c,"\xff\x90\x60\x01\x00\x00");
    verified.store(match,std::memory_order_release); return match;
} catch (...) { return false; }

bool read_native_inputs(NativeInputs& output, void (*report)(const char*), const NativeDepthSnapshot* producer) noexcept try {
    output={};
    NativeState before{},after{};
    NativeCamera native{},confirmed{};
    NativeTarget colour{},motion{};
    uintptr_t depth_texture{},depth_view{},selected{},last_key{};
    uint32_t scanned{}, camera_matches{}; unsigned char last_jitter{}, matching_jitter{};
    bool camera_read{};
    NativeInputs result;
    ViewFacts colour_facts{},motion_facts{},depth_facts{};
    DXGI_SWAP_CHAIN_DESC swap{};
    D3D11_RENDER_TARGET_VIEW_DESC rtv{};
    D3D11_VIEWPORT active_viewport{},confirmed_viewport{};
    UINT viewport_count{},confirmed_viewport_count{};
    ReadStage stage=ReadStage::unverified;
    const auto receipt=[&](ReadStage reason) noexcept { try {
        if (!report) return;
        const auto bit=uint64_t(1)<<unsigned(reason);
        // One bundle per named stage, at most count bundles per process. Loading
        // cannot spend the budget reserved for a later distinct refusal stage.
        if (reader_reported.fetch_or(bit,std::memory_order_relaxed)&bit) return;
        char text[1600]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 reader stage=%s native=%u partial=1 renderer=%p expected=%p device=%p context=%p chain=%p window=%p output=%ux%u camera=%p cache=%p count=%u scanned=%u camera_matches=%u matching_jitter=%u selected=%p last_key=%p last_jitter=%u taa=%u disable=%u dt=%g metadata_viewport=%g,%g,%g,%g default=%u dynamic=%u rs_count=%u rs=%g,%g,%g,%g,%g,%g confirmed_rs=%u/%g,%g ratio=%g,%g maps=%u,%u,%u swap=%ux%u/%p rtv=%u/%u/%u",
            stage_names[unsigned(reason)],before.frame,reinterpret_cast<void*>(before.renderer),reinterpret_cast<void*>(image()+0x3a25d10),
            reinterpret_cast<void*>(before.device),reinterpret_cast<void*>(before.context),reinterpret_cast<void*>(before.chain),reinterpret_cast<void*>(before.window),
            before.width,before.height,reinterpret_cast<void*>(before.camera),reinterpret_cast<void*>(before.cache),before.count,scanned,camera_matches,unsigned(matching_jitter),
            reinterpret_cast<void*>(selected),reinterpret_cast<void*>(last_key),unsigned(last_jitter),before.taa,before.disable,double(before.delta_seconds),
            double(before.viewport[0]),double(before.viewport[1]),double(before.viewport[2]),double(before.viewport[3]),unsigned(before.default_viewport),unsigned(before.dynamic_viewport),
            viewport_count,double(active_viewport.TopLeftX),double(active_viewport.TopLeftY),double(active_viewport.Width),double(active_viewport.Height),double(active_viewport.MinDepth),double(active_viewport.MaxDepth),
            confirmed_viewport_count,double(confirmed_viewport.Width),double(confirmed_viewport.Height),double(before.ratio[0]),double(before.ratio[1]),
            before.colour,before.motion,before.depth,swap.BufferDesc.Width,swap.BufferDesc.Height,static_cast<void*>(swap.OutputWindow),
            unsigned(rtv.Format),unsigned(rtv.ViewDimension),rtv.Texture2D.MipSlice);
        report(text);
        const std::array<ID3D11Texture2D*,3> textures{result.colour.Get(),result.motion.Get(),result.depth.Get()};
        const std::array<ViewFacts*,3> views{&colour_facts,&motion_facts,&depth_facts};
        const std::array<void*,3> expected{colour.texture,motion.texture,reinterpret_cast<void*>(depth_texture)};
        const std::array<const char*,3> names{"colour","motion","depth"};
        for (size_t i=0;i<textures.size();++i) if (views[i]->read) {
            D3D11_TEXTURE2D_DESC desc{};
            if (textures[i]) textures[i]->GetDesc(&desc); // First-stage receipt only, on this native callback.
            const auto& view=*views[i];
            std::snprintf(text,sizeof(text),
                "Fallout4 reader stage=%s %s expected=%p actual=%p resource_match=%u device_match=%u texture=%ux%u format=%u mips=%u array=%u sample=%u/%u bind=0x%x srv_format=%u srv_dimension=%u srv_mip=%u srv_mips=%u",
                stage_names[unsigned(reason)],names[i],expected[i],static_cast<void*>(textures[i]),unsigned(view.resource_match),unsigned(view.device_match),
                desc.Width,desc.Height,unsigned(desc.Format),desc.MipLevels,desc.ArraySize,desc.SampleDesc.Count,desc.SampleDesc.Quality,desc.BindFlags,
                unsigned(view.description.Format),unsigned(view.description.ViewDimension),view.description.Texture2D.MostDetailedMip,view.description.Texture2D.MipLevels);
            report(text);
        }
        if (camera_read) {
            std::snprintf(text,sizeof(text),
                "Fallout4 reader stage=%s projection=%g,%g,%g,%g,%g,%g origin=%g,%g,%g previous_origin=%g,%g,%g global_jitter=%g,%g confirmed_native=%u camera_valid=%u render=%ux%u near=%g far=%g jitter=%g,%g",
                stage_names[unsigned(reason)],double(native.projection[0]),double(native.projection[5]),double(native.projection[10]),double(native.projection[11]),double(native.projection[14]),double(native.projection[15]),
                double(native.position_adjust[0]),double(native.position_adjust[1]),double(native.position_adjust[2]),double(native.previous_position_adjust[0]),double(native.previous_position_adjust[1]),double(native.previous_position_adjust[2]),
                double(native.jitter_ndc[0]),double(native.jitter_ndc[1]),after.frame,result.pass.camera_valid,result.pass.camera.render_width,result.pass.camera.render_height,
                double(result.pass.camera.near_plane),double(result.pass.camera.far_plane),double(result.pass.camera.jitter_pixels[0]),double(result.pass.camera.jitter_pixels[1]));
            report(text);
        }
    } catch (...) {} };
    const auto refuse=[&](ReadStage reason) { receipt(reason); return false; };
    if (!verified.load(std::memory_order_acquire)) return refuse(ReadStage::unverified);
    if (!state(before,&stage)) return refuse(stage);
    if (producer) {
        if (!native_depth_snapshot_current(*producer) || producer->native_frame!=before.frame ||
            producer->world_camera_key!=before.camera || producer->width!=before.width || producer->height!=before.height ||
            producer->device_context_key!=before.context || producer->cache_key!=before.cache) return refuse(ReadStage::state_changed);
    } else if (before.ratio[0]!=1 || before.ratio[1]!=1) return refuse(ReadStage::ratio);
    for (uint32_t i=0;i<before.count;++i) {
        const auto entry=before.cache+size_t(i)*0x250;
        uintptr_t key{}; unsigned char jitter{};
        if (!read(entry+0x238,key) || !read(entry+0x240,jitter)) return refuse(ReadStage::cache_read);
        ++scanned; last_key=key; last_jitter=jitter;
        if (key==before.camera) { ++camera_matches; matching_jitter=jitter; }
        if (key==before.camera && jitter==1) { if (selected) return refuse(ReadStage::cache_duplicate); selected=entry; }
    }
    if (!selected) return refuse(ReadStage::cache_missing);
    if (!camera(selected,native)) return refuse(ReadStage::camera_read);
    camera_read=true;
    if (!read(before.renderer+0xa58+size_t(before.colour)*0x30,colour) ||
        !read(before.renderer+0xa58+size_t(before.motion)*0x30,motion) ||
        !read(before.renderer+0x1d48+size_t(before.depth)*0x98,depth_texture) ||
        !read(before.renderer+0x1d48+size_t(before.depth)*0x98+0x88,depth_view)) return refuse(ReadStage::target_read);
    result.context=reinterpret_cast<ID3D11DeviceContext*>(before.context);
    result.chain=reinterpret_cast<IDXGISwapChain*>(before.chain);
    if (result.context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return refuse(ReadStage::context);
    ComPtr<ID3D11Device> device,chain_device;
    result.context->GetDevice(&device);
    if (!same_object(device.Get(),reinterpret_cast<ID3D11Device*>(before.device)) ||
        FAILED(result.chain->GetDevice(IID_PPV_ARGS(&chain_device))) || !same_object(device.Get(),chain_device.Get())) return refuse(ReadStage::device);
    if (!active_full_viewport(result.context.Get(),before.width,before.height,viewport_count,active_viewport,!producer)) return refuse(ReadStage::viewport_bounds);
    if (!view_texture(colour.srv,colour.texture,device.Get(),DXGI_FORMAT_R11G11B10_FLOAT,result.colour,colour_facts)) return refuse(ReadStage::colour_srv);
    if (!view_texture(motion.srv,motion.texture,device.Get(),DXGI_FORMAT_R16G16_FLOAT,result.motion,motion_facts)) return refuse(ReadStage::motion_srv);
    if (!view_texture(reinterpret_cast<ID3D11ShaderResourceView*>(depth_view),reinterpret_cast<ID3D11Texture2D*>(depth_texture),device.Get(),DXGI_FORMAT_UNKNOWN,result.depth,depth_facts)) return refuse(ReadStage::depth_srv);
    if (FAILED(result.chain->GetDesc(&swap)) || swap.OutputWindow!=reinterpret_cast<HWND>(before.window) ||
        swap.BufferDesc.Width!=before.width || swap.BufferDesc.Height!=before.height || !colour.rtv) return refuse(ReadStage::swap);
    ComPtr<ID3D11Resource> render_target; colour.rtv->GetResource(&render_target);
    colour.rtv->GetDesc(&rtv);
    if (!same_object(render_target.Get(),result.colour.Get()) || rtv.ViewDimension!=D3D11_RTV_DIMENSION_TEXTURE2D ||
        rtv.Format!=DXGI_FORMAT_R11G11B10_FLOAT || rtv.Texture2D.MipSlice) return refuse(ReadStage::colour_rtv);
    D3D11_TEXTURE2D_DESC c{},d{},m{};
    if (!extent(result.colour.Get(),before.width,before.height,c) || !extent(result.depth.Get(),before.width,before.height,d) ||
        !extent(result.motion.Get(),before.width,before.height,m) || c.Format!=DXGI_FORMAT_R11G11B10_FLOAT ||
        m.Format!=DXGI_FORMAT_R16G16_FLOAT || (d.Format!=DXGI_FORMAT_R24G8_TYPELESS && d.Format!=DXGI_FORMAT_R32_TYPELESS && d.Format!=DXGI_FORMAT_R32_FLOAT)) return refuse(ReadStage::extent_format);
    CameraRaster raster{};
    if (producer) {
        std::memcpy(raster.xywh,producer->actual_raster,sizeof(raster.xywh));
        std::memcpy(raster.crop,producer->render_rect,sizeof(raster.crop));
        const auto& w=producer->world_camera;
        if (producer->depth_key!=depth_texture || !close_matrix(native.view,w.view) ||
            !close_matrix(native.projection,w.projection) || !close_matrix(native.view_projection_unjittered,w.view_projection_unjittered) ||
            std::memcmp(native.position_adjust,w.origin,sizeof(w.origin))) return refuse(ReadStage::camera_changed);
    } else {
        raster.xywh[2]=float(before.width); raster.xywh[3]=float(before.height);
        raster.crop[2]=int32_t(before.width); raster.crop[3]=int32_t(before.height);
    }
    const auto width=uint32_t(raster.crop[2]-raster.crop[0]);
    const auto height=uint32_t(raster.crop[3]-raster.crop[1]);
    auxiliary(before,device.Get(),14,result.refraction_normals);
    auxiliary(before,device.Get(),30,result.pipboy);
    auxiliary(before,device.Get(),15,result.vats);
    auto& pass=result.pass;
    bool valid_history{};
    const CameraRaster* previous_raster=nullptr;
    if (history.seen && history.frame!=before.frame) previous_raster=&history.raster;
    else if (history.previous_raster_valid) previous_raster=&history.previous_raster;
    if (!camera_frame(native,width,height,pass.camera,pass.previous_clip_to_clip,valid_history,&raster,previous_raster)) return refuse(ReadStage::camera_math);
    if (!state(after) || !detail::same_native_state(before,after)) return refuse(ReadStage::state_changed);
    if (!active_full_viewport(result.context.Get(),before.width,before.height,confirmed_viewport_count,confirmed_viewport,!producer) ||
        std::memcmp(&active_viewport,&confirmed_viewport,sizeof(active_viewport))) return refuse(ReadStage::viewport_bounds);
    if (producer && !native_depth_snapshot_current(*producer)) return refuse(ReadStage::state_changed);
    if (!camera(selected,confirmed) || std::memcmp(&native,&confirmed,sizeof(native))) return refuse(ReadStage::camera_changed);
    if (!history.seen || history.frame!=before.frame) {
        const auto previous=history;
        history={}; history.seen=true; history.camera=before.camera; history.frame=before.frame;
        history.width=width; history.height=height; history.native=native;
        history.raster=raster; history.previous_raster=previous.raster; history.previous_raster_valid=previous.seen;
        std::memcpy(history.jitter,pass.camera.jitter_pixels,sizeof(history.jitter));
        history.reset=!previous.seen || !valid_history || previous.camera!=before.camera || before.frame!=uint32_t(previous.frame+1) ||
            previous.width!=width || previous.height!=height || std::memcmp(&previous.raster,&raster,sizeof(raster)) ||
            !close_matrix(native.previous_view_projection_unjittered,previous.native.view_projection_unjittered) ||
            std::memcmp(native.previous_position_adjust,previous.native.position_adjust,sizeof(native.position_adjust));
        if (!history.reset) std::memcpy(history.previous_jitter,previous.jitter,sizeof(history.previous_jitter));
    } else if (history.camera!=before.camera || history.width!=width || history.height!=height ||
        !close_matrix(native.view_projection_unjittered,history.native.view_projection_unjittered) ||
        std::memcmp(&history.raster,&raster,sizeof(raster))) return refuse(ReadStage::history_changed);
    pass.struct_size=sizeof(pass); pass.native_frame=before.frame;
    pass.family_key=pass.view_key=pass.history_key=before.camera;
    pass.viewport_key=pass.window_key=pass.rhi_viewport_key=before.window;
    if (history.reset) pass.flags|=RSF_GAME_RENDER_RESET;
    pass.camera.output_width=before.width; pass.camera.output_height=before.height;
    pass.camera.frame_time_seconds=before.delta_seconds;
    std::memcpy(pass.camera.previous_jitter_pixels,history.previous_jitter,sizeof(history.previous_jitter));
    std::memcpy(pass.render_rect,raster.crop,sizeof(pass.render_rect));
    pass.output_rect[2]=int32_t(before.width); pass.output_rect[3]=int32_t(before.height);
    pass.color_input=pass.color_output=result.colour.Get(); pass.depth=result.depth.Get(); pass.motion=result.motion.Get();
    pass.swapchain=result.chain.Get(); pass.camera_valid=1;
    // Native velocity encodes previous-current UV in the native camera's
    // viewport metric. The steady-history crop scales that displacement.
    if (!camera_motion_to_uv(raster,pass.motion_to_uv)) return refuse(ReadStage::camera_math);
    pass.motion_camera_included=1;
    receipt(ReadStage::accepted);
    output=std::move(result); return true;
} catch (...) {
    const auto bit=uint64_t(1)<<unsigned(ReadStage::exception);
    if (report && !(reader_reported.fetch_or(bit,std::memory_order_relaxed)&bit)) try { report("Fallout4 reader stage=exception"); } catch (...) {}
    return false;
}

bool read_native_framebuffer(NativeFramebuffer& output, void (*report)(const char*)) noexcept try {
    output={};
    NativeState before{},after{};
    uint32_t platform{},confirmed_platform{};
    NativeTarget target{},confirmed_target{};
    NativeFramebuffer result;
    D3D11_RENDER_TARGET_VIEW_DESC rtv{};
    D3D11_TEXTURE2D_DESC texture{};
    DXGI_SWAP_CHAIN_DESC swap{};
    ReadStage owner_failure=ReadStage::owner;
    enum class Stage : unsigned {
        unverified, owner, map, target, context, device, resource, target_texture,
        chain_device, texture_device, extent, rtv, swap, window, swap_extent,
        owner_changed, map_changed, target_changed, accepted
    };
    const auto receipt=[&](Stage stage) noexcept { try {
        if (!report) return;
        static std::atomic<uint32_t> reported{};
        const auto bit=uint32_t(1)<<unsigned(stage);
        if (reported.fetch_or(bit,std::memory_order_relaxed)&bit) return;
        constexpr const char* names[]{"unverified","owner","map","target","context","device","resource","target-texture",
            "chain-device","texture-device","extent","rtv","swap","window","swap-extent",
            "owner-changed","map-changed","target-changed","accepted"};
        char text[960]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 framebuffer stage=%s partial=1 native=%u owner_stage=%s renderer=%p context=%p chain=%p window=%p output=%ux%u logical=0 platform=%u/%u native_texture=%p rtv=%p leased_texture=%p texture=%ux%u mip=%u array=%u samples=%u format=%u rtv_kind=%u rtv_mip=%u swap=%ux%u/%p after=%u/%p/%p/%p",
            names[unsigned(stage)],before.frame,stage_names[unsigned(owner_failure)],
            reinterpret_cast<void*>(before.renderer),reinterpret_cast<void*>(before.context),reinterpret_cast<void*>(before.chain),
            reinterpret_cast<void*>(before.window),before.width,before.height,platform,confirmed_platform,
            static_cast<void*>(target.texture),static_cast<void*>(target.rtv),static_cast<void*>(result.texture.Get()),
            texture.Width,texture.Height,texture.MipLevels,texture.ArraySize,texture.SampleDesc.Count,unsigned(texture.Format),
            unsigned(rtv.ViewDimension),rtv.Texture2D.MipSlice,swap.BufferDesc.Width,swap.BufferDesc.Height,
            static_cast<void*>(swap.OutputWindow),after.frame,reinterpret_cast<void*>(after.context),
            reinterpret_cast<void*>(after.chain),reinterpret_cast<void*>(after.window));
        report(text);
    } catch (...) {} };
    const auto refuse=[&](Stage stage) { receipt(stage); return false; };
    if (!verified.load(std::memory_order_acquire)) return refuse(Stage::unverified);
    const auto map=image()+targets_rva+0xdf4;
    if (!owner(before,&owner_failure)) return refuse(Stage::owner);
    if (!read(map,platform) || platform>=101) return refuse(Stage::map);
    if (!read(before.renderer+0xa58+size_t(platform)*0x30,target) || !target.rtv) return refuse(Stage::target);
    result.context=reinterpret_cast<ID3D11DeviceContext*>(before.context);
    result.chain=reinterpret_cast<IDXGISwapChain*>(before.chain);
    if (result.context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return refuse(Stage::context);
    ComPtr<ID3D11Device> device,chain_device,texture_device;
    result.context->GetDevice(&device);
    ComPtr<ID3D11Resource> resource; target.rtv->GetResource(&resource);
    if (!same_object(device.Get(),reinterpret_cast<ID3D11Device*>(before.device))) return refuse(Stage::device);
    if (!resource || FAILED(resource.As(&result.texture))) return refuse(Stage::resource);
    if (target.texture && !same_object(target.texture,result.texture.Get())) return refuse(Stage::target_texture);
    if (FAILED(result.chain->GetDevice(IID_PPV_ARGS(&chain_device))) || !same_object(device.Get(),chain_device.Get()))
        return refuse(Stage::chain_device);
    result.texture->GetDevice(&texture_device);
    target.rtv->GetDesc(&rtv);
    if (!same_object(texture_device.Get(),device.Get())) return refuse(Stage::texture_device);
    if (!extent(result.texture.Get(),before.width,before.height,texture)) return refuse(Stage::extent);
    if (rtv.ViewDimension!=D3D11_RTV_DIMENSION_TEXTURE2D || rtv.Texture2D.MipSlice) return refuse(Stage::rtv);
    if (FAILED(result.chain->GetDesc(&swap))) return refuse(Stage::swap);
    if (swap.OutputWindow!=reinterpret_cast<HWND>(before.window)) return refuse(Stage::window);
    if (swap.BufferDesc.Width!=before.width || swap.BufferDesc.Height!=before.height) return refuse(Stage::swap_extent);
    if (!owner(after,&owner_failure) || !detail::same_native_owner(before,after)) return refuse(Stage::owner_changed);
    if (!read(map,confirmed_platform) || platform!=confirmed_platform) return refuse(Stage::map_changed);
    if (!read(before.renderer+0xa58+size_t(platform)*0x30,confirmed_target) ||
        std::memcmp(&target,&confirmed_target,sizeof(target))) return refuse(Stage::target_changed);
    result.native_frame=before.frame; result.window_key=before.window;
    receipt(Stage::accepted);
    output=std::move(result); return true;
} catch (...) {
    static std::atomic_flag reported=ATOMIC_FLAG_INIT;
    if (report && !reported.test_and_set(std::memory_order_relaxed)) try { report("Fallout4 framebuffer stage=exception"); } catch (...) {}
    return false;
}
}
