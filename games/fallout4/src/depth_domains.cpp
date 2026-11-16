// SPDX-License-Identifier: GPL-3.0-only
#include "depth_domains.h"
#include "camera.h"
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <DirectXMath.h>
#include <intrin.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <cstdio>
#include <tuple>

namespace rsf::fallout4 {
namespace {
using namespace DirectX;
using Microsoft::WRL::ComPtr;
constexpr uintptr_t graphics_rva=0x3d871a0, targets_rva=0x2f4d750;
static_assert(sizeof(NativeDepthCamera)==71*sizeof(float));
std::atomic<bool> verified{};
enum class Stage : unsigned {
    unverified, source, tls, owner, dimensions, cache, range, camera_owner, depth_owner,
    cache_duplicate, cache_missing, camera_read, camera_math, world_cache, near_inverse,
    raster_math, pass_association, target_camera, depth_format, depth_view, state_changed,
    producer_raster, producer_target, producer_shadow, producer_scissor,
    projection_mismatch, finite_map, near_w, world_w,
    producer_selector, producer_descriptor, producer_target_dirty, producer_target_map, producer_colour, near_basis, count
};
constexpr const char* names[]{"unverified","source","tls","owner","dimensions","cache","range",
    "camera-owner","depth-owner","cache-duplicate","cache-missing","camera-read","camera-math",
    "world-cache","near-inverse","raster-math","pass-association","target-camera","depth-format","depth-view","state-changed",
    "producer-raster","producer-target","producer-shadow","producer-scissor",
    "projection-mismatch","finite-map","near-W","world-W",
    "producer-selector","producer-descriptor","producer-target-dirty","producer-target-map","producer-colour","near-basis"};
static_assert(std::size(names)==unsigned(Stage::count) && unsigned(Stage::count)<=64);
std::atomic<uint64_t> reported{};
struct MathFailure {
    unsigned index{};
    float actual{}, expected{}, relative_error{};
    double w[4]{}, minimum_w{}, range{};
};
bool refuse(Stage stage, const NativeDepthSnapshot& s, void (*report)(const char*), const MathFailure* detail=nullptr) noexcept {
    if (report) try {
        const auto bit=uint64_t(1)<<unsigned(stage);
        if (!(reported.fetch_or(bit,std::memory_order_relaxed)&bit)) {
            char text[800]{};
            std::snprintf(text,sizeof(text),
                "Fallout4 depth stage=%s source=%llu submission=%llu native=%llu mode=%u range=%g,%g/%g,%g raster=%g,%g,%g,%g shadow=%g,%g,%g,%g dirty=%u selectors=%u,%u,%u descriptor=%ux%u descriptor_read=%u physical=%u/%u,%u ratio=%g,%g default=%u dynamic=%u",
                names[unsigned(stage)],static_cast<unsigned long long>(s.identity.source),
                static_cast<unsigned long long>(s.identity.submission),static_cast<unsigned long long>(s.native_frame),
                unsigned(s.mode),double(s.intervals[0]),double(s.intervals[1]),double(s.intervals[2]),double(s.intervals[3]),
                double(s.actual_raster[0]),double(s.actual_raster[1]),double(s.actual_raster[2]),double(s.actual_raster[3]),
                double(s.shadow_raster[0]),double(s.shadow_raster[1]),double(s.shadow_raster[2]),double(s.shadow_raster[3]),
                s.shadow_dirty,s.colour_selector,s.depth_selector,s.cube_selector,s.descriptor_width,s.descriptor_height,
                unsigned(s.descriptor_read),
                s.colour_slot,s.bound_colour_slot,s.bound_depth_slot,
                double(s.ratio[0]),double(s.ratio[1]),unsigned(s.default_viewport),unsigned(s.dynamic_viewport));
            report(text);
            if (detail && (stage==Stage::projection_mismatch || stage==Stage::near_w || stage==Stage::world_w)) {
                char math_text[520]{};
                if (stage==Stage::projection_mismatch) {
                    std::snprintf(math_text,sizeof(math_text),
                        "Fallout4 depth math stage=%s source=%llu native=%llu mismatch_index=%u actual=%.9g expected=%.9g relative_error=%.9g tolerance=0.0005",
                        names[unsigned(stage)],static_cast<unsigned long long>(s.identity.source),static_cast<unsigned long long>(s.native_frame),
                        detail->index,double(detail->actual),double(detail->expected),double(detail->relative_error));
                } else {
                    std::snprintf(math_text,sizeof(math_text),
                        "Fallout4 depth math stage=%s source=%llu native=%llu W=%.9g,%.9g,%.9g,%.9g minimum=%.12g range=%.9g near_origin=%.9g,%.9g,%.9g world_origin=%.9g,%.9g,%.9g",
                        names[unsigned(stage)],static_cast<unsigned long long>(s.identity.source),static_cast<unsigned long long>(s.native_frame),
                        detail->w[0],detail->w[1],detail->w[2],detail->w[3],detail->minimum_w,detail->range,
                        double(s.near_camera.origin[0]),double(s.near_camera.origin[1]),double(s.near_camera.origin[2]),
                        double(s.world_camera.origin[0]),double(s.world_camera.origin[1]),double(s.world_camera.origin[2]));
                }
                report(math_text);
            }
        }
    } catch (...) {}
    return false;
}
uintptr_t image() noexcept { return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)); }
bool bytes(uintptr_t address, void* output, size_t size) noexcept {
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,size,&copied) && copied==size;
}
template<class T> bool read(uintptr_t address, T& output) noexcept { return bytes(address,&output,sizeof(output)); }
template<size_t N> bool guard(uintptr_t rva, const char (&expected)[N]) noexcept {
    std::array<char,N-1> actual{};
    return bytes(image()+rva,actual.data(),actual.size()) && !std::memcmp(actual.data(),expected,actual.size());
}
bool identity(const CpuIdentity& a, const CpuIdentity& b) noexcept {
    return a.session && a.source && a.submission &&
        std::tie(a.session,a.source,a.submission,a.began_qpc,a.frequency)==
        std::tie(b.session,b.source,b.submission,b.began_qpc,b.frequency);
}
bool close(float a, float b) noexcept {
    return std::isfinite(a) && std::isfinite(b) &&
        std::abs(a-b)<=0.0005f*std::max({1.0f,std::abs(a),std::abs(b)});
}
XMMATRIX matrix(const float* values) noexcept {
    XMFLOAT4X4 copy{}; std::memcpy(&copy,values,sizeof(copy)); return XMLoadFloat4x4(&copy);
}
bool store(FXMMATRIX value, float* output) noexcept {
    XMFLOAT4X4 copy{}; XMStoreFloat4x4(&copy,value);
    for (const auto& row : copy.m) for (float v : row) if (!std::isfinite(v)) return false;
    std::memcpy(output,&copy,sizeof(copy)); return true;
}
bool inverse(FXMMATRIX value, XMMATRIX& output) noexcept {
    XMVECTOR determinant{}; output=XMMatrixInverse(&determinant,value);
    float checked[16]{}; const float det=XMVectorGetX(determinant);
    return std::isfinite(det) && std::abs(det)>1e-8f && store(output,checked);
}
bool matches(FXMMATRIX value, const float* expected) noexcept {
    float actual[16]{};
    if (!store(value,actual)) return false;
    for (unsigned i=0;i<16;++i) if (!close(actual[i],expected[i])) return false;
    return true;
}
bool forward_domain(FXMMATRIX value, float range, MathFailure& detail) noexcept {
    float m[16]{};
    if (!store(value,m) || !(range>0)) return false;
    CameraDepthBound bound{};
    const bool forward=camera_depth_domain_forward(m,range,bound);
    std::memcpy(detail.w,bound.coefficients,sizeof(detail.w));
    detail.minimum_w=bound.minimum; detail.range=bound.range;
    return forward;
}
bool camera(uintptr_t entry, uintptr_t key, unsigned char jitter, NativeDepthCamera& output) noexcept {
    uintptr_t actual_key{}; unsigned char actual_jitter{};
    return read(entry+0x238,actual_key) && actual_key==key && read(entry+0x240,actual_jitter) && actual_jitter==jitter &&
        read(entry+0x50,output.view) && read(entry+0x90,output.projection) &&
        read(entry+0xd0,output.view_projection) && read(entry+0x110,output.view_projection_unjittered) &&
        read(entry+0x210,output.origin) && read(key+0x184,output.port);
}
bool valid_camera(const NativeDepthCamera& value) noexcept {
    for (float v : value.origin) if (!std::isfinite(v)) return false;
    // Camera ports remain native normalized facts; producer raster validates
    // their common allocation-space viewport independently of the ratios.
    for (float v : value.port) if (!std::isfinite(v) || v<0 || v>1) return false;
    if (!(value.port[1]>value.port[0]) || !(value.port[2]>value.port[3])) return false;
    XMMATRIX inv{};
    return inverse(matrix(value.view),inv) && inverse(matrix(value.projection),inv) &&
        inverse(matrix(value.view_projection),inv) && inverse(matrix(value.view_projection_unjittered),inv) &&
        matches(XMMatrixMultiply(matrix(value.view),matrix(value.projection)),value.view_projection);
}
bool sample(const CpuIdentity& cpu, NativeDepthSnapshot& out, Stage& stage) noexcept {
    const auto base=image(), state=base+graphics_rva;
    out.identity=cpu;
    uint32_t tls_index{}; uintptr_t tls_array{}, tls{}, context{}, tls_device_context{}, graph_camera{};
    stage=Stage::tls;
    if (!read(base+0x3e73018,tls_index) || tls_index>4096) return false;
    tls_array=static_cast<uintptr_t>(__readgsqword(0x58));
    if (!read(tls_array+size_t(tls_index)*sizeof(uintptr_t),tls) || !tls ||
        !read(tls+0xb20,context) || !read(tls+0xb18,tls_device_context)) return false;
    if (!context && !read(base+0x38e1398,context)) return false;
    out.context_key=context;
    stage=Stage::owner;
    if (!context || !read(base+0x38e13a0,out.renderer_key) || out.renderer_key!=base+0x3a25d10 ||
        !read(out.renderer_key+0x48,out.device_key) || !out.device_key ||
        !read(out.renderer_key+0x50,out.device_context_key) || !out.device_context_key || tls_device_context!=out.device_context_key ||
        !read(out.renderer_key+0x58,out.window_key) || !out.window_key ||
        !read(out.renderer_key+0x70,out.chain_key) || !out.chain_key) return false;
    uint32_t frame{};
    stage=Stage::dimensions;
    if (!read(state+0x98,frame)) return false;
    out.native_frame=frame;
    if (!read(state+0x78,out.width) || !read(state+0x7c,out.height) ||
        !out.width || !out.height || out.width>16384 || out.height>16384) return false;
    stage=Stage::cache;
    if (!read(state+0x140,out.cache_key) || !out.cache_key ||
        !read(state+0x150,out.cache_count) || !out.cache_count || out.cache_count>256 ||
        !read(state+0xac,out.taa) || !out.taa || !read(state+0xb0,out.disable) || out.disable ||
        !read(state+4,out.jitter) || !read(base+targets_rva+0xfb8,out.ratio) ||
        !read(base+targets_rva+0xfe4,out.default_viewport) || !read(base+targets_rva+0xfe5,out.dynamic_viewport)) return false;
    for (float ratio : out.ratio) if (!std::isfinite(ratio) || ratio<=0 || ratio>1) return false;
    stage=Stage::range;
    if (!read(base+0x3e62250,out.mode) || out.mode ||
        !read(base+0x3e62190,out.intervals[0]) || !read(base+0x2f97b30,out.intervals[1]) ||
        !read(base+0x3e62194,out.intervals[2]) || !read(base+0x2f97b34,out.intervals[3])) return false;
    const auto& r=out.intervals;
    for (float v : r) if (!std::isfinite(v)) return false;
    if (r[0]!=0 || !(r[1]>0 && r[1]<1) || r[2]!=r[1] || r[3]!=1) return false;
    stage=Stage::camera_owner;
    if (!read(base+0x3e622d0,out.near_camera_key) || !out.near_camera_key ||
        !read(base+0x3e62210,out.world_camera_key) || !out.world_camera_key ||
        !read(base+0x32dd338,out.graph_key) || !out.graph_key ||
        !read(out.graph_key+0x140,graph_camera) || graph_camera!=out.world_camera_key) return false;
    stage=Stage::depth_owner;
    if (!read(base+targets_rva+0xf84+4,out.depth_slot) || out.depth_slot>=13 ||
        !read(out.renderer_key+0x1d48+size_t(out.depth_slot)*0x98,out.depth_key) || !out.depth_key ||
        !read(out.renderer_key+0x1d48+size_t(out.depth_slot)*0x98+0x88,out.depth_srv_key) || !out.depth_srv_key ||
        !read(context+0x2410,out.near_inverse_transposed)) return false;
    uintptr_t near_entry{}, world_entry{}, unjittered_entry{};
    for (uint32_t i=0;i<out.cache_count;++i) {
        const auto entry=out.cache_key+size_t(i)*0x250;
        uintptr_t key{}; unsigned char jitter{};
        stage=Stage::cache;
        if (!read(entry+0x238,key) || !read(entry+0x240,jitter)) return false;
        stage=Stage::cache_duplicate;
        if (key==out.near_camera_key && jitter==1) { if (near_entry) return false; near_entry=entry; }
        if (key==out.world_camera_key && jitter==1) { if (world_entry) return false; world_entry=entry; }
        if (key==out.world_camera_key && jitter==0) { if (unjittered_entry) return false; unjittered_entry=entry; }
    }
    stage=Stage::cache_missing;
    if (!near_entry || !world_entry || !unjittered_entry) return false;
    stage=Stage::camera_read;
    return camera(near_entry,out.near_camera_key,1,out.near_camera) &&
        camera(world_entry,out.world_camera_key,1,out.world_camera) &&
        camera(unjittered_entry,out.world_camera_key,0,out.world_unjittered_camera);
}
bool same(const NativeDepthSnapshot& a, const NativeDepthSnapshot& b) noexcept {
    return identity(a.identity,b.identity) &&
        std::tie(a.native_frame,a.renderer_key,a.context_key,a.device_context_key,a.device_key,a.window_key,a.chain_key,
            a.graph_key,a.near_camera_key,a.world_camera_key,a.cache_key,a.depth_key,a.depth_srv_key,a.width,a.height,
            a.cache_count,a.depth_slot,a.taa,a.disable,a.mode,a.default_viewport,a.dynamic_viewport)==
        std::tie(b.native_frame,b.renderer_key,b.context_key,b.device_context_key,b.device_key,b.window_key,b.chain_key,
            b.graph_key,b.near_camera_key,b.world_camera_key,b.cache_key,b.depth_key,b.depth_srv_key,b.width,b.height,
            b.cache_count,b.depth_slot,b.taa,b.disable,b.mode,b.default_viewport,b.dynamic_viewport) &&
        !std::memcmp(a.intervals,b.intervals,sizeof(a.intervals)) && !std::memcmp(a.jitter,b.jitter,sizeof(a.jitter)) &&
        !std::memcmp(a.ratio,b.ratio,sizeof(a.ratio)) &&
        !std::memcmp(a.near_inverse_transposed,b.near_inverse_transposed,sizeof(a.near_inverse_transposed)) &&
        !std::memcmp(&a.near_camera,&b.near_camera,sizeof(a.near_camera)) &&
        !std::memcmp(&a.world_camera,&b.world_camera,sizeof(a.world_camera)) &&
        !std::memcmp(&a.world_unjittered_camera,&b.world_unjittered_camera,sizeof(a.world_unjittered_camera));
}
bool math(const NativeDepthSnapshot& s, XMMATRIX& near_map, XMMATRIX& world_map, XMMATRIX& target_projection,
    Stage& stage, MathFailure& detail) noexcept {
    stage=Stage::camera_math;
    if (!valid_camera(s.near_camera) || !valid_camera(s.world_camera) || !valid_camera(s.world_unjittered_camera)) return false;
    for (float v : s.jitter) if (!std::isfinite(v)) return false;
    const auto& w=s.world_camera;
    const auto& u=s.world_unjittered_camera;
    stage=Stage::world_cache;
    if (std::memcmp(w.origin,u.origin,sizeof(w.origin)) || !matches(matrix(w.view),u.view) ||
        !matches(matrix(w.view_projection_unjittered),u.view_projection) ||
        !matches(matrix(u.view_projection),u.view_projection_unjittered)) return false;
    stage=Stage::near_basis;
    if (!matches(matrix(s.near_camera.view),w.view)) return false;
    XMMATRIX inv_view{}, inv_near_projection{}, inv_world{};
    stage=Stage::near_inverse;
    if (!inverse(matrix(w.view),inv_view) || !inverse(matrix(s.near_camera.projection),inv_near_projection) ||
        !matches(XMMatrixTranspose(inv_near_projection),s.near_inverse_transposed) ||
        !inverse(matrix(w.view_projection),inv_world)) return false;
    target_projection=XMMatrixMultiply(inv_view,matrix(w.view_projection_unjittered));
    stage=Stage::projection_mismatch;
    float recovered[16]{};
    if (!store(target_projection,recovered)) { stage=Stage::finite_map; return false; }
    bool projection_matches=true;
    for (unsigned i=0;i<16;++i) {
        const float error=std::abs(recovered[i]-u.projection[i])/std::max({1.0f,std::abs(recovered[i]),std::abs(u.projection[i])});
        if (error>=detail.relative_error) {
            detail.index=i; detail.actual=recovered[i]; detail.expected=u.projection[i]; detail.relative_error=error;
        }
        if (!close(recovered[i],u.projection[i])) projection_matches=false;
    }
    if (!projection_matches) return false;
    // Native first-person roots supply a separate local camera pose. Describe
    // the composed pixel's view Z, never translate it into physical world space.
    float near_values[16]{};
    stage=Stage::near_basis;
    // The direct jitter0 projection has just passed the unchanged recovered-P
    // comparison; use its native structural zeros instead of roundoff in inv(V)*VP.
    if (!camera_composed_view_depth(s.near_camera.projection,u.projection,near_values)) return false;
    near_map=matrix(near_values);
    world_map=XMMatrixMultiply(inv_world,matrix(w.view_projection_unjittered));
    float checked[16]{};
    stage=Stage::finite_map;
    if (!store(near_map,checked) || !store(world_map,checked) || !store(target_projection,checked)) return false;
    stage=Stage::near_w;
    if (!forward_domain(near_map,s.intervals[1]-s.intervals[0],detail)) return false;
    stage=Stage::world_w;
    return forward_domain(world_map,s.intervals[3]-s.intervals[2],detail);
}
bool same_object(IUnknown* a, IUnknown* b) noexcept {
    ComPtr<IUnknown> left,right;
    return a && b && SUCCEEDED(a->QueryInterface(IID_PPV_ARGS(&left))) &&
        SUCCEEDED(b->QueryInterface(IID_PPV_ARGS(&right))) && left.Get()==right.Get();
}
bool producer_raster(NativeDepthSnapshot& s, Stage& stage, bool temporal=true) noexcept {
    stage=Stage::producer_raster;
    auto* context=reinterpret_cast<ID3D11DeviceContext*>(s.device_context_key);
    if (context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return false;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (!same_object(device.Get(),reinterpret_cast<ID3D11Device*>(s.device_key))) return false;
    UINT count{}; context->RSGetViewports(&count,nullptr);
    if (count!=1) return false;
    D3D11_VIEWPORT actual{}; context->RSGetViewports(&count,&actual);
    if (count!=1) return false;
    const float values[]{actual.TopLeftX,actual.TopLeftY,actual.Width,actual.Height,actual.MinDepth,actual.MaxDepth};
    std::memcpy(s.actual_raster,values,sizeof(values));
    for (float v : values) if (!std::isfinite(v)) return false;
    if (actual.TopLeftX<0 || actual.TopLeftY<0 || actual.Width<=0 || actual.Height<=0 ||
        double(actual.TopLeftX)+actual.Width>s.width || double(actual.TopLeftY)+actual.Height>s.height ||
        actual.MinDepth<0 || actual.MaxDepth>1 || actual.MinDepth>=actual.MaxDepth) return false;
    // Half-open viewport support evaluated at source texel centers.
    s.render_rect[0]=int32_t(std::ceil(double(actual.TopLeftX)-0.5));
    s.render_rect[1]=int32_t(std::ceil(double(actual.TopLeftY)-0.5));
    s.render_rect[2]=int32_t(std::ceil(double(actual.TopLeftX)+actual.Width-0.5));
    s.render_rect[3]=int32_t(std::ceil(double(actual.TopLeftY)+actual.Height-0.5));
    if (s.render_rect[2]<=s.render_rect[0] || s.render_rect[3]<=s.render_rect[1]) return false;
    stage=Stage::producer_target;
    ComPtr<ID3D11RenderTargetView> rtv;
    ComPtr<ID3D11DepthStencilView> dsv; context->OMGetRenderTargets(1,&rtv,&dsv);
    if (!dsv) return false;
    s.depth_dsv_key=reinterpret_cast<uintptr_t>(dsv.Get());
    ComPtr<ID3D11Resource> depth; dsv->GetResource(&depth);
    if (!same_object(depth.Get(),reinterpret_cast<ID3D11Texture2D*>(s.depth_key))) return false;
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(depth.As(&texture))) return false;
    D3D11_TEXTURE2D_DESC allocation{}; texture->GetDesc(&allocation);
    if (allocation.Width!=s.width || allocation.Height!=s.height || allocation.Format!=DXGI_FORMAT_R24G8_TYPELESS ||
        allocation.MipLevels!=1 || allocation.ArraySize!=1 || allocation.SampleDesc.Count!=1 || allocation.SampleDesc.Quality) return false;
    D3D11_DEPTH_STENCIL_VIEW_DESC ds{}; dsv->GetDesc(&ds);
    if (ds.Format!=DXGI_FORMAT_D24_UNORM_S8_UINT || ds.ViewDimension!=D3D11_DSV_DIMENSION_TEXTURE2D || ds.Texture2D.MipSlice) return false;
    const auto shadow=s.context_key+0x1b70, targets=image()+targets_rva;
    stage=Stage::producer_selector;
    uint32_t cube{};
    if (!read(shadow,s.shadow_dirty) || !read(shadow+0x90,s.shadow_raster) ||
        !read(shadow+0x6d0,s.normalized_port) || !read(shadow+0x84,s.colour_selector) ||
        !read(shadow+0x88,s.depth_selector) || !read(shadow+0x8c,s.cube_selector) || !read(shadow+0x54,cube)) return false;
    s.cube_key=cube;
    if (cube!=UINT32_MAX || s.depth_selector!=1 || (s.colour_selector>=100 && s.colour_selector!=UINT32_MAX)) return false;
    // Native width/height getters use the logical slot0 selector, not Main's ID.
    // SetCurrentRenderTarget separately maps that ID to the physical OM slot.
    stage=Stage::producer_descriptor;
    const auto descriptor=s.colour_selector!=UINT32_MAX?targets+size_t(s.colour_selector)*0x20:targets+0xc80+0x1c;
    if (!read(descriptor,s.descriptor_width) || !read(descriptor+4,s.descriptor_height)) return false;
    s.descriptor_read=true;
    if (s.descriptor_width!=s.width || s.descriptor_height!=s.height) return false;
    stage=Stage::producer_target_dirty;
    if (s.shadow_dirty&1) return false;
    stage=Stage::producer_target_map;
    if (!read(shadow+0x2c,s.bound_colour_slot) || !read(shadow+0x4c,s.bound_depth_slot) ||
        s.bound_depth_slot!=s.depth_slot) return false;
    s.colour_slot=UINT32_MAX;
    if (s.colour_selector!=UINT32_MAX &&
        (!read(targets+0xdf4+size_t(s.colour_selector)*4,s.colour_slot) || s.colour_slot>=101)) return false;
    if (s.bound_colour_slot!=s.colour_slot) return false;
    stage=Stage::producer_colour;
    if (s.colour_selector==UINT32_MAX) {
        if (rtv) return false;
    } else {
        uintptr_t native_texture{},native_rtv{};
        const auto target=s.renderer_key+0xa58+size_t(s.colour_slot)*0x30;
        if (!read(target,native_texture) || !native_texture || !read(target+0x10,native_rtv) || !native_rtv ||
            !rtv || !same_object(rtv.Get(),reinterpret_cast<ID3D11RenderTargetView*>(native_rtv))) return false;
        ComPtr<ID3D11Resource> colour; rtv->GetResource(&colour);
        ComPtr<ID3D11Texture2D> colour_texture;
        if (!colour || FAILED(colour.As(&colour_texture)) ||
            !same_object(colour_texture.Get(),reinterpret_cast<ID3D11Texture2D*>(native_texture))) return false;
        ComPtr<ID3D11Device> colour_device; colour_texture->GetDevice(&colour_device);
        D3D11_TEXTURE2D_DESC colour_allocation{}; colour_texture->GetDesc(&colour_allocation);
        D3D11_RENDER_TARGET_VIEW_DESC colour_view{}; rtv->GetDesc(&colour_view);
        if (!same_object(colour_device.Get(),device.Get()) || colour_allocation.Width!=s.descriptor_width ||
            colour_allocation.Height!=s.descriptor_height || colour_allocation.MipLevels!=1 ||
            colour_allocation.ArraySize!=1 || colour_allocation.SampleDesc.Count!=1 || colour_allocation.SampleDesc.Quality ||
            colour_view.ViewDimension!=D3D11_RTV_DIMENSION_TEXTURE2D || colour_view.Texture2D.MipSlice) return false;
        s.colour_texture_key=native_texture; s.colour_rtv_key=native_rtv;
    }
    stage=Stage::producer_shadow;
    for (float v : s.shadow_raster) if (!std::isfinite(v)) return false;
    for (unsigned i=0;i<4;++i) if (s.shadow_raster[i]!=s.actual_raster[i] ||
        (temporal && (s.normalized_port[i]!=s.world_camera.port[i] || s.near_camera.port[i]!=s.world_camera.port[i] ||
        s.world_unjittered_camera.port[i]!=s.world_camera.port[i]))) return false;
    const auto& port=s.normalized_port;
    for (float value:port) if (!std::isfinite(value) || value<0 || value>1) return false;
    if (!(port[1]>port[0]) || !(port[2]>port[3])) return false;
    if (!s.default_viewport || ((s.ratio[0]<1 || s.ratio[1]<1) && !s.dynamic_viewport) ||
        actual.TopLeftX!=float(s.width)*port[0] || actual.TopLeftY!=float(s.height)*(1-port[2]) ||
        actual.Width>float(s.width)*(port[1]-port[0]) || actual.Height>float(s.height)*(port[2]-port[3])) return false;
    stage=Stage::producer_scissor;
    ComPtr<ID3D11RasterizerState> rasterizer; context->RSGetState(&rasterizer);
    D3D11_RASTERIZER_DESC rs{}; if (rasterizer) rasterizer->GetDesc(&rs);
    s.scissor_enabled=rs.ScissorEnable!=FALSE;
    if (rs.ScissorEnable) {
        UINT scissors{}; context->RSGetScissorRects(&scissors,nullptr);
        if (scissors!=1) return false;
        D3D11_RECT rectangle{}; context->RSGetScissorRects(&scissors,&rectangle);
        if (scissors!=1 || rectangle.left>s.render_rect[0] || rectangle.top>s.render_rect[1] ||
            rectangle.right<s.render_rect[2] || rectangle.bottom<s.render_rect[3]) return false;
        s.scissor[0]=rectangle.left; s.scissor[1]=rectangle.top;
        s.scissor[2]=rectangle.right; s.scissor[3]=rectangle.bottom;
    }
    return true;
}
// Only native owner/raster prerequisites. No temporal cache, projection, history
// or CPU sample-validity predicate is admitted through this boundary.
bool raster_owner(const CpuIdentity& cpu, NativeDepthSnapshot& out) noexcept {
    const auto base=image(), state=base+graphics_rva, targets=base+targets_rva;
    out.identity=cpu;
    uint32_t index{},frame{}; uintptr_t block{},tls_device{};
    if (!read(base+0x3e73018,index) || index>4096) return false;
    const auto slots=static_cast<uintptr_t>(__readgsqword(0x58));
    if (!slots || !read(slots+size_t(index)*sizeof(uintptr_t),block) || !block ||
        !read(block+0xb20,out.context_key) || !read(block+0xb18,tls_device)) return false;
    if (!out.context_key && !read(base+0x38e1398,out.context_key)) return false;
    if (!out.context_key || !read(base+0x38e13a0,out.renderer_key) || out.renderer_key!=base+0x3a25d10 ||
        !read(out.renderer_key+0x48,out.device_key) || !out.device_key ||
        !read(out.renderer_key+0x50,out.device_context_key) || !out.device_context_key || tls_device!=out.device_context_key ||
        !read(out.renderer_key+0x58,out.window_key) || !out.window_key ||
        !read(out.renderer_key+0x70,out.chain_key) || !out.chain_key ||
        !read(state+0x98,frame) || !read(state+0x78,out.width) || !read(state+0x7c,out.height) ||
        !out.width || !out.height || out.width>16384 || out.height>16384 ||
        !read(base+0x32dd338,out.graph_key) || !out.graph_key ||
        !read(out.graph_key+0x140,out.world_camera_key) || !out.world_camera_key ||
        !read(targets+0xfb8,out.ratio) || !read(targets+0xfe4,out.default_viewport) ||
        !read(targets+0xfe5,out.dynamic_viewport) ||
        !read(targets+0xf84+4,out.depth_slot) || out.depth_slot>=13 ||
        !read(out.renderer_key+0x1d48+size_t(out.depth_slot)*0x98,out.depth_key) || !out.depth_key ||
        !read(out.renderer_key+0x1d48+size_t(out.depth_slot)*0x98+0x88,out.depth_srv_key) || !out.depth_srv_key) return false;
    out.native_frame=frame;
    for (float v:out.ratio) if (!std::isfinite(v) || v<=0 || v>1) return false;
    return true;
}
bool same_raster_owner(const NativeDepthSnapshot& a,const NativeDepthSnapshot& b) noexcept {
    return identity(a.identity,b.identity) &&
        std::tie(a.native_frame,a.renderer_key,a.context_key,a.device_context_key,a.device_key,a.window_key,a.chain_key,
            a.graph_key,a.world_camera_key,a.width,a.height,a.depth_slot,a.depth_key,a.depth_srv_key)==
        std::tie(b.native_frame,b.renderer_key,b.context_key,b.device_context_key,b.device_key,b.window_key,b.chain_key,
            b.graph_key,b.world_camera_key,b.width,b.height,b.depth_slot,b.depth_key,b.depth_srv_key);
}
bool same_raster(const NativeDepthSnapshot& a, const NativeDepthSnapshot& b) noexcept {
    return !std::memcmp(a.render_rect,b.render_rect,sizeof(a.render_rect)) &&
        !std::memcmp(a.actual_raster,b.actual_raster,sizeof(a.actual_raster)) &&
        !std::memcmp(a.shadow_raster,b.shadow_raster,sizeof(a.shadow_raster)) &&
        !std::memcmp(a.normalized_port,b.normalized_port,sizeof(a.normalized_port)) &&
        a.descriptor_read==b.descriptor_read &&
        a.scissor_enabled==b.scissor_enabled && !std::memcmp(a.scissor,b.scissor,sizeof(a.scissor)) &&
        std::tie(a.shadow_dirty,a.colour_selector,a.depth_selector,a.cube_selector,a.descriptor_width,a.descriptor_height,a.cube_key,a.depth_dsv_key,
            a.colour_slot,a.bound_colour_slot,a.bound_depth_slot,a.colour_texture_key,a.colour_rtv_key)==
        std::tie(b.shadow_dirty,b.colour_selector,b.depth_selector,b.cube_selector,b.descriptor_width,b.descriptor_height,b.cube_key,b.depth_dsv_key,
            b.colour_slot,b.bound_colour_slot,b.bound_depth_slot,b.colour_texture_key,b.colour_rtv_key);
}
}

