// SPDX-License-Identifier: GPL-3.0-only
#include "render_hooks.h"
#include "cpu_events.h"
#include "render_inputs.h"
#include "resolution.h"
#include "screen.h"
#include "native_creation.h"
#include "depth_domains.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::fallout4 {
namespace {
using Viewport = void (*)(void*, bool);
using Imagespace = void (*)();
using WorldBegin = void (*)();
using Effects = void (*)(void*, uint32_t, uint32_t, uint32_t, uint32_t);
using TaaActive = bool (*)(void*);
using NativePresent = void (*)(void*);
using Prepass = void (*)();
using DynamicResolution = void (*)(void*, void*, void*, void*, void*);
Viewport viewport{};
Imagespace imagespace{};
WorldBegin world_begin{};
Effects effects{};
TaaActive taa_active{};
NativePresent native_present{};
Prepass prepass{};
DynamicResolution dynamic_resolution{};
decltype(&rsf_d3d11_host_upscale_auxiliaries) upscale_auxiliaries{};
decltype(&rsf_d3d11_host_restore_auxiliaries) restore_auxiliaries{};
rsf_game_host_services host{};
std::atomic<bool> live{};
std::atomic<uint32_t> entered{}, windows{}, frames{}, auxiliaries{};
struct World {
    CpuIdentity cpu{};
    rsf_game_render_pass pass{}, frame_pass{}, submission_pass{}, screen_basis{};
    NativeScreen screen{};
    NativeDepthSnapshot depth_snapshot{};
    rsf_d3d11_auxiliary_scope auxiliary_scope{};
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context;
    Microsoft::WRL::ComPtr<IDXGISwapChain> chain;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> final_colour;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> colour, depth, motion;
    bool in_imagespace{}, inputs{}, published{}, window_open{}, frame_open{}, submission_open{}, refused{}, depth_interrupted{}, auxiliary_open{}, raster_refused{};
};
thread_local World world;
thread_local bool ordinary_world{};
thread_local uint32_t prepass_depth{};
uintptr_t image() { return uintptr_t(GetModuleHandleW(nullptr)); }
bool read(uintptr_t address, void* destination, size_t length) {
    SIZE_T copied{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), destination, length, &copied) && copied == length;
}
void report(const char* message) noexcept {
    if (host.log) try { host.log(host.user, message); } catch (...) {}
}
void diagnostic(const char* message) noexcept {
    static std::atomic<uint32_t> count{};
    if (count.fetch_add(1, std::memory_order_relaxed) < 16) report(message);
}
struct Entry {
    Entry() { entered.fetch_add(1, std::memory_order_acq_rel); }
    ~Entry() { entered.fetch_sub(1, std::memory_order_acq_rel); }
};
bool same_source() {
    return world.cpu.session == host.session_id && cpu_source_is_current(world.cpu);
}
void notify(const rsf_game_render_pass& pass, void* context, uint32_t begin) noexcept {
    if (host.render_pass) try { host.render_pass(host.user, context, &pass, begin); } catch (...) {}
}
void identify(rsf_game_render_pass& pass) {
    pass.session_id = world.cpu.session;
    pass.source_frame_id = world.cpu.source;
    pass.submission_id = pass.scope_id = world.cpu.submission;
    pass.flags |= RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
    pass.screen = RSF_SCREEN_UNKNOWN;
    // The admitted ordinary WorldBegin follows the native update/redraw work.
    // This constructor fact does not report a SIM_END or classify gameplay.
}
struct Owner {
    uintptr_t renderer{}, device{}, context{}, chain{}, window{}, graph{}, view{};
    uint32_t frame{};
    bool operator==(const Owner&) const = default;
};
bool owner(Owner& value) {
    const auto base=image();
    const auto get=[](uintptr_t address, auto& output) { return read(address,&output,sizeof(output)); };
    return get(base+0x38e13a0,value.renderer) && value.renderer==base+0x3a25d10 &&
        get(value.renderer+0x48,value.device) && value.device && get(value.renderer+0x50,value.context) && value.context &&
        get(value.renderer+0x58,value.window) && value.window && get(value.renderer+0x70,value.chain) && value.chain &&
        get(base+0x32dd338,value.graph) && value.graph && get(value.graph+0x140,value.view) && value.view &&
        get(base+0x3d871a0+0x98,value.frame) && accept_window(nullptr,reinterpret_cast<void*>(value.window));
}
bool same_object(IUnknown* a, IUnknown* b) {
    Microsoft::WRL::ComPtr<IUnknown> left,right;
    return a && b && SUCCEEDED(a->QueryInterface(IID_PPV_ARGS(&left))) &&
        SUCCEEDED(b->QueryInterface(IID_PPV_ARGS(&right))) && left.Get()==right.Get();
}
bool begin_render_scope() {
    Owner before{},after{};
    if (!owner(before)) return false;
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context=reinterpret_cast<ID3D11DeviceContext*>(before.context);
    Microsoft::WRL::ComPtr<IDXGISwapChain> chain=reinterpret_cast<IDXGISwapChain*>(before.chain);
    Microsoft::WRL::ComPtr<ID3D11Device> device,chain_device;
    context->GetDevice(&device);
    DXGI_SWAP_CHAIN_DESC description{};
    if (context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE ||
        !same_object(device.Get(),reinterpret_cast<ID3D11Device*>(before.device)) ||
        FAILED(chain->GetDevice(IID_PPV_ARGS(&chain_device))) || !same_object(device.Get(),chain_device.Get()) ||
        FAILED(chain->GetDesc(&description)) || description.OutputWindow!=reinterpret_cast<HWND>(before.window) ||
        !owner(after) || before!=after || !same_source()) return false;
    world.context=context; world.chain=chain;
    auto& frame=world.frame_pass;
    frame.struct_size=sizeof(frame); identify(frame);
    frame.role=RSF_GAME_RENDER_FRAME; frame.native_frame=before.frame;
    frame.family_key=frame.view_key=frame.history_key=before.view;
    frame.viewport_key=frame.window_key=frame.rhi_viewport_key=before.window;
    frame.pass_key=image()+0x21f0ac0; frame.swapchain=chain.Get();
    world.submission_pass=frame; world.submission_pass.role=RSF_GAME_RENDER_SUBMISSION;
    world.frame_open=true; frames.fetch_add(1,std::memory_order_acq_rel);
    notify(frame,context.Get(),1);
    world.submission_open=true;
    notify(world.submission_pass,context.Get(),1);
    static std::atomic_flag announced = ATOMIC_FLAG_INIT;
    if (!announced.test_and_set(std::memory_order_relaxed))
        report("Fallout4 native FRAME/SUBMISSION begin before ordinary WorldBegin; screen/input eligibility not inferred.");
    return true;
}
void screen_facts(rsf_game_render_pass& pass, bool first) {
    const auto facts=classify_native_screen(world.cpu,world.screen_basis,true);
    if (first) {
        world.screen=facts;
        char message[180]{};
        std::snprintf(message,sizeof(message),"Fallout4 screen facts source=%llu native=%llu screen=%u facts=0x%x refusals=0x%x camera=%u",
            static_cast<unsigned long long>(world.cpu.source),static_cast<unsigned long long>(pass.native_frame),
            facts.screen,facts.facts,facts.refusals,facts.camera_index);
        static std::atomic<uint32_t> receipts{};
        if (receipts.fetch_add(1, std::memory_order_relaxed) < 8) report(message);
    }
    if (facts.refusals || facts.screen!=RSF_SCREEN_GAMEPLAY || !same_source() ||
        facts.screen!=world.screen.screen || facts.facts!=world.screen.facts || facts.camera_state!=world.screen.camera_state ||
        !world.frame_open || !world.submission_open) world.refused=true;
    pass.screen=facts.screen;
    if (world.refused) pass.flags|=RSF_GAME_RENDER_NO_FG;
    else pass.flags&=~RSF_GAME_RENDER_NO_FG;
}
void close_submission() {
    if (!world.submission_open) return;
    if (world.refused) world.submission_pass.flags|=RSF_GAME_RENDER_NO_FG;
    notify(world.submission_pass,world.context.Get(),0);
    world.submission_open=false;
}
bool drain_auxiliaries() noexcept {
    if (!world.auxiliary_open) return true;
    int32_t result=RSF_ERROR_NOT_READY;
    try { result=restore_auxiliaries(host.user,&world.auxiliary_scope); } catch (...) {}
    if (result!=RSF_OK) { world.refused=true; return false; }
    world.auxiliary_open=false; auxiliaries.fetch_sub(1,std::memory_order_acq_rel);
    return true;
}
bool auxiliary_colour(ID3D11Texture2D* texture, const NativeInputs& inputs) {
    if (!texture) return false;
    Microsoft::WRL::ComPtr<ID3D11Device> device,owner;
    inputs.context->GetDevice(&device); texture->GetDevice(&owner);
    D3D11_TEXTURE2D_DESC d{}; texture->GetDesc(&d);
    if (!same_object(device.Get(),owner.Get()) || d.Width!=inputs.pass.camera.output_width || d.Height!=inputs.pass.camera.output_height ||
        d.MipLevels!=1 || d.ArraySize!=1 || d.SampleDesc.Count!=1 || d.SampleDesc.Quality || d.Usage!=D3D11_USAGE_DEFAULT) return false;
    UINT support{};
    constexpr UINT required=D3D11_FORMAT_SUPPORT_TEXTURE2D|D3D11_FORMAT_SUPPORT_SHADER_LOAD|D3D11_FORMAT_SUPPORT_RENDER_TARGET;
    if (FAILED(device->CheckFormatSupport(d.Format,&support)) || (support&required)!=required) return false;
    return d.Format==DXGI_FORMAT_R8_UNORM || d.Format==DXGI_FORMAT_R8G8B8A8_UNORM || d.Format==DXGI_FORMAT_R16G16_FLOAT ||
        d.Format==DXGI_FORMAT_R16_FLOAT || d.Format==DXGI_FORMAT_R32_FLOAT || d.Format==DXGI_FORMAT_R16G16B16A16_FLOAT || d.Format==DXGI_FORMAT_R11G11B10_FLOAT;
}
int32_t reconstruct(NativeInputs& inputs, const rsf_game_render_pass& pass) {
    const bool reduced=pass.camera.render_width!=pass.camera.output_width || pass.camera.render_height!=pass.camera.output_height ||
        pass.render_rect[0]!=0 || pass.render_rect[1]!=0;
    if (!reduced) return host.process_upscale(host.user,inputs.context.Get(),&pass);
    D3D11_TEXTURE2D_DESC depth{}; inputs.depth->GetDesc(&depth);
    if (depth.Format!=DXGI_FORMAT_R24G8_TYPELESS || !auxiliary_colour(inputs.motion.Get(),inputs)) return RSF_ERROR_NATIVE_REFUSED;
    std::array<rsf_d3d11_auxiliary_surface,5> surfaces{};
    uint32_t count{};
    const auto add=[&](ID3D11Texture2D* texture,uint32_t kind,uint32_t flags,const char* name,const char* source) {
        auto& surface=surfaces[count++]; surface.struct_size=sizeof(surface); surface.kind=kind; surface.texture=texture;
        for (unsigned i=0;i<4;++i) { surface.source_rect[i]=float(pass.render_rect[i]); surface.output_rect[i]=pass.output_rect[i]; }
        if (kind==RSF_D3D11_AUXILIARY_COLOUR) {
            surface.source_rect[0]=world.depth_snapshot.actual_raster[0]; surface.source_rect[1]=world.depth_snapshot.actual_raster[1];
            surface.source_rect[2]=surface.source_rect[0]+world.depth_snapshot.actual_raster[2];
            surface.source_rect[3]=surface.source_rect[1]+world.depth_snapshot.actual_raster[3];
        }
        surface.jitter_pixels[0]=pass.camera.jitter_pixels[0]; surface.jitter_pixels[1]=pass.camera.jitter_pixels[1];
        surface.flags=flags; surface.name=name; surface.source_name=source;
    };
    add(inputs.depth.Get(),RSF_D3D11_AUXILIARY_DEPTH_STENCIL,0,"native_depth_expanded","native_depth_partitioned");
    add(inputs.motion.Get(),RSF_D3D11_AUXILIARY_COLOUR,0,"native_motion_expanded","native_motion_reduced");
    const auto optional=[&](ID3D11Texture2D* texture,uint32_t flags,const char* name,const char* source) {
        if (!auxiliary_colour(texture,inputs) || same_object(texture,static_cast<ID3D11Texture2D*>(pass.color_output))) return;
        for (uint32_t i=0;i<count;++i) if (same_object(texture,static_cast<ID3D11Texture2D*>(surfaces[i].texture))) return;
        add(texture,RSF_D3D11_AUXILIARY_COLOUR,flags,name,source);
    };
    optional(inputs.pipboy.Get(),0,"native_pipboy_expanded","native_pipboy_reduced");
    optional(inputs.vats.Get(),RSF_D3D11_AUXILIARY_RESTORE_CONTENTS,"native_vats_expanded","native_vats_reduced");
    optional(inputs.refraction_normals.Get(),0,"native_refraction_expanded","native_refraction_reduced");
    const rsf_d3d11_auxiliary_pass request{sizeof(request),RSF_D3D11_HOST_ABI_VERSION,&pass,count,surfaces.data()};
    world.auxiliary_scope={sizeof(world.auxiliary_scope),RSF_D3D11_HOST_ABI_VERSION,pass.session_id,pass.native_frame,pass.view_key};
    world.auxiliary_open=true; auxiliaries.fetch_add(1,std::memory_order_acq_rel);
    const auto result=upscale_auxiliaries(host.user,inputs.context.Get(),&request);
    if (result!=RSF_OK) (void)drain_auxiliaries();
    return result;
}
bool emit_sr() {
    if (!world.in_imagespace || !same_source() || world.inputs || world.raster_refused || world.auxiliary_open || !host.process_upscale) return false;
    NativeInputs inputs;
    if (!read_native_inputs(inputs, report, &world.depth_snapshot)) return false;
    auto pass = inputs.pass;
    identify(pass);
    pass.role = RSF_GAME_RENDER_SR;
    pass.pass_key = image() + 0x21fa283;
    if (!native_resolution_inputs(pass)) {
        diagnostic("Fallout4 SR refused: actual producer LTRB crop/camera/output extents disagree.");
        return false;
    }
    if (!world.frame_open || pass.native_frame!=world.frame_pass.native_frame ||
        pass.view_key!=world.frame_pass.view_key || pass.window_key!=world.frame_pass.window_key ||
        inputs.context.Get()!=world.context.Get() || inputs.chain.Get()!=world.chain.Get()) {
        world.refused=true;
        diagnostic("Fallout4 SR refused: native lease differs from the pre-world FRAME/SUBMISSION owner.");
        return false;
    }
    if (!world.depth_snapshot.valid ||
        !make_native_depth_domains(world.depth_snapshot, world.cpu, pass, pass.depth_domains, report)) {
        world.refused = true;
        diagnostic("Fallout4 temporal publication refused: same-source native prepass depth mapping unavailable; raw mixed depth not substituted.");
        return false;
    }
    auto planning = pass;
    planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
    try { (void)host.process_upscale(host.user, inputs.context.Get(), &planning); } catch (...) { return false; }
    rsf_game_render_config requested{sizeof(requested)};
    const bool sr_off=host.render_config && host.render_config(host.user,&requested) && !requested.enabled &&
        requested.output_width==pass.camera.output_width && requested.output_height==pass.camera.output_height;
    world.colour=inputs.colour; world.depth=inputs.depth; world.motion=inputs.motion;
    world.screen_basis=pass;
    auto fg = pass;
    screen_facts(fg,true);
    if (world.screen.refusals || world.screen.screen!=RSF_SCREEN_GAMEPLAY || !same_source()) return false;
    fg.role = RSF_GAME_RENDER_FG_INPUTS;
    notify(fg, inputs.context.Get(), 1);
    notify(fg, inputs.context.Get(), 0);
    if (!same_source()) world.refused = true;
    // The host normalizes and retains these contents synchronously before SR.
    // No borrowed resource pointer survives in the scalar identity below.
    world.pass = fg;
    world.pass.color_input = world.pass.color_output = world.pass.color_output_readable = nullptr;
    world.pass.depth = world.pass.motion = world.pass.exposure = nullptr;
    world.pass.swapchain = world.pass.ui_input = world.pass.scene_surface = world.pass.sampled_texture = nullptr;
    world.pass.native_frame_context = world.pass.temporal_mask = nullptr;
    world.context = inputs.context;
    world.chain = inputs.chain;
    world.inputs = true;
    if (sr_off) return false; // Keep actual FG/native output facts, with no SR/output-state mutation.
    if (!begin_output_resolution(world.depth_snapshot,pass)) { world.refused=true; return false; }
    int32_t result = RSF_ERROR_NOT_READY;
    try { result = reconstruct(inputs,pass); } catch (...) { (void)drain_auxiliaries(); }
    if (result != RSF_OK) { if (!end_output_resolution()) world.refused=true; return false; }
    world.published = true;
    diagnostic("Fallout4 producer-verified SR publication accepted before imagespace; scoped native output/auxiliary consumers active, FG remains unproven.");
    return true;
}
void emit_hudless() {
    if (!world.in_imagespace || !world.inputs || world.window_open || !same_source()) return;
    NativeFramebuffer frame;
    const auto refuse=[&](unsigned stage, const char* name) {
        static std::atomic<uint32_t> reported{};
        if (reported.fetch_or(uint32_t(1)<<stage,std::memory_order_relaxed)&(uint32_t(1)<<stage)) return;
        char message[700]{};
        std::snprintf(message,sizeof(message),
            "Fallout4 HUDless stage=%s source=%llu native=%llu/%llu frame_basis=%llu screen_basis=%llu window=%p/%p context=%p/%p outer_chain=%p/%p",
            name,static_cast<unsigned long long>(world.cpu.source),
            static_cast<unsigned long long>(frame.native_frame),static_cast<unsigned long long>(world.pass.native_frame),
            static_cast<unsigned long long>(world.frame_pass.native_frame),static_cast<unsigned long long>(world.screen_basis.native_frame),
            reinterpret_cast<void*>(frame.window_key),reinterpret_cast<void*>(world.pass.window_key),
            static_cast<void*>(frame.context.Get()),static_cast<void*>(world.context.Get()),
            static_cast<void*>(frame.chain.Get()),static_cast<void*>(world.chain.Get()));
        report(message);
    };
    // A failed logical-0 read is not evidence that any identity comparison failed.
    if (!read_native_framebuffer(frame,report)) { refuse(0,"framebuffer-read"); return; }
    if (frame.native_frame!=world.pass.native_frame) { refuse(1,"native-frame"); return; }
    if (frame.window_key!=world.pass.window_key) { refuse(2,"window"); return; }
    if (frame.context.Get()!=world.context.Get()) { refuse(3,"context"); return; }
    if (frame.chain.Get()!=world.chain.Get()) { refuse(4,"outer-chain"); return; }
    D3D11_TEXTURE2D_DESC description{};
    frame.texture->GetDesc(&description);
    if (description.Width != world.pass.camera.output_width || description.Height != world.pass.camera.output_height || description.SampleDesc.Count != 1) {
        refuse(5,"output-extent"); return;
    }
    auto final = world.pass;
    screen_facts(final,false);
    final.role = RSF_GAME_RENDER_FINAL_SCENE;
    final.scene_surface = frame.texture.Get();
    final.swapchain = world.chain.Get();
    notify(final, world.context.Get(), 1);
    notify(final, world.context.Get(), 0);
    world.final_colour = frame.texture;
    world.pass=final;
    world.pass.role = RSF_GAME_RENDER_WINDOW;
    world.pass.swapchain = world.chain.Get();
    world.pass.scene_surface = nullptr;
    world.window_open = true;
    windows.fetch_add(1, std::memory_order_acq_rel);
    notify(world.pass, world.context.Get(), 1);
}
void observe_viewport(void* manager, bool enabled) {
    Entry entry;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    viewport(manager, enabled);
    const auto error = GetLastError();
    if (manager==reinterpret_cast<void*>(image()+0x2f4d750) && !native_viewport_write(enabled)) {
        world.refused=true;
        diagnostic("Fallout4 owned full-output selector write failed after native viewport intent; scope retained.");
    }
    if (live.load(std::memory_order_acquire) && manager == reinterpret_cast<void*>(image() + 0x2f4d750)) {
        if (caller == image() + 0x21fa2ca) {
            try { emit_hudless(); } catch (...) { diagnostic("Fallout4 HUDless callback failed; native drawing retained."); }
        }
    }
    SetLastError(error);
}
void observe_world_begin() {
    Entry entry;
    const auto error = GetLastError();
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto cpu = current_cpu_identity();
    const bool selected = live.load(std::memory_order_acquire) && caller == image() + 0xc382a5 &&
        cpu.session == host.session_id && cpu.source && !cpu.submission && !ordinary_world;
    struct Restore {
        bool previous;
        ~Restore() { ordinary_world = previous; }
    } restore{ordinary_world};
    if (selected && !world.frame_open && !world.auxiliary_open && !resolution_busy() && begin_cpu_render()) {
        ordinary_world=true;
        world={}; world.cpu=current_cpu_identity();
        try {
            if (!begin_render_scope()) {
                world.refused=true;
                diagnostic("Fallout4 pre-world FRAME/SUBMISSION owner/view/frame lease refused.");
            }
        } catch (...) { world.refused=true; }
    }
    SetLastError(error);
    world_begin();
}
void observe_dynamic_resolution(void* manager,void* a,void* b,void* c,void* d) {
    Entry entry;
    const auto error=GetLastError();
#if defined(_MSC_VER)
    const auto caller=uintptr_t(_ReturnAddress());
#else
    const auto caller=uintptr_t(__builtin_return_address(0));
#endif
    if (world.frame_open) world.depth_snapshot={};
    SetLastError(error);
    dynamic_resolution(manager,a,b,c,d);
    const auto native_error=GetLastError();
    const bool overlapping=manager==reinterpret_cast<void*>(image()+0x2f4d750) && resolution_busy();
    if (overlapping) {
        world.raster_refused=world.refused=true;
        if (!native_resolution_write()) diagnostic("Fallout4 native DR ownership transfer refused; pending scope retained.");
    }
    if (live.load(std::memory_order_acquire) && caller==image()+0x21f9f34 &&
        manager==reinterpret_cast<void*>(image()+0x2f4d750) && !overlapping && world.frame_open && world.submission_open && !world.inputs && same_source()) {
        Owner owner_now{};
        const rsf_game_render_pass candidate{};
        const auto screen=classify_native_screen(world.cpu,candidate,true);
        if (screen.refusals==ScreenWorldLeaseInvalid && owner(owner_now) &&
            owner_now.frame==world.frame_pass.native_frame && owner_now.view==world.frame_pass.view_key &&
            owner_now.window==world.frame_pass.window_key && owner_now.context==uintptr_t(world.context.Get()) && owner_now.chain==uintptr_t(world.chain.Get())) {
            if (!configure_native_resolution(host,world.frame_pass,world.context.Get()) && (resolution_busy() || resolution_refused()))
                world.raster_refused=world.refused=true;
        }
    } else if (world.frame_open && resolution_busy()) world.raster_refused=world.refused=true;
    SetLastError(native_error);
}
void observe_prepass() {
    Entry entry;
    const auto error = GetLastError();
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    ++prepass_depth;
    struct Exit { ~Exit() { --prepass_depth; } } exit;
    const auto identity = world.cpu;
    // FRAME admission records the ordinary parent beyond WorldBegin's return.
    const bool selected = live.load(std::memory_order_acquire) && prepass_depth == 1 &&
        caller == image() + 0x21f9f78 && world.frame_open &&
        world.submission_open && !world.inputs && same_source();
    // Every producer call invalidates the previous copy, including foreign views.
    if (world.frame_open) {
        world.depth_snapshot = {};
        if (prepass_depth != 1) world.depth_interrupted = true;
        if (world.inputs) world.refused = true;
    }
    if (selected) world.depth_interrupted = false;
    SetLastError(error);
    prepass();
    const auto native_error = GetLastError();
    if (selected && live.load(std::memory_order_acquire) &&
        world.frame_open && world.submission_open && !world.inputs &&
        !world.depth_interrupted && cpu_source_is_current(identity) && same_source()) {
        NativeDepthSnapshot snapshot{};
        if (capture_native_depth_domains(identity, snapshot, report) &&
            cpu_source_is_current(identity) && same_source()) {
            world.depth_snapshot = snapshot;
        } else {
            world.refused = true;
            diagnostic("Fallout4 native prepass returned without a valid same-source depth-domain snapshot.");
        }
    }
    SetLastError(native_error);
}
void observe_imagespace() {
    Entry entry;
    const auto error = GetLastError();
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const bool selected = live.load(std::memory_order_acquire) && !world.in_imagespace && same_source() &&
        (caller == image() + 0xc383c7 || caller == image() + 0xc39d6f || caller == image() + 0xc39e13);
    struct Cleanup {
        bool selected;
        ~Cleanup() {
            if (!selected) return;
            const auto error = GetLastError();
            if (!drain_auxiliaries()) diagnostic("Fallout4 auxiliary retirement remains pending after native imagespace; owner retained.");
            if (!end_output_resolution()) world.refused=true;
            if (output_resolution_busy()) { world.refused=true; diagnostic("Fallout4 native output-state cleanup remains owned; subsequent world admission blocked."); }
            world.in_imagespace = world.published = false;
            SetLastError(error);
        }
    } cleanup{selected};
    if (selected) world.in_imagespace = true;
    SetLastError(error);
    imagespace();
}
void observe_effects(void* manager, uint32_t first, uint32_t last, uint32_t mode, uint32_t flags) {
    Entry entry;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto error = GetLastError();
    uintptr_t native_manager{};
    if (live.load(std::memory_order_acquire) && caller == image() + 0x21fa288 && first == 0 && last == 13 && mode == 1 && flags == 1 &&
        read(image() + 0x3e5e4b0, &native_manager, sizeof(native_manager)) && uintptr_t(manager) == native_manager) {
        try { (void)emit_sr(); } catch (...) { diagnostic("Fallout4 SR callback failed; native imagespace retained."); }
    }
    SetLastError(error);
    effects(manager, first, last, mode, flags);
    const auto native_error=GetLastError();
    if (caller==image()+0x21fa288 && first==0 && last==13 && mode==1 && flags==1 && world.auxiliary_open)
        if (!drain_auxiliaries()) diagnostic("Fallout4 VATS/auxiliary restoration refused at native Effects return; owner retained.");
    SetLastError(native_error);
}
bool observe_taa(void* effect) {
    Entry entry;
    const auto result = taa_active(effect);
    const auto error = GetLastError();
    const bool excluded = live.load(std::memory_order_acquire) && world.in_imagespace && world.published && same_source();
    SetLastError(error);
    return result && !excluded;
}
void observe_native_present(void* renderer) {
    Entry entry;
    const auto error=GetLastError();
#if defined(_MSC_VER)
    const auto caller=uintptr_t(_ReturnAddress());
#else
    const auto caller=uintptr_t(__builtin_return_address(0));
#endif
    bool cpu_phase=false;
    if (caller==image()+0xc335ec && renderer==reinterpret_cast<void*>(image()+0x3a25d00) && world.submission_open) {
        Owner current{};
        uintptr_t current_window{},current_chain{};
        const bool matched=owner(current) &&
            read(image()+0x38e13b0,&current_window,sizeof(current_window)) && current_window==current.renderer+0x58 &&
            read(current_window+0x18,&current_chain,sizeof(current_chain)) && current_chain==uintptr_t(world.chain.Get()) &&
            current.frame==world.frame_pass.native_frame &&
            current.view==world.frame_pass.view_key && current.window==world.frame_pass.window_key &&
            current.context==uintptr_t(world.context.Get()) && current.chain==uintptr_t(world.chain.Get());
        if (matched && same_source()) {
            // Earlier refusal stays sticky. No additional source is minted for retries.
            if (world.inputs) screen_facts(world.pass,false);
            if (!world.refused) {
                close_submission();
                cpu_phase=begin_cpu_present_phase(world.cpu,world.frame_pass.native_frame,world.frame_pass.window_key);
                static std::atomic_flag announced = ATOMIC_FLAG_INIT;
                if (!announced.test_and_set(std::memory_order_relaxed))
                    report("Fallout4 native SUBMISSION end before original outer Present; FRAME/WINDOW remain open.");
            } else {
                diagnostic("Fallout4 pre-Present screen/refusal changed; submission completion withheld from FG.");
            }
        } else {
            // Missing completion refuses prepare; the old scope drains after Swap.
            world.refused=true;
            diagnostic("Fallout4 pre-Present source/owner changed; submission completion withheld from FG.");
        }
    }
    SetLastError(error);
    native_present(renderer);
    const auto native_error=GetLastError();
    if (cpu_phase) end_cpu_present_phase(world.cpu);
    SetLastError(native_error);
}
struct Hook {
    uintptr_t rva;
    const char* bytes;
    size_t length;
    void* detour;
    void** original;
    bool created{};
};
Hook hooks[]{
    {0x1839ed0, "\x48\x83\xec\x18\x8b\x05\x5e\x48\x71\x01", 10, reinterpret_cast<void*>(&observe_dynamic_resolution), reinterpret_cast<void**>(&dynamic_resolution)},
    {0x21f38b0, "\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10", 10, reinterpret_cast<void*>(&observe_prepass), reinterpret_cast<void**>(&prepass)},
    {0x1818550, "\x40\x53\x48\x83\xec\x20", 6, reinterpret_cast<void*>(&observe_native_present), reinterpret_cast<void**>(&native_present)},
    {0x21f0ac0, "\x48\x8b\xc4\x53\x48\x81\xec\xb0\x00\x00\x00", 11, reinterpret_cast<void*>(&observe_world_begin), reinterpret_cast<void**>(&world_begin)},
    {0x1839ec0, "\x88\x91\xe4\x0f\x00\x00\xc3", 7, reinterpret_cast<void*>(&observe_viewport), reinterpret_cast<void**>(&viewport)},
    {0x21fa200, "\x48\x89\x5c\x24\x10\x56\x48\x83\xec\x40", 10, reinterpret_cast<void*>(&observe_imagespace), reinterpret_cast<void**>(&imagespace)},
    {0x2185420, "\x48\x89\x5c\x24\x18\x55\x56\x57\x41\x55\x41\x56\x48\x83\xec\x30", 16, reinterpret_cast<void*>(&observe_effects), reinterpret_cast<void**>(&effects)},
    {0x2214fc0, "\x83\x3d\x85\x22\xb7\x01\x01\x75\x12\x80\x79\x08\x00\x74\x0c\x83\x3d\x7a\x22\xb7\x01\x00\x75\x03\xb0\x01\xc3", 27, reinterpret_cast<void*>(&observe_taa), reinterpret_cast<void**>(&taa_active)}
};
bool call(uintptr_t rva, uintptr_t target) {
    unsigned char bytes[5]{};
    if (!read(image() + rva, bytes, sizeof(bytes)) || bytes[0] != 0xe8) return false;
    int32_t relative{}; std::memcpy(&relative, bytes + 1, sizeof(relative));
    return uintptr_t(intptr_t(rva + 5) + relative) == target;
}
}
bool verify_render_hooks() noexcept try {
    for (const auto& hook : hooks) {
        char actual[32]{};
        if (!read(image() + hook.rva, actual, hook.length) || std::memcmp(actual, hook.bytes, hook.length)) return false;
    }
    const auto guard = [](uintptr_t rva, const char* bytes, size_t length) {
        char actual[16]{};
        return length <= sizeof(actual) && read(image() + rva, actual, length) && !std::memcmp(actual, bytes, length);
    };
    uintptr_t native_taa{};
    return verify_resolution() && verify_screen_queries() && verify_native_depth_domains() &&
        read(image() + 0x2915eb0 + 8 * sizeof(void*), &native_taa, sizeof(native_taa)) &&
        native_taa == image() + 0x2214fc0 && call(0xc335e7,0x1818550) &&
        call(0x21f9f73, 0x21f38b0) &&
        call(0x21f9f2f, 0x1839ed0) &&
        call(0x21f9f69, 0x21f30b0) &&
        call(0x21f32a5, 0x182bf20) && call(0x21f32bb, 0x182bf20) && call(0x21f32d1, 0x182bf20) &&
        call(0x182c02d, 0x182d750) &&
        call(0x182c597, 0x1817ef0) &&
        guard(0xc335e0, "\x48\x8d\x0d\x19\x27\xdf\x02", 7) &&
        guard(0x1818572, "\x48\x8b\x05\x37\x8e\x0c\x02", 7) &&
        guard(0x181858a, "\x48\x8b\x48\x18", 4) &&
        guard(0x181859b, "\x41\xff\x51\x40", 4) &&
        guard(0x18185b6, "\x48\x8b\x05\xf3\x8d\x0c\x02", 7) &&
        guard(0x18185c4, "\x48\x8b\x48\x18", 4) &&
        guard(0x18185d5, "\x41\xff\x51\x40", 4) &&
        call(0xc382a0, 0x21f0ac0) &&
        call(0x21f0ae6, 0x1839ec0) && call(0x21f0e4c, 0x182d580) &&
        call(0x21fa283, 0x2185420) && call(0x21fa2c5, 0x1839ec0) &&
        call(0xc383c2, 0x21fa200) && call(0xc39d6a, 0x21fa200) && call(0xc39e0e, 0x21fa200);
} catch (...) { return false; }
bool prepare_render_hooks(const rsf_game_host_services& services) {
    if (live.load(std::memory_order_acquire) || render_hooks_busy() || !services.process_upscale || !services.render_pass ||
        !verify_render_hooks() || !verify_render_inputs()) return false;
    const auto runtime=GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    upscale_auxiliaries=reinterpret_cast<decltype(upscale_auxiliaries)>(GetProcAddress(runtime,"rsf_d3d11_host_upscale_auxiliaries"));
    restore_auxiliaries=reinterpret_cast<decltype(restore_auxiliaries)>(GetProcAddress(runtime,"rsf_d3d11_host_restore_auxiliaries"));
    if (!upscale_auxiliaries || !restore_auxiliaries || !services.render_config) return false;
    std::array<Hook*, sizeof(hooks) / sizeof(hooks[0])> created{};
    size_t count{};
    for (auto& hook : hooks) {
        if (hook.created) continue;
        if (MH_CreateHook(reinterpret_cast<void*>(image() + hook.rva), hook.detour, hook.original) != MH_OK) {
            while (count) {
                auto& added = *created[--count];
                if (MH_RemoveHook(reinterpret_cast<void*>(image() + added.rva)) == MH_OK) {
                    added.created = false;
                    *added.original = nullptr;
                } else report("Fallout4 cold render-hook rollback refused; retained hook ownership, startup remains refused.");
            }
            return false;
        }
        hook.created = true;
        created[count++] = &hook;
    }
    host = services;
    return true;
}
bool start_render_hooks() {
    if (render_hooks_busy() || !verify_render_hooks()) return false;
    bool queued = true;
    for (auto& hook : hooks) if (!hook.created || MH_QueueEnableHook(reinterpret_cast<void*>(image() + hook.rva)) != MH_OK) queued = false;
    if (queued) live.store(true, std::memory_order_release);
    if (!queued || MH_ApplyQueued() != MH_OK) {
        live.store(false, std::memory_order_release);
        for (auto& hook : hooks) if (hook.created) (void)MH_QueueDisableHook(reinterpret_cast<void*>(image() + hook.rva));
        (void)MH_ApplyQueued();
        return false;
    }
    report("Fallout4 guarded native DR/SR/auxiliary output callbacks enabled; actual producer and screen admission required, FG/input capabilities remain gated.");
    return true;
}
void quiesce_render_hooks() { live.store(false, std::memory_order_release); }
bool stop_render_hooks() {
    quiesce_render_hooks();
    if (render_hooks_busy()) return false;
    bool disabled = true;
    for (auto& hook : hooks) if (hook.created) {
        const auto result = MH_DisableHook(reinterpret_cast<void*>(image() + hook.rva));
        disabled = disabled && (result == MH_OK || result == MH_ERROR_DISABLED);
    }
    return disabled && !render_hooks_busy();
}
bool render_hooks_busy() {
    return entered.load(std::memory_order_acquire) || windows.load(std::memory_order_acquire) || frames.load(std::memory_order_acquire) || auxiliaries.load(std::memory_order_acquire) || resolution_busy();
}
void finish_render_scope() noexcept try {
    if (!world.cpu.source) return;
    if (!same_source()) world.refused=true;
    // Refused/unpresented sources drain here; only the guarded pre-Present path
    // can provide completion in time for that source's generation prepare.
    if (world.submission_open) {
        world.refused=true;
        diagnostic("Fallout4 refused/unpresented SUBMISSION balanced after Swap, before FRAME end.");
        close_submission();
    }
    if (world.refused) world.pass.flags|=RSF_GAME_RENDER_NO_FG;
    if (world.window_open) {
        notify(world.pass, world.context.Get(), 0);
        world.window_open = false;
        windows.fetch_sub(1, std::memory_order_acq_rel);
    }
    if (world.frame_open) {
        if (world.refused) world.frame_pass.flags|=RSF_GAME_RENDER_NO_FG;
        notify(world.frame_pass,world.context.Get(),0);
        world.frame_open=false;
        frames.fetch_sub(1,std::memory_order_acq_rel);
    }
    (void)drain_auxiliaries();
    if (!finish_resolution_source()) world.refused=true;
    if (!resolution_busy() && !world.auxiliary_open) world = {};
} catch (...) {}
}
