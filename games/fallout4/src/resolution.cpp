// SPDX-License-Identifier: GPL-3.0-only
#include "resolution.h"
#include "depth_domains.h"
#include "screen.h"
#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <intrin.h>
#include <d3d11.h>
#include <cstdio>

namespace rsf::fallout4 {
namespace {
constexpr uintptr_t state_rva=0x3d871a0, targets_rva=0x2f4d750;
std::atomic<uint32_t> owners{};
struct State {
    uint64_t native_frame{}, window{};
    uint32_t width{}, height{};
    float native_ratio[2]{}, input_ratio[2]{}, output_ratio[2]{}, jitter[2]{};
    unsigned char native_active{}, input_active{}, output_active{}, viewport{};
    uintptr_t context{}, device_context{}, chain{}, view{}, manager{}, triangles[3]{};
    float raster[6]{}, owned_raster[6]{};
    uint32_t dirty{};
    bool input{}, output{}, changed{}, ratio_owned[2]{}, active_owned{};
    bool output_ratios[2]{}, output_activation{}, viewport_owned{}, jitter_owned{}, raster_owned{}, dirty_owned{}, native_full_viewport{};
};
thread_local State state;
uintptr_t image() { return uintptr_t(GetModuleHandleW(nullptr)); }
bool copy(uintptr_t address, void* destination, size_t size) noexcept {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),destination,size,&copied) && copied==size;
}
template<class T> bool read(uintptr_t address,T& value) noexcept { return copy(address,&value,sizeof(value)); }
template<class T> bool write(uintptr_t address,const T& value) noexcept {
    SIZE_T written{};
    return WriteProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),&value,sizeof(value),&written) && written==sizeof(value);
}
template<class T> bool restore(uintptr_t address,const T& saved,const T& owned,bool& pending) noexcept {
    if (!pending) return true;
    T current{};
    if (!read(address,current)) return false;
    if (!std::memcmp(&current,&saved,sizeof(T))) { pending=false; return true; }
    if (std::memcmp(&current,&owned,sizeof(T))) {
        state.changed=true; pending=false; return true; // Another writer owns this value; preserve it.
    }
    if (!write(address,saved)) return false;
    pending=false; return true;
}
template<size_t N> bool bytes(uintptr_t rva,const unsigned char (&expected)[N]) noexcept {
    unsigned char actual[N]{};
    return copy(image()+rva,actual,N) && !std::memcmp(actual,expected,N);
}
bool native_context(uintptr_t& context) noexcept {
    uint32_t index{}; uintptr_t block{};
    if (!read(image()+0x3e73018,index) || index>4096) return false;
    const auto slots=static_cast<uintptr_t>(__readgsqword(0x58));
    if (!slots || !read(slots+size_t(index)*sizeof(uintptr_t),block) || !block || !read(block+0xb20,context)) return false;
    if (!context && !read(image()+0x38e1398,context)) return false;
    return context!=0;
}
bool current_owner(bool shadow=false) noexcept {
    uint32_t frame{},width{},height{}; uintptr_t renderer{},window{},context{},chain{},graph{},view{},engine_context{};
    return (!shadow || (native_context(engine_context) && engine_context==state.context)) &&
        read(image()+state_rva+0x98,frame) && frame==state.native_frame &&
        read(image()+state_rva+0x78,width) && width==state.width && read(image()+state_rva+0x7c,height) && height==state.height &&
        read(image()+0x38e13a0,renderer) && renderer==image()+0x3a25d10 &&
        read(renderer+0x58,window) && window==state.window &&
        read(renderer+0x50,context) && context==state.device_context && read(renderer+0x70,chain) && chain==state.chain &&
        read(image()+0x32dd338,graph) && graph && read(graph+0x140,view) && view==state.view;
}
bool triangles_current() noexcept {
    uintptr_t manager{},triangles[3]{};
    return read(image()+0x3e5e4b0,manager) && manager==state.manager &&
        read(manager+0x28,triangles[0]) && read(manager+0x38,triangles[1]) && read(manager+0x40,triangles[2]) &&
        !std::memcmp(triangles,state.triangles,sizeof(triangles));
}
}
void report_resolution_state(ResolutionStage stage,const rsf_game_render_pass& pass,void* context,
    bool published,int32_t result,void (*report)(const char*)) noexcept try {
    if (!report || !pass.source_frame_id) return;
    const auto saved_error=GetLastError();
    struct LastError { DWORD value; ~LastError() { SetLastError(value); } } last_error{saved_error};
    struct Receipt { uint64_t session{},source{}; uint32_t sources{},stages{}; bool ordinary{}; };
    thread_local Receipt receipt;
    const bool reduced=(state.input && (state.input_ratio[0]<1 || state.input_ratio[1]<1)) ||
        (pass.camera.render_width && pass.camera.output_width && pass.camera.render_width<pass.camera.output_width) ||
        (pass.camera.render_height && pass.camera.output_height && pass.camera.render_height<pass.camera.output_height) ||
        (receipt.session==pass.session_id && receipt.source==pass.source_frame_id);
    if (!reduced) return; // Native-quality startup cannot consume reduced-source receipts.
    constexpr const char* names[]{"pre-sr","output-ready","evaluated","effects-return","hudless","ui-return","ui-restored","source-retired"};
    const auto index=static_cast<unsigned>(stage);
    if (index>=sizeof(names)/sizeof(names[0])) return;
    if (receipt.session!=pass.session_id || receipt.source!=pass.source_frame_id) {
        receipt.session=pass.session_id; receipt.source=pass.source_frame_id; receipt.stages=0; receipt.ordinary=receipt.sources++<4;
    }
    if ((!receipt.ordinary && pass.native_frame%128) || (receipt.stages&(1u<<index))) return;
    static std::atomic<uint32_t> ordinary_rows{},sampled_rows{};
    auto& budget=receipt.ordinary ? ordinary_rows : sampled_rows;
    if (budget.fetch_add(1,std::memory_order_relaxed)>=32) return;
    receipt.stages|=1u<<index;
    const auto targets=image()+targets_rva;
    float ratio[2]{},raster[6]{}; unsigned char viewport{},active{},partial{};
    uint32_t frame{},dirty{}; uintptr_t manager{},geometry[3]{};
    const bool scalars=read(targets+0xfb8,ratio) && read(targets+0xfe4,viewport) && read(targets+0xfe5,active) &&
        read(image()+state_rva+0x98,frame) && read(image()+0x3e5e4b0,manager) && manager &&
        read(manager+0x48,partial) && read(manager+0x28,geometry[0]) && read(manager+0x38,geometry[1]) && read(manager+0x40,geometry[2]);
    const bool owner=current_owner(true) && context && uintptr_t(context)==state.device_context;
    const bool shadow=owner && read(state.context+0x1b70+0x90,raster) && read(state.context+0x1b70,dirty);
    UINT count{}; D3D11_VIEWPORT actual{}; bool rs{};
    // No resource operation or off-thread query. Unknown owners remain unknown.
    if (owner) {
        auto* device_context=static_cast<ID3D11DeviceContext*>(context);
        device_context->RSGetViewports(&count,nullptr);
        if (count==1) { UINT capacity=1; device_context->RSGetViewports(&capacity,&actual); rs=capacity==1; }
    }
    char text[1700]{};
    std::snprintf(text,sizeof(text),
        "Fallout4 reduced-stage=%s session=%llu source=%llu native=%llu/%u result=%d result_is_evaluation=%u published=%u scope=%u/%u changed=%u scalar_read=%u owner=%u input_ratio=%g,%g current_ratio=%g,%g viewport=%u active=%u partial=%u geometry=%p,%p,%p shadow_read=%u shadow=%g,%g,%g,%g,%g,%g dirty=0x%x rs_count=%u rs_read=%u rs=%g,%g,%g,%g,%g,%g camera=%ux%u->%ux%u context=%p",
        names[index],static_cast<unsigned long long>(pass.session_id),static_cast<unsigned long long>(pass.source_frame_id),static_cast<unsigned long long>(pass.native_frame),frame,
        result,unsigned(stage==ResolutionStage::evaluated),unsigned(published),unsigned(state.input),unsigned(state.output),unsigned(state.changed),unsigned(scalars),unsigned(owner),
        double(state.input_ratio[0]),double(state.input_ratio[1]),double(ratio[0]),double(ratio[1]),unsigned(viewport),unsigned(active),unsigned(partial),
        reinterpret_cast<void*>(geometry[0]),reinterpret_cast<void*>(geometry[1]),reinterpret_cast<void*>(geometry[2]),unsigned(shadow),
        double(raster[0]),double(raster[1]),double(raster[2]),double(raster[3]),double(raster[4]),double(raster[5]),dirty,count,unsigned(rs),
        double(actual.TopLeftX),double(actual.TopLeftY),double(actual.Width),double(actual.Height),double(actual.MinDepth),double(actual.MaxDepth),
        pass.camera.render_width,pass.camera.render_height,pass.camera.output_width,pass.camera.output_height,context);
    report(text);
} catch (...) {}
bool verify_resolution() noexcept {
    constexpr unsigned char taa[]{0x83,0xb9,0xac,0,0,0,0};
    constexpr unsigned char jitter[]{0xf3,0x0f,0x11,0x7f,0x04,0xf3,0x0f,0x11,0x5f,0x08};
    constexpr unsigned char selector[]{0x80,0x3d,0x38,0x90,0xdc,0,0,0xba,0x38,0,0,0,0x75,0x05,0xba,0x28,0,0,0};
    constexpr unsigned char partial[]{0x80,0x7d,0x48,0,0x74,0x07,0xba,0x40,0,0,0};
    constexpr unsigned char viewport[]{0x88,0x91,0xe4,0x0f,0,0,0xc3};
    constexpr unsigned char disable[]{0xf0,0xff,0x81,0xe8,0x0f,0,0};
    constexpr unsigned char activation[]{0x41,0xc6,0x82,0xe5,0x0f,0,0,1};
    constexpr unsigned char raster[]{0xf3,0x0f,0x11,0x83,0x98,0,0,0};
    constexpr unsigned char builder[]{0x48,0x83,0xec,0x38,0x8b,0x15,0x0e,0x92,0x63,0x02};
    constexpr unsigned char builder_entry[]{0x40,0x53,0x48,0x83,0xec,0x60};
    constexpr unsigned char explicit_extent[]{0x45,0x85,0xc0,0x74,0x10,0x41,0x8b,0xc0};
    return bytes(0x182d586,taa) && bytes(0x182d647,jitter) && bytes(0x21856f5,selector) &&
        bytes(0x21856df,partial) && bytes(0x1839ec0,viewport) && bytes(0x183a2d4,disable) &&
        bytes(0x183a278,activation) && bytes(0x181801e,raster) && bytes(0x1839e00,builder) &&
        bytes(0x1817ef0,builder_entry) && bytes(0x1817f1f,explicit_extent);
}
bool native_resolution_inputs(const rsf_game_render_pass& pass) noexcept {
    return pass.camera_valid && pass.render_rect[0]>=0 && pass.render_rect[1]>=0 &&
        pass.render_rect[2]>pass.render_rect[0] && pass.render_rect[3]>pass.render_rect[1] &&
        pass.render_rect[2]<=pass.output_rect[2] && pass.render_rect[3]<=pass.output_rect[3] &&
        uint32_t(pass.render_rect[2]-pass.render_rect[0])==pass.camera.render_width &&
        uint32_t(pass.render_rect[3]-pass.render_rect[1])==pass.camera.render_height &&
        pass.output_rect[0]==0 && pass.output_rect[1]==0 &&
        uint32_t(pass.output_rect[2])==pass.camera.output_width && uint32_t(pass.output_rect[3])==pass.camera.output_height;
}
bool configure_native_resolution(const rsf_game_host_services& host,const rsf_game_render_pass& frame,void* context) noexcept try {
    if (state.input || state.output || !context || !host.render_config || !host.process_upscale) return false;
    const auto cpu=current_cpu_identity();
    if (cpu.session!=frame.session_id || cpu.source!=frame.source_frame_id || cpu.submission!=frame.submission_id || !cpu_source_is_current(cpu)) return false;
    uint32_t width{},height{},disabled{},taa{},taa_disabled{}; unsigned char viewport{};
    const auto targets=image()+targets_rva;
    if (!read(image()+state_rva+0x78,width) || !read(image()+state_rva+0x7c,height) || !width || !height || width>16384 || height>16384 ||
        !read(targets+0xfe8,disabled) || disabled || !read(targets+0xfe4,viewport) || viewport!=1 ||
        !read(image()+state_rva+0xac,taa) || taa!=1 || !read(image()+state_rva+0xb0,taa_disabled) || taa_disabled) return false;
    auto planning=frame; planning.role=RSF_GAME_RENDER_SR;
    planning.flags=RSF_GAME_RENDER_RECONFIGURE|RSF_GAME_RENDER_NO_FG;
    planning.camera.output_width=width; planning.camera.output_height=height;
    (void)host.process_upscale(host.user,context,&planning);
    rsf_game_render_config config{sizeof(config)};
    if (!host.render_config(host.user,&config) || !config.enabled || config.output_width!=width || config.output_height!=height ||
        !config.render_width || !config.render_height || config.render_width>width || config.render_height>height) return false;
    const rsf_game_render_pass candidate{};
    if (!cpu_source_is_current(cpu) || classify_native_screen(cpu,candidate,true).refusals!=ScreenWorldLeaseInvalid) return false;
    if (!read(targets+0xfe8,disabled) || disabled || !read(targets+0xfe4,viewport) || viewport!=1 ||
        !read(image()+state_rva+0xac,taa) || taa!=1 || !read(image()+state_rva+0xb0,taa_disabled) || taa_disabled) return false;
    state={}; state.native_frame=frame.native_frame; state.window=frame.window_key; state.width=width; state.height=height;
    state.device_context=uintptr_t(context); state.chain=uintptr_t(frame.swapchain); state.view=frame.view_key;
    if (!native_context(state.context) || !current_owner() || !read(targets+0xfb8,state.native_ratio) || !read(targets+0xfe5,state.native_active)) return false;
    for (float value:state.native_ratio) if (!std::isfinite(value) || value<=0 || value>1) return false;
    state.input_ratio[0]=float(config.render_width)/float(width); state.input_ratio[1]=float(config.render_height)/float(height);
    state.input_active=static_cast<unsigned char>(config.render_width!=width || config.render_height!=height);
    state.input=true; owners.fetch_add(1,std::memory_order_acq_rel);
    for (unsigned i=0;i<2;++i) {
        state.ratio_owned[i]=true;
        if (!write(targets+0xfb8+4*i,state.input_ratio[i])) { if (!finish_resolution_source()) state.changed=true; return false; }
    }
    state.active_owned=true;
    if (!write(targets+0xfe5,state.input_active)) { if (!finish_resolution_source()) state.changed=true; return false; }
    return true;
} catch (...) { return false; }
bool native_resolution_write() noexcept {
    if (!state.input && !state.output) return true;
    if (!current_owner(state.output)) return false;
    const auto targets=image()+targets_rva;
    float ratio[2]{}; unsigned char active{};
    if (!read(targets+0xfb8,ratio) || !read(targets+0xfe5,active)) return false;
    for (float value:ratio) if (!std::isfinite(value) || value<=0 || value>1) return false;
    // This observed native producer supersedes our input values, even if equal.
    // Retain the source obligation until Swap; no failed cleanup is discarded.
    state.ratio_owned[0]=state.ratio_owned[1]=state.active_owned=false;
    if (!state.output) return true;
    std::memcpy(state.output_ratio,ratio,sizeof(ratio)); state.output_active=active;
    const float one=1; const unsigned char off=0;
    for (unsigned i=0;i<2;++i) { state.output_ratios[i]=true; if (!write(targets+0xfb8+4*i,one)) return false; }
    state.output_activation=true;
    return write(targets+0xfe5,off);
}
bool begin_output_resolution(const NativeDepthSnapshot& snapshot,const rsf_game_render_pass& pass) noexcept {
    if (state.output || !native_depth_snapshot_current(snapshot)) return false;
    if (!state.input) {
        state={}; state.native_frame=pass.native_frame; state.window=pass.window_key;
        state.width=pass.camera.output_width; state.height=pass.camera.output_height;
        state.device_context=static_cast<uintptr_t>(snapshot.device_context_key);
        state.chain=static_cast<uintptr_t>(snapshot.chain_key); state.view=pass.view_key;
    }
    const auto targets=image()+targets_rva;
    uint32_t disabled{};
    // Input ownership covers globals only. Adopt this freshly verified producer's
    // native Context before borrowing its shadow viewport for output.
    state.context=static_cast<uintptr_t>(snapshot.context_key);
    if (!current_owner(true) || !read(targets+0xfe8,disabled) || disabled ||
        !read(targets+0xfb8,state.output_ratio) || !read(targets+0xfe5,state.output_active) || !read(targets+0xfe4,state.viewport) ||
        !read(image()+state_rva+4,state.jitter) || !read(image()+0x3e5e4b0,state.manager) || !state.manager ||
        !read(state.manager+0x28,state.triangles[0]) || !read(state.manager+0x38,state.triangles[1]) || !read(state.manager+0x40,state.triangles[2]) ||
        !state.triangles[0] || !state.triangles[1] || !state.triangles[2]) return false;
    const auto shadow=state.context+0x1b70;
    if (!read(shadow+0x90,state.raster) || !read(shadow,state.dirty)) return false;
    for (float value:state.raster) if (!std::isfinite(value)) return false;
    for (float value:state.jitter) if (!std::isfinite(value)) return false;
    state.output=true; state.native_full_viewport=false; owners.fetch_add(1,std::memory_order_acq_rel);
    const float one=1,zero[2]{}; const unsigned char off=0;
    for (unsigned i=0;i<2;++i) { state.output_ratios[i]=true; if (!write(targets+0xfb8+4*i,one)) { end_output_resolution(); return false; } }
    state.output_activation=true; state.viewport_owned=true; state.jitter_owned=true;
    if (!write(targets+0xfe5,off) || !write(targets+0xfe4,off) || !write(image()+state_rva+4,zero)) { end_output_resolution(); return false; }
    state.raster_owned=state.dirty_owned=true;
    // Native camera setter uses this same explicit-size ABI. The previous
    // bound auxiliary target must not decide the new full-output extent.
    using Viewport=void (*)(void*,void*,uint32_t,uint32_t,bool);
    reinterpret_cast<Viewport>(image()+0x1817ef0)(reinterpret_cast<void*>(image()+0x3a25d00),
        reinterpret_cast<void*>(shadow),state.width,state.height,true);
    if (!read(shadow+0x90,state.owned_raster) || !triangles_current() ||
        state.owned_raster[0]!=0 || state.owned_raster[1]!=0 || state.owned_raster[2]!=float(state.width) || state.owned_raster[3]!=float(state.height)) {
        end_output_resolution(); return false;
    }
    return true;
}
bool native_viewport_write(bool enabled) noexcept {
    if (!state.output) return true;
    // A genuine native write transfers FE4 intent even when it writes our same value.
    state.viewport=static_cast<unsigned char>(enabled);
    if (!enabled) state.native_full_viewport=true;
    const unsigned char off=0;
    return !enabled || write(image()+targets_rva+0xfe4,off);
}
bool end_output_resolution() noexcept {
    if (!state.output) return !state.changed;
    if (!current_owner(true)) return false;
    const auto targets=image()+targets_rva,shadow=state.context+0x1b70;
    const float one=1,zero[2]{}; const unsigned char off=0;
    if (!triangles_current()) state.changed=true; // Borrowed geometry is never an ownership obligation.
    bool okay=true;
    for (unsigned i=0;i<2;++i) okay=restore(targets+0xfb8+4*i,state.output_ratio[i],one,state.output_ratios[i]) && okay;
    okay=restore(targets+0xfe5,state.output_active,off,state.output_activation) && okay;
    okay=restore(targets+0xfe4,state.viewport,off,state.viewport_owned) && okay;
    okay=restore(image()+state_rva+4,state.jitter,zero,state.jitter_owned) && okay;
    if (state.native_full_viewport) state.raster_owned=state.dirty_owned=false;
    okay=restore(shadow+0x90,state.raster,state.owned_raster,state.raster_owned) && okay;
    if (state.dirty_owned) {
        uint32_t dirty{};
        if (!read(shadow,dirty) || !write(shadow,(dirty&~2u)|(state.dirty&2u))) okay=false;
        else state.dirty_owned=false;
    }
    if (okay) { state.output=false; owners.fetch_sub(1,std::memory_order_acq_rel); }
    return okay && !state.changed;
}
bool finish_resolution_source() noexcept {
    const bool valid=end_output_resolution();
    if (state.output) return false;
    if (!state.input) { state={}; return valid; }
    if (!current_owner()) return false;
    const auto targets=image()+targets_rva;
    bool okay=true;
    for (unsigned i=0;i<2;++i) okay=restore(targets+0xfb8+4*i,state.native_ratio[i],state.input_ratio[i],state.ratio_owned[i]) && okay;
    okay=restore(targets+0xfe5,state.native_active,state.input_active,state.active_owned) && okay;
    const bool accepted=okay && valid && !state.changed;
    if (okay) { state.input=false; owners.fetch_sub(1,std::memory_order_acq_rel); state={}; }
    return accepted;
}
bool resolution_busy() noexcept { return owners.load(std::memory_order_acquire)!=0; }
bool output_resolution_busy() noexcept { return state.output; }
bool resolution_refused() noexcept { return state.changed; }
}