bool capture_native_raster(const CpuIdentity& cpu, NativeDepthSnapshot& output, void (*report)(const char*)) noexcept {
    output={}; NativeDepthSnapshot before{},after{}; Stage stage=Stage::source;
    if (!verified.load(std::memory_order_acquire) || !identity(cpu,current_cpu_identity()) ||
        !raster_owner(cpu,before) || !producer_raster(before,stage,false) ||
        !raster_owner(cpu,after) || !producer_raster(after,stage,false) ||
        !same_raster_owner(before,after) || !same_raster(before,after) ||
        std::memcmp(before.ratio,after.ratio,sizeof(before.ratio)) ||
        before.default_viewport!=after.default_viewport || before.dynamic_viewport!=after.dynamic_viewport ||
        !identity(cpu,current_cpu_identity())) return refuse(stage,before,report);
    before.raster_valid=true; output=before; return true;
}
bool native_raster_snapshot_current(const NativeDepthSnapshot& snapshot) noexcept {
    NativeDepthSnapshot current{};
    return verified.load(std::memory_order_acquire) && snapshot.raster_valid &&
        identity(snapshot.identity,current_cpu_identity()) && raster_owner(snapshot.identity,current) &&
        same_raster_owner(snapshot,current);
}

bool native_depth_snapshot_current(const NativeDepthSnapshot& snapshot) noexcept {
    Stage stage=Stage::source; NativeDepthSnapshot current{};
    return verified.load(std::memory_order_acquire) && snapshot.valid &&
        identity(snapshot.identity,current_cpu_identity()) && cpu_source_is_current(snapshot.identity) &&
        sample(snapshot.identity,current,stage) && same(snapshot,current) && cpu_source_is_current(snapshot.identity);
}

