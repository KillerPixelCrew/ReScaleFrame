// SPDX-License-Identifier: GPL-3.0-only
#include "depth_domains.h"
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
    target_math, pass_association, target_camera, depth_format, depth_view, state_changed, count
};
constexpr const char* names[]{"unverified","source","tls","owner","dimensions","cache","range",
    "camera-owner","depth-owner","cache-duplicate","cache-missing","camera-read","camera-math",
    "world-cache","near-inverse","target-math","pass-association","target-camera","depth-format","depth-view","state-changed"};
static_assert(std::size(names)==unsigned(Stage::count) && unsigned(Stage::count)<=64);
std::atomic<uint64_t> reported{};
bool refuse(Stage stage, const NativeDepthSnapshot& s, void (*report)(const char*)) noexcept {
    if (report) try {
        const auto bit=uint64_t(1)<<unsigned(stage);
        if (!(reported.fetch_or(bit,std::memory_order_relaxed)&bit)) {
            char text[400]{};
            std::snprintf(text,sizeof(text),
                "Fallout4 depth stage=%s source=%llu submission=%llu native=%llu mode=%u range=%g,%g/%g,%g",
                names[unsigned(stage)],static_cast<unsigned long long>(s.identity.source),
                static_cast<unsigned long long>(s.identity.submission),static_cast<unsigned long long>(s.native_frame),
                unsigned(s.mode),double(s.intervals[0]),double(s.intervals[1]),double(s.intervals[2]),double(s.intervals[3]));
            report(text);
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
bool forward_domain(FXMMATRIX value, float range) noexcept {
    float m[16]{};
    if (!store(value,m) || !(range>0)) return false;
    // Include one D24 step beyond each unmapping endpoint. W is affine,
    // so its minimum over the clip box is attained at a corner.
    const double epsilon=1.0/(16777215.0*double(range));
    const double low=double(m[15])-std::abs(double(m[3]))-std::abs(double(m[7]))+
        std::min(-epsilon*double(m[11]),(1.0+epsilon)*double(m[11]));
    return std::isfinite(low) && low>1e-6;
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
    // Initial mapping is only the existing full native raster path.
    constexpr float full[]{0,1,1,0};
    for (unsigned i=0;i<4;++i) if (value.port[i]!=full[i]) return false;
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
        !read(state+4,out.jitter) || !read(base+targets_rva+0xfb8,out.ratio) || out.ratio[0]!=1 || out.ratio[1]!=1) return false;
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
            a.cache_count,a.depth_slot,a.taa,a.disable,a.mode)==
        std::tie(b.native_frame,b.renderer_key,b.context_key,b.device_context_key,b.device_key,b.window_key,b.chain_key,
            b.graph_key,b.near_camera_key,b.world_camera_key,b.cache_key,b.depth_key,b.depth_srv_key,b.width,b.height,
            b.cache_count,b.depth_slot,b.taa,b.disable,b.mode) &&
        !std::memcmp(a.intervals,b.intervals,sizeof(a.intervals)) && !std::memcmp(a.jitter,b.jitter,sizeof(a.jitter)) &&
        !std::memcmp(a.ratio,b.ratio,sizeof(a.ratio)) &&
        !std::memcmp(a.near_inverse_transposed,b.near_inverse_transposed,sizeof(a.near_inverse_transposed)) &&
        !std::memcmp(&a.near_camera,&b.near_camera,sizeof(a.near_camera)) &&
        !std::memcmp(&a.world_camera,&b.world_camera,sizeof(a.world_camera)) &&
        !std::memcmp(&a.world_unjittered_camera,&b.world_unjittered_camera,sizeof(a.world_unjittered_camera));
}
bool math(const NativeDepthSnapshot& s, XMMATRIX& near_map, XMMATRIX& world_map, XMMATRIX& target_projection, Stage& stage) noexcept {
    stage=Stage::camera_math;
    if (!valid_camera(s.near_camera) || !valid_camera(s.world_camera) || !valid_camera(s.world_unjittered_camera)) return false;
    for (float v : s.jitter) if (!std::isfinite(v)) return false;
    const auto& w=s.world_camera;
    const auto& u=s.world_unjittered_camera;
    stage=Stage::world_cache;
    if (std::memcmp(w.origin,u.origin,sizeof(w.origin)) || !matches(matrix(w.view),u.view) ||
        !matches(matrix(w.view_projection_unjittered),u.view_projection) ||
        !matches(matrix(u.view_projection),u.view_projection_unjittered)) return false;
    XMMATRIX inv_view{}, inv_near_projection{}, inv_near{}, inv_world{};
    stage=Stage::near_inverse;
    if (!inverse(matrix(w.view),inv_view) || !inverse(matrix(s.near_camera.projection),inv_near_projection) ||
        !matches(XMMatrixTranspose(inv_near_projection),s.near_inverse_transposed) ||
        !inverse(matrix(s.near_camera.view_projection),inv_near) || !inverse(matrix(w.view_projection),inv_world)) return false;
    target_projection=XMMatrixMultiply(inv_view,matrix(w.view_projection_unjittered));
    stage=Stage::target_math;
    if (!matches(target_projection,u.projection)) return false;
    const auto& n=s.near_camera.origin;
    const auto& o=w.origin;
    const auto delta=XMMatrixTranslation(n[0]-o[0],n[1]-o[1],n[2]-o[2]);
    near_map=XMMatrixMultiply(XMMatrixMultiply(inv_near,delta),matrix(w.view_projection_unjittered));
    world_map=XMMatrixMultiply(inv_world,matrix(w.view_projection_unjittered));
    float checked[16]{};
    return store(near_map,checked) && store(world_map,checked) && store(target_projection,checked) &&
        forward_domain(near_map,s.intervals[1]-s.intervals[0]) &&
        forward_domain(world_map,s.intervals[3]-s.intervals[2]);
}
bool same_object(IUnknown* a, IUnknown* b) noexcept {
    ComPtr<IUnknown> left,right;
    return a && b && SUCCEEDED(a->QueryInterface(IID_PPV_ARGS(&left))) &&
        SUCCEEDED(b->QueryInterface(IID_PPV_ARGS(&right))) && left.Get()==right.Get();
}
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
        guard(0x21f0e44,"\xf3\x0f\x11\x35\x48\x13\xc7\x01");
    verified.store(okay,std::memory_order_release); return okay;
}