bool verify_native_depth_domains() noexcept {
    static_assert(sizeof(rsf_game_depth_domain)==104 && sizeof(rsf_game_depth_domains)==240);
    const bool okay=
        guard(0x21f38b0,"\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10") &&
        guard(0x21f9f69,"\xe8\x42\x91\xff\xff") && guard(0x21f9f73,"\xe8\x38\x99\xff\xff") &&
        guard(0x21f3294,"\x48\x8b\x15\x35\xf0\xc6\x01\x48\x8d\x0d\xfe\x3e\xb9\x01\x41\xb0\x01\xe8\x76\x8c\x63\xff") &&
        guard(0x21f32aa,"\x48\x8b\x15\x5f\xef\xc6\x01\x48\x8d\x0d\xe8\x3e\xb9\x01\x41\xb0\x01\xe8\x60\x8c\x63\xff") &&
        guard(0x21f32c0,"\x48\x8b\x15\x49\xef\xc6\x01\x48\x8d\x0d\xd2\x3e\xb9\x01\x45\x33\xc0\xe8\x4a\x8c\x63\xff") &&
        guard(0x182c02d,"\xe8\x1e\x17\x00\x00") &&
        guard(0x182bf4a,"\x48\x8d\xb1\x40\x01\x00\x00\x8b\x46\x10") &&
        guard(0x182bf6a,"\x49\x39\xb8\x38\x02\x00\x00\x75\x09\x41\x38\x98\x40\x02\x00\x00") &&
        guard(0x182d9d9,"\xe8\x52\x8a\xb0\xfe\x0f\x11\x83\xd0\x00\x00\x00\x0f\x11\x83\x10\x01\x00\x00") &&
        guard(0x182da16,"\x41\x83\xbe\xac\x00\x00\x00\x00\x0f\x84\xc5\x00\x00\x00\x40\x84\xed\x0f\x84\xbc\x00\x00\x00") &&
        guard(0x21f38d1,"\x44\x8b\x05\x40\xf7\xc7\x01") &&
        guard(0x21f3920,"\x65\x48\x8b\x04\x25\x58\x00\x00\x00\x41\x8b\xf1\x4a\x8b\x3c\xc0\x49\x8b\x04\x39") &&
        guard(0x21f3939,"\x48\x8b\x05\x58\xda\x6e\x01\x48\x05\x70\x1b\x00\x00") &&
        guard(0x21f3b36,"\x0f\x10\x80\x60\x07\x00\x00\x48\x8d\x4c\x24\x40\x0f\x10\x88\x70\x07\x00\x00\x0f\x10\x90\x80\x07\x00\x00\x0f\x10\x98\x90\x07\x00\x00\xe8\xe4\x25\x14\xfe") &&
        guard(0x21f3b94,"\x0f\x11\x9b\xa0\x08\x00\x00\x0f\x11\xa3\xb0\x08\x00\x00\x0f\x11\x8b\xc0\x08\x00\x00\x0f\x11\x83\xd0\x08\x00\x00") &&
        guard(0x21f3db6,"\xe8\xc5\x7e\x63\xff") &&
        guard(0x182db0f,"\x89\x83\x10\x02\x00\x00\x8b\x86\xa4\x00\x00\x00\x89\x83\x14\x02\x00\x00\x8b\x86\xa8\x00\x00\x00\x89\x83\x18\x02\x00\x00\x48\x89\xb3\x38\x02\x00\x00") &&
        guard(0x182db38,"\x40\x88\xab\x40\x02\x00\x00") &&
        guard(0x21f0e25,"\x84\xc0\x75\x04\x44\x0f\x28\xc8") &&
        guard(0x21f0e2d,"\xf3\x44\x0f\x11\x0d\xfa\x6c\xda\x00") &&
        guard(0x21f0e44,"\xf3\x0f\x11\x35\x48\x13\xc7\x01") &&
        guard(0x183985b,"\x48\x63\x88\xf4\x1b\x00\x00") &&
        guard(0x1839868,"\x85\xc9\x78\x0c\x48\x8b\xc1\x48\xc1\xe0\x05\x42\x8b\x04\x00") &&
        guard(0x1839bc9,"\x85\xd2\x75\x07\x44\x89\x83\x84\x00\x00\x00") &&
        guard(0x1839bdf,"\x49\x63\xc0\x8b\xb4\x81\xf4\x0d\x00\x00") &&
        guard(0x1839c3c,"\x89\x74\xab\x2c\xc7\x43\x54\xff\xff\xff\xff\x89\x7c\xab\x5c\x83\x0b\x01") &&
        guard(0x1839cb1,"\x48\x63\xc2\x41\x89\x92\x88\x00\x00\x00\x8b\x8c\x81\x84\x0f\x00\x00") &&
        guard(0x1823db7,"\x48\x63\x47\xd0\x83\xf8\xff\x74\x47\x48\x8d\x0c\x40\x48\x8b\x05\xd5\xd5\x0b\x02\x48\x03\xc9\x83\x3f\x00\x48\x8b\x94\xc8\x68\x0a\x00\x00") &&
        guard(0x1823f14,"\x48\x8b\x01\xff\x90\x08\x01\x00\x00") &&
        guard(0x1839878,"\x48\x63\x88\x88\x00\x00\x00") &&
        guard(0x1839883,"\x48\x6b\xc9\x1c\x42\x8b\x84\x01\x80\x0c\x00\x00") &&
        guard(0x18398df,"\x48\xc1\xe0\x05\x42\x8b\x44\x00\x04") &&
        guard(0x181805f,"\x83\x0b\x02\x89\x83\xa4\x00\x00\x00") &&
        guard(0x182430d,"\x4c\x8d\x87\x90\x00\x00\x00\xba\x01\x00\x00\x00\x48\x8b\x01\xff\x90\x60\x01\x00\x00");
    verified.store(okay,std::memory_order_release); return okay;
}