bool capture_native_depth_domains(const CpuIdentity& cpu, NativeDepthSnapshot& output, void (*report)(const char*)) noexcept {
    output={}; NativeDepthSnapshot before{}, after{};
    before.identity=cpu;
    Stage stage=Stage::unverified;
    XMMATRIX near_map{},world_map{},projection{};
    if (!verified.load(std::memory_order_acquire)) return refuse(stage,before,report);
    if (!identity(cpu,current_cpu_identity()) || !cpu_source_is_current(cpu)) return refuse(Stage::source,before,report);
    if (!sample(cpu,before,stage) || !math(before,near_map,world_map,projection,stage)) return refuse(stage,before,report);
    if (!sample(cpu,after,stage)) return refuse(stage,after,report);
    if (!same(before,after)) return refuse(Stage::state_changed,after,report);
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
        pass.camera.depth_inverted || pass.camera.render_width!=snapshot.width || pass.camera.render_height!=snapshot.height ||
        pass.camera.output_width!=snapshot.width || pass.camera.output_height!=snapshot.height) return refuse(Stage::pass_association,snapshot,report);
    for (unsigned i=0;i<4;++i) {
        const int32_t expected=i<2?0:static_cast<int32_t>(i==2?snapshot.width:snapshot.height);
        if (pass.render_rect[i]!=expected || pass.output_rect[i]!=expected) return refuse(Stage::pass_association,snapshot,report);
    }
    NativeDepthSnapshot current{},confirmed{};
    Stage stage=Stage::source;
    XMMATRIX near_map{},world_map{},projection{},inverse_projection{},view_world{};
    if (!sample(cpu,current,stage)) return refuse(stage,current,report);
    if (!same(snapshot,current)) return refuse(Stage::state_changed,current,report);
    if (!math(snapshot,near_map,world_map,projection,stage)) return refuse(stage,snapshot,report);
    if (!inverse(projection,inverse_projection) || !matches(projection,pass.camera.view_to_clip) ||
        !matches(inverse_projection,pass.camera.clip_to_view)) return refuse(Stage::target_camera,snapshot,report);
    const auto& w=snapshot.world_camera;
    const auto world_view=XMMatrixMultiply(XMMatrixTranslation(-w.origin[0],-w.origin[1],-w.origin[2]),matrix(w.view));
    if (!matches(world_view,pass.camera.world_to_view) || !inverse(world_view,view_world) ||
        !matches(view_world,pass.camera.view_to_world)) return refuse(Stage::target_camera,snapshot,report);
    float target[16]{};
    if (!store(projection,target)) return refuse(Stage::target_camera,snapshot,report);
    for (unsigned i=0;i<16;++i) if (i!=8 && i!=9 && !close(w.projection[i],target[i])) return refuse(Stage::target_camera,snapshot,report);
    if (!close(pass.camera.jitter_pixels[0],(w.projection[8]-target[8])*float(snapshot.width)*0.5f) ||
        !close(pass.camera.jitter_pixels[1],-(w.projection[9]-target[9])*float(snapshot.height)*0.5f)) return refuse(Stage::target_camera,snapshot,report);
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
        domain.viewport_xywh[2]=float(snapshot.width); domain.viewport_xywh[3]=float(snapshot.height);
        domain.viewport_min_depth=snapshot.intervals[i*2]; domain.viewport_max_depth=snapshot.intervals[i*2+1];
        if (!store(i?world_map:near_map,domain.source_clip_to_target_clip)) return refuse(Stage::target_math,snapshot,report);
    }
    if (!sample(cpu,confirmed,stage)) return refuse(stage,confirmed,report);
    if (!same(snapshot,confirmed)) return refuse(Stage::state_changed,confirmed,report);
    if (!identity(cpu,current_cpu_identity()) || !cpu_source_is_current(cpu)) return refuse(Stage::source,confirmed,report);
    output=result; return true;
}
}