bool capture_native_depth_domains(const CpuIdentity& cpu, NativeDepthSnapshot& output, void (*report)(const char*)) noexcept {
    output={}; NativeDepthSnapshot before{}, after{};
    before.identity=cpu;
    Stage stage=Stage::unverified;
    MathFailure detail{};
    XMMATRIX near_map{},world_map{},projection{};
    if (!verified.load(std::memory_order_acquire)) return refuse(stage,before,report);
    if (!identity(cpu,current_cpu_identity()) || !cpu_source_is_current(cpu)) return refuse(Stage::source,before,report);
    if (!sample(cpu,before,stage) || !producer_raster(before,stage) || !math(before,near_map,world_map,projection,stage,detail)) return refuse(stage,before,report,&detail);
    if (!sample(cpu,after,stage) || !producer_raster(after,stage)) return refuse(stage,after,report);
    if (!same(before,after) || !same_raster(before,after)) return refuse(Stage::state_changed,after,report);
    if (!identity(cpu,current_cpu_identity()) || !cpu_source_is_current(cpu)) return refuse(Stage::source,after,report);
    before.valid=true; output=before; return true;
}

bool make_native_depth_domains(const NativeDepthSnapshot& snapshot, const CpuIdentity& cpu,
    const rsf_game_render_pass& pass, rsf_game_depth_domains& output, void (*report)(const char*)) noexcept {
    output={};
    if (!verified.load(std::memory_order_acquire)) return refuse(Stage::unverified,snapshot,report);
    if (!snapshot.valid || !identity(snapshot.identity,cpu) || !identity(cpu,current_cpu_identity()) ||
        !cpu_source_is_current(cpu)) return refuse(Stage::source,snapshot,report);
    if (pass.struct_size<sizeof(pass) || pass.role!=RSF_GAME_RENDER_SR || !(pass.flags&RSF_GAME_RENDER_PRIMARY) ||
        pass.session_id!=cpu.session || pass.source_frame_id!=cpu.source || pass.submission_id!=cpu.submission ||
        pass.scope_id!=cpu.submission || pass.native_frame!=snapshot.native_frame ||
        pass.view_key!=snapshot.world_camera_key || pass.family_key!=snapshot.world_camera_key ||
        pass.window_key!=snapshot.window_key || pass.viewport_key!=snapshot.window_key || pass.rhi_viewport_key!=snapshot.window_key ||
        reinterpret_cast<uintptr_t>(pass.depth)!=snapshot.depth_key || reinterpret_cast<uintptr_t>(pass.swapchain)!=snapshot.chain_key ||
        !pass.camera_valid || pass.camera.struct_size<sizeof(pass.camera) || pass.camera.abi_version!=RSF_GAME_CAMERA_ABI_VERSION ||
        pass.camera.depth_inverted || pass.camera.render_width!=uint32_t(snapshot.render_rect[2]-snapshot.render_rect[0]) ||
        pass.camera.render_height!=uint32_t(snapshot.render_rect[3]-snapshot.render_rect[1]) ||
        pass.camera.output_width!=snapshot.width || pass.camera.output_height!=snapshot.height) return refuse(Stage::pass_association,snapshot,report);
    for (unsigned i=0;i<4;++i) {
        const int32_t expected=i<2?0:static_cast<int32_t>(i==2?snapshot.width:snapshot.height);
        if (pass.render_rect[i]!=snapshot.render_rect[i] || pass.output_rect[i]!=expected) return refuse(Stage::pass_association,snapshot,report);
    }
    NativeDepthSnapshot current{},confirmed{};
    Stage stage=Stage::source;
    MathFailure detail{};
    XMMATRIX near_map{},world_map{},projection{},inverse_projection{},view_world{};
    if (!sample(cpu,current,stage)) return refuse(stage,current,report);
    if (!same(snapshot,current)) return refuse(Stage::state_changed,current,report);
    if (!math(snapshot,near_map,world_map,projection,stage,detail)) return refuse(stage,snapshot,report,&detail);
    float native_projection[16]{}, affine[16]{};
    CameraRaster raster{};
    std::memcpy(raster.xywh,snapshot.actual_raster,sizeof(raster.xywh));
    std::memcpy(raster.crop,snapshot.render_rect,sizeof(raster.crop));
    if (!store(projection,native_projection)) return refuse(Stage::finite_map,snapshot,report);
    if (!camera_raster_transform(raster,affine)) return refuse(Stage::raster_math,snapshot,report);
    projection=XMMatrixMultiply(projection,matrix(affine));
    near_map=XMMatrixMultiply(near_map,matrix(affine));
    world_map=XMMatrixMultiply(world_map,matrix(affine));
    if (!inverse(projection,inverse_projection) || !matches(projection,pass.camera.view_to_clip) ||
        !matches(inverse_projection,pass.camera.clip_to_view)) return refuse(Stage::target_camera,snapshot,report);
    const auto& w=snapshot.world_camera;
    const auto world_view=XMMatrixMultiply(XMMatrixTranslation(-w.origin[0],-w.origin[1],-w.origin[2]),matrix(w.view));
    if (!matches(world_view,pass.camera.world_to_view) || !inverse(world_view,view_world) ||
        !matches(view_world,pass.camera.view_to_world)) return refuse(Stage::target_camera,snapshot,report);
    float target[16]{};
    if (!store(projection,target)) return refuse(Stage::target_camera,snapshot,report);
    for (unsigned i=0;i<16;++i) if (i!=8 && i!=9 && !close(w.projection[i],native_projection[i])) return refuse(Stage::target_camera,snapshot,report);
    if (!close(pass.camera.jitter_pixels[0],(w.projection[8]-native_projection[8])*snapshot.actual_raster[2]*0.5f) ||
        !close(pass.camera.jitter_pixels[1],-(w.projection[9]-native_projection[9])*snapshot.actual_raster[3]*0.5f)) return refuse(Stage::target_camera,snapshot,report);
    auto* depth=static_cast<ID3D11Texture2D*>(pass.depth);
    D3D11_TEXTURE2D_DESC desc{}; depth->GetDesc(&desc);
    ComPtr<ID3D11Device> device; depth->GetDevice(&device);
    if (desc.Format!=DXGI_FORMAT_R24G8_TYPELESS || desc.Width!=snapshot.width || desc.Height!=snapshot.height ||
        desc.MipLevels!=1 || desc.ArraySize!=1 || desc.SampleDesc.Count!=1 || desc.SampleDesc.Quality ||
        !same_object(device.Get(),reinterpret_cast<ID3D11Device*>(current.device_key))) return refuse(Stage::depth_format,snapshot,report);
    auto* srv=reinterpret_cast<ID3D11ShaderResourceView*>(current.depth_srv_key);
    D3D11_SHADER_RESOURCE_VIEW_DESC view{}; srv->GetDesc(&view);
    ComPtr<ID3D11Resource> viewed; srv->GetResource(&viewed);
    ComPtr<ID3D11Device> view_device; srv->GetDevice(&view_device);
    if (view.Format!=DXGI_FORMAT_R24_UNORM_X8_TYPELESS || view.ViewDimension!=D3D11_SRV_DIMENSION_TEXTURE2D ||
        view.Texture2D.MostDetailedMip || view.Texture2D.MipLevels!=1 ||
        !same_object(viewed.Get(),depth) || !same_object(view_device.Get(),device.Get())) return refuse(Stage::depth_view,snapshot,report);
    rsf_game_depth_domains result{};
    result.struct_size=sizeof(result); result.version=RSF_DEPTH_DOMAINS_VERSION; result.domain_count=2;
    result.clear_enabled=1; result.clear_value=1;
    for (unsigned i=0;i<2;++i) {
        auto& domain=result.domains[i];
        domain.classify_min=i?snapshot.intervals[1]:0; domain.classify_max=i?1:snapshot.intervals[1];
        // Explicit reference-aligned ownership policy, not primitive provenance.
        domain.endpoint_flags=RSF_DEPTH_DOMAIN_INCLUDE_MAX | (i?0u:RSF_DEPTH_DOMAIN_INCLUDE_MIN);
        std::memcpy(domain.viewport_xywh,snapshot.actual_raster,sizeof(domain.viewport_xywh));
        domain.viewport_min_depth=snapshot.intervals[i*2]; domain.viewport_max_depth=snapshot.intervals[i*2+1];
        if (!store(i?world_map:near_map,domain.source_clip_to_target_clip)) return refuse(Stage::finite_map,snapshot,report);
    }
    if (!sample(cpu,confirmed,stage)) return refuse(stage,confirmed,report);
    if (!same(snapshot,confirmed)) return refuse(Stage::state_changed,confirmed,report);
    if (!identity(cpu,current_cpu_identity()) || !cpu_source_is_current(cpu)) return refuse(Stage::source,confirmed,report);
    output=result; return true;
}
}
