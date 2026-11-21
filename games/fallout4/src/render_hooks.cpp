// SPDX-License-Identifier: MIT
#include "render_hooks.h"
#include "render_hooks_platform.h"
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
#include <utility>
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
decltype(&rsf_d3d11_host_reduced_frame) reduced_frame{};
decltype(&rsf_d3d11_host_capture_texture) capture_texture{};
using CaptureRequested = int (*)();
CaptureRequested capture_requested{};
// Bound by the Rust prepare (render_hooks.rs) before any hook is enabled.
rsf_game_host_services host{};
// Rust owns the live flag with its release store and acquire load.
bool live() noexcept { return rsf_fallout4_render_live(); }
std::atomic<uint32_t> entered{}, windows{}, frames{}, auxiliaries{};
struct World {
    CpuIdentity cpu{};
    rsf_game_render_pass pass{}, frame_pass{}, submission_pass{}, screen_basis{};
    NativeScreen screen{};
    NativeDepthSnapshot depth_snapshot{}, raster_snapshot{};
    // Recovery leases. Rust (render_hooks/recovery.rs) owns the token and decides adoption and
    // release; the candidate lives only for one Rust step.
    NativeInputs recovery_inputs{}, recovery_candidate{};
    // SR input and HUDless framebuffer leases. Rust (render_hooks/publication.rs) releases each
    // when its step returns, as the former stack leases ended.
    NativeInputs sr_inputs{};
    NativeFramebuffer hudless{};
    bool full_output_resolved{};
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
    return world.cpu.session == host.session_id && render_source_is_current(world.cpu);
}
void notify(const rsf_game_render_pass& pass, void* context, uint32_t begin) noexcept {
    if (host.render_pass) try { host.render_pass(host.user, context, &pass, begin); } catch (...) {}
}
// Rust owns the pass identity (render_hooks/publication.rs).
void identify(rsf_game_render_pass& pass) noexcept {
    const auto cpu=cpu_identity_abi(world.cpu);
    rsf_fallout4_render_identify(&pass,&cpu);
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
// Recovery leaves (render_hooks_platform.h). Rust sequences the transaction and decides every
// verdict; these read the World, compare COM identity, hold the leases and guard the Runtime.
static_assert(sizeof(rsf_d3d11_reduced_frame)==56 && offsetof(rsf_d3d11_reduced_frame,result_flags)==52);
static_assert(sizeof(rsf_d3d11_auxiliary_surface)==80 && offsetof(rsf_d3d11_auxiliary_surface,flags)==72);
static_assert(sizeof(NativeOutputOwner)==40);
void recovery_refuse() noexcept { world.refused=true; }
void recovery_facts(NativeRecoveryFacts* facts) noexcept {
    *facts={world.context.Get(),world.cpu.source,reduced_frame!=nullptr,world.frame_open,world.full_output_resolved};
}
bool recovery_same_source() noexcept { return same_source(); }
void recovery_resolve(bool full_output, bool published) noexcept {
    world.full_output_resolved=full_output; world.published=published;
}
bool recovery_read_raster(bool producer) noexcept {
    world.recovery_candidate={};
    if (!producer) return read_native_raster_inputs(world.recovery_candidate,world.frame_pass);
    return world.raster_snapshot.raster_valid &&
        read_native_raster_inputs(world.recovery_candidate,world.frame_pass,&world.raster_snapshot);
}
// Colour, depth, motion, Pip-Boy, VATS and refraction COM identity, in that order; owners adds
// Context and chain first.
bool same_resources(const NativeInputs& current, const NativeInputs& held, bool owners) noexcept {
    return (!owners || (same_object(current.context.Get(),held.context.Get()) && same_object(current.chain.Get(),held.chain.Get()))) &&
        same_object(current.colour.Get(),held.colour.Get()) && same_object(current.depth.Get(),held.depth.Get()) &&
        same_object(current.motion.Get(),held.motion.Get()) && same_object(current.pipboy.Get(),held.pipboy.Get()) &&
        same_object(current.vats.Get(),held.vats.Get()) && same_object(current.refraction_normals.Get(),held.refraction_normals.Get());
}
bool recovery_same_resources(bool owners) noexcept {
    return same_resources(world.recovery_candidate,world.recovery_inputs,owners);
}
void recovery_adopt(const int32_t* render_rect) noexcept {
    auto& current=world.recovery_candidate;
    if (render_rect) std::memcpy(current.pass.render_rect,render_rect,sizeof(current.pass.render_rect));
    world.recovery_inputs=std::move(current); current={};
}
void recovery_discard() noexcept { world.recovery_candidate={}; }
void recovery_release() noexcept { world.recovery_inputs={}; }
void recovery_lease(uint32_t slot, NativeRecoveryLease* lease) noexcept {
    const auto& inputs=slot==RecoveryLeaseCandidate ? world.recovery_candidate : world.recovery_inputs;
    *lease={&inputs.pass,inputs.context.Get(),{inputs.depth.Get(),inputs.motion.Get(),inputs.pipboy.Get(),
        inputs.vats.Get(),inputs.refraction_normals.Get()}};
}
bool recovery_preflight(const rsf_game_render_pass* pass, void* context, NativeOutputOwner* owner) noexcept {
    return preflight_output_resolution(*pass,context,owner);
}
bool recovery_raster_current() noexcept { return native_raster_snapshot_current(world.raster_snapshot); }
bool recovery_begin_output() noexcept {
    return begin_output_resolution(world.raster_snapshot,world.recovery_inputs.pass);
}
bool recovery_reduce(rsf_d3d11_reduced_frame* request, int32_t* result) noexcept {
    if (!reduced_frame || !world.recovery_inputs.context) return false;
    request->owner=&world.recovery_inputs.pass;
    *result=RSF_ERROR_NOT_READY;
    try { *result=reduced_frame(host.user,world.recovery_inputs.context.Get(),request); } catch (...) {}
    return true;
}
constexpr NativeRecoveryLeaves recovery_leaves{report, recovery_refuse, recovery_facts, recovery_same_source,
    recovery_resolve, recovery_read_raster, recovery_same_resources, recovery_adopt, recovery_discard,
    recovery_release, recovery_lease, recovery_preflight, recovery_raster_current, recovery_begin_output,
    recovery_reduce};
bool recovery_owned() noexcept { return rsf_fallout4_recovery_token()!=0; }
bool reserve_recovery(const rsf_game_render_pass& planned) noexcept {
    return rsf_fallout4_recovery_reserve(&recovery_leaves,&planned);
}
bool retire_recovery() noexcept { return rsf_fallout4_recovery_retire(&recovery_leaves); }
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
    if (recovery_owned() && count!=RecoverySurfaceCount) return RSF_ERROR_NATIVE_REFUSED;
    const rsf_d3d11_auxiliary_pass request{sizeof(request),RSF_D3D11_HOST_ABI_VERSION,&pass,count,surfaces.data()};
    if (!recovery_owned()) {
        world.auxiliary_scope={sizeof(world.auxiliary_scope),RSF_D3D11_HOST_ABI_VERSION,pass.session_id,pass.native_frame,pass.view_key};
        world.auxiliary_open=true; auxiliaries.fetch_add(1,std::memory_order_acq_rel);
    }
    // The host adopts the sealed recovery originals while retaining this actual
    // fractional viewport/jitter temporal mapping. One evaluation, one owner.
    const auto result=upscale_auxiliaries(host.user,inputs.context.Get(),&request);
    if (result!=RSF_OK && !recovery_owned()) (void)drain_auxiliaries();
    return result;
}
// SR and HUDless publication leaves (render_hooks_platform.h). Rust (render_hooks/publication.rs)
// sequences both seams and decides every refusal; these read the World, hold the SR input and
// HUDless framebuffer leases, compare COM identity and guard the Runtime calls.
static_assert(sizeof(NativeScreen)==24 && offsetof(NativeScreen,camera_state)==16);
static_assert(int(ResolutionStage::output_ready)==1 && int(ResolutionStage::evaluated)==2);
void publication_world(NativePublicationWorld* out) noexcept {
    *out={cpu_identity_abi(world.cpu),&world.pass,&world.frame_pass,&world.screen_basis,world.context.Get(),
        world.chain.Get(),host.process_upscale!=nullptr,world.in_imagespace,world.inputs,world.window_open,
        world.frame_open,world.submission_open,world.refused,world.raster_refused,world.auxiliary_open,
        world.depth_snapshot.valid};
}
bool publication_read_inputs() noexcept {
    world.sr_inputs={};
    return read_native_inputs(world.sr_inputs,report,&world.depth_snapshot);
}
void publication_inputs(NativePublicationInputs* out) noexcept {
    *out={&world.sr_inputs.pass,world.sr_inputs.context.Get(),world.sr_inputs.chain.Get()};
}
void publication_release_inputs() noexcept { world.sr_inputs={}; }
bool publication_same_recovery_resources() noexcept {
    return same_resources(world.sr_inputs,world.recovery_inputs,false);
}
bool publication_depth_domains(rsf_game_render_pass* pass) noexcept {
    return make_native_depth_domains(world.depth_snapshot,world.cpu,*pass,pass->depth_domains,report);
}
bool publication_plan(const rsf_game_render_pass* planning) noexcept {
    try { (void)host.process_upscale(host.user,world.sr_inputs.context.Get(),planning); } catch (...) { return false; }
    return true;
}
int32_t publication_render_config(rsf_game_render_config* config) noexcept {
    try { return host.render_config && host.render_config(host.user,config) ? 1 : 0; } catch (...) { return -1; }
}
void publication_retain_inputs(const rsf_game_render_pass* basis) noexcept {
    world.colour=world.sr_inputs.colour; world.depth=world.sr_inputs.depth; world.motion=world.sr_inputs.motion;
    world.screen_basis=*basis;
}
void publication_classify(bool first, NativeScreen* current, NativeScreen* leased) noexcept {
    *current=classify_native_screen(world.cpu,world.screen_basis,true);
    if (first) world.screen=*current;
    *leased=world.screen;
}
void publication_notify(const rsf_game_render_pass* pass, void* context, uint32_t begin) noexcept {
    notify(*pass,context,begin);
}
void publication_publish_inputs(const rsf_game_render_pass* scalar) noexcept {
    world.pass=*scalar;
    world.context=world.sr_inputs.context;
    world.chain=world.sr_inputs.chain;
    world.inputs=true;
}
bool publication_begin_output(const rsf_game_render_pass* pass) noexcept {
    return begin_output_resolution(world.depth_snapshot,*pass);
}
void publication_report_state(uint32_t stage, const rsf_game_render_pass* pass, void* context, bool published,
    int32_t result) noexcept {
    report_resolution_state(static_cast<ResolutionStage>(stage),*pass,context,published,result,report);
}
int32_t publication_reconstruct(const rsf_game_render_pass* pass) noexcept {
    int32_t result=RSF_ERROR_NOT_READY;
    try { result=reconstruct(world.sr_inputs,*pass); } catch (...) { (void)drain_auxiliaries(); }
    return result;
}
bool publication_read_framebuffer() noexcept {
    world.hudless={};
    return read_native_framebuffer(world.hudless,report);
}
void publication_framebuffer(NativePublicationFramebuffer* out) noexcept {
    *out={world.hudless.context.Get(),world.hudless.chain.Get(),world.hudless.texture.Get(),
        world.hudless.native_frame,world.hudless.window_key};
}
void publication_framebuffer_extent(uint32_t extent[3]) noexcept {
    D3D11_TEXTURE2D_DESC description{};
    if (world.hudless.texture) world.hudless.texture->GetDesc(&description);
    extent[0]=description.Width; extent[1]=description.Height; extent[2]=description.SampleDesc.Count;
}
void publication_release_framebuffer() noexcept { world.hudless={}; }
void publication_publish_window(const rsf_game_render_pass* window) noexcept {
    world.final_colour=world.hudless.texture;
    world.pass=*window;
    world.window_open=true;
    windows.fetch_add(1,std::memory_order_acq_rel);
}
constexpr NativePublicationLeaves publication_leaves{publication_world, diagnostic, publication_read_inputs,
    publication_inputs, publication_release_inputs, publication_same_recovery_resources, publication_depth_domains,
    publication_plan, publication_render_config, publication_retain_inputs, publication_classify,
    publication_notify, publication_publish_inputs, publication_begin_output, publication_report_state,
    publication_reconstruct, end_output_resolution, publication_read_framebuffer, publication_framebuffer,
    publication_framebuffer_extent, publication_release_framebuffer, publication_publish_window};
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
    if (live() && manager == reinterpret_cast<void*>(image() + 0x2f4d750)) {
        if (caller == image() + 0x21fa2ca) {
            report_resolution_state(ResolutionStage::hudless,world.inputs ? world.pass : world.frame_pass,world.context.Get(),world.published,RSF_OK,report);
            rsf_fallout4_render_emit_hudless(&publication_leaves,&recovery_leaves);
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
    const auto cpu = cpu_identity_abi(current_cpu_identity());
    struct Restore {
        bool previous;
        ~Restore() { ordinary_world = previous; }
    } restore{ordinary_world};
    const bool active=live();
    const NativeWorldAdmission candidate{caller, active ? host.session_id : 0, active,
        ordinary_world, world.frame_open, world.auxiliary_open, recovery_owned()};
    if (rsf_fallout4_render_admit_world(&candidate, &cpu)) {
        ordinary_world=true;
        world={}; rsf_fallout4_recovery_reset(); world.cpu=current_cpu_identity();
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
    if (live() && caller==image()+0x21f9f34 &&
        manager==reinterpret_cast<void*>(image()+0x2f4d750) && !overlapping && world.frame_open && world.submission_open && !world.inputs && same_source()) {
        Owner owner_now{};
        const rsf_game_render_pass candidate{};
        const auto screen=classify_native_screen(world.cpu,candidate,true);
        if (screen.refusals==ScreenWorldLeaseInvalid && owner(owner_now) &&
            owner_now.frame==world.frame_pass.native_frame && owner_now.view==world.frame_pass.view_key &&
            owner_now.window==world.frame_pass.window_key && owner_now.context==uintptr_t(world.context.Get()) && owner_now.chain==uintptr_t(world.chain.Get())) {
            if (!configure_native_resolution(host,world.frame_pass,world.context.Get(),reserve_recovery) && (resolution_busy() || resolution_refused()))
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
    const bool selected = (live() || recovery_owned()) && prepass_depth == 1 &&
        caller == image() + 0x21f9f78 && world.frame_open &&
        world.submission_open && !world.inputs;
    // Every producer call invalidates the previous copy, including foreign views.
    if (world.frame_open) {
        world.depth_snapshot = {}; world.raster_snapshot = {};
        if (prepass_depth != 1) world.depth_interrupted = true;
        if (world.inputs) world.refused = true;
    }
    if (selected) world.depth_interrupted = false;
    SetLastError(error);
    prepass();
    const auto native_error = GetLastError();
    if (selected && (live() || recovery_owned()) &&
        world.frame_open && world.submission_open && !world.inputs &&
        !world.depth_interrupted) {
        (void)capture_native_raster(identity,world.raster_snapshot,report);
        NativeDepthSnapshot snapshot{};
        if (capture_native_depth_domains(identity, snapshot, report) &&
            render_source_is_current(identity) && same_source()) {
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
    const bool selected = !world.in_imagespace && world.frame_open &&
        ((live() && same_source()) || recovery_owned()) &&
        (caller == image() + 0xc383c7 || caller == image() + 0xc39d6f || caller == image() + 0xc39e13);
    struct Cleanup {
        bool selected;
        ~Cleanup() {
            if (!selected) return;
            const auto error = GetLastError();
            if (!retire_recovery()) diagnostic("Fallout4 reduced recovery remains owned after imagespace; next reduction blocked.");
            if (!drain_auxiliaries()) diagnostic("Fallout4 auxiliary retirement remains pending after native imagespace; owner retained.");
            report_resolution_state(ResolutionStage::ui_return,world.inputs ? world.pass : world.frame_pass,world.context.Get(),world.published,RSF_OK,report);
            if (!end_output_resolution()) world.refused=true;
            report_resolution_state(ResolutionStage::ui_restored,world.inputs ? world.pass : world.frame_pass,world.context.Get(),world.published,RSF_OK,report);
            if (output_resolution_busy()) { world.refused=true; diagnostic("Fallout4 native output-state cleanup remains owned; subsequent world admission blocked."); }
            world.in_imagespace = world.published = world.full_output_resolved = false;
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
    if ((live() || recovery_owned()) && caller == image() + 0x21fa288 && first == 0 && last == 13 && mode == 1 && flags == 1 &&
        read(image() + 0x3e5e4b0, &native_manager, sizeof(native_manager)) && uintptr_t(manager) == native_manager) {
        report_resolution_state(ResolutionStage::pre_sr,world.frame_pass,world.context.Get(),world.published,RSF_ERROR_NOT_READY,report);
        if (rsf_fallout4_render_emit_sr(&publication_leaves,&recovery_leaves)==NativeSrOutcome::failed)
            diagnostic("Fallout4 SR callback failed; native imagespace retained.");
    }
    SetLastError(error);
    effects(manager, first, last, mode, flags);
    const auto native_error=GetLastError();
    if (caller==image()+0x21fa288 && first==0 && last==13 && mode==1 && flags==1)
        report_resolution_state(ResolutionStage::effects_return,world.inputs ? world.pass : world.frame_pass,world.context.Get(),world.published,RSF_OK,report);
    if (caller==image()+0x21fa288 && first==0 && last==13 && mode==1 && flags==1 && recovery_owned())
        if (!retire_recovery()) diagnostic("Fallout4 recovery VATS retirement refused at original Effects return; ownership retained.");
    if (caller==image()+0x21fa288 && first==0 && last==13 && mode==1 && flags==1 && world.auxiliary_open)
        if (!drain_auxiliaries()) diagnostic("Fallout4 VATS/auxiliary restoration refused at native Effects return; owner retained.");
    SetLastError(native_error);
}
bool observe_taa(void* effect) {
    Entry entry;
    const auto result = taa_active(effect);
    const auto error = GetLastError();
    const bool owner_current = world.in_imagespace && world.full_output_resolved &&
        (world.raster_snapshot.raster_valid ? native_raster_snapshot_current(world.raster_snapshot) : same_source());
    const bool excluded = owner_current;
    // One receipt per observed gate combination, not an unbounded frame log.
    const unsigned key=unsigned(result) | (unsigned(world.in_imagespace)<<1) |
        (unsigned(world.full_output_resolved)<<2) | (unsigned(world.published)<<3) | (unsigned(excluded)<<4);
    static std::atomic<uint32_t> receipts{};
    if (!(receipts.fetch_or(uint32_t(1)<<key,std::memory_order_relaxed)&(uint32_t(1)<<key))) {
        char text[400]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 TAA gate source=%llu native=%llu original=%u imagespace=%u full_output=%u temporal=%u owner_current=%u native_taa_runs=%u recovery=%llu",
            static_cast<unsigned long long>(world.cpu.source),
            static_cast<unsigned long long>(world.frame_pass.native_frame),unsigned(result),unsigned(world.in_imagespace),
            unsigned(world.full_output_resolved),unsigned(world.published),unsigned(owner_current),unsigned(result && !excluded),
            static_cast<unsigned long long>(rsf_fallout4_recovery_token()));
        report(text);
    }
    SetLastError(error);
    return result && !excluded;
}
bool read_present_owner(NativeRenderScopeKey* key, uintptr_t* outer_chain) noexcept {
    Owner current{};
    uintptr_t current_window{};
    if (!owner(current) ||
        !read(image()+0x38e13b0,&current_window,sizeof(current_window)) || current_window!=current.renderer+0x58 ||
        !read(current_window+0x18,outer_chain,sizeof(*outer_chain))) return false;
    *key={current.frame,current.view,current.window,current.context,current.chain};
    return true;
}
void capture_recovery_output() {
    // Preserve failed reduced-frame output for diagnostics independently of
    // temporal/FG admission. The runtime rechecks the exact capture identity.
    if (rsf_fallout4_recovery_committed() && capture_texture && capture_requested &&
        capture_requested() && native_raster_snapshot_current(world.raster_snapshot)) {
        NativeFramebuffer observed;
        if (read_native_framebuffer(observed,report) && observed.native_frame==world.frame_pass.native_frame &&
            observed.window_key==world.frame_pass.window_key && observed.context.Get()==world.context.Get() &&
            same_object(observed.chain.Get(),world.chain.Get())) {
            rsf_d3d11_host_capture capture{};
            capture.struct_size=sizeof(capture); capture.abi_version=RSF_D3D11_HOST_ABI_VERSION;
            capture.session_id=world.cpu.session; capture.native_frame=world.frame_pass.native_frame;
            capture.view_key=world.frame_pass.view_key; capture.texture=observed.texture.Get();
            capture.name="recovery_native_output";
            capture.description="Native framebuffer before Present after reduced rendering; no temporal or FG admission implied";
            capture.view=0; capture.scale=1;
            (void)capture_texture(host.user,observed.context.Get(),&capture);
        }
    }
}
bool revalidate_present_screen() noexcept {
    if (world.inputs) rsf_fallout4_render_screen_facts(&publication_leaves,&recovery_leaves,&world.pass,false);
    return world.refused;
}
void observe_native_present(void* renderer) {
    Entry entry;
    const auto error=GetLastError();
#if defined(_MSC_VER)
    const auto caller=uintptr_t(_ReturnAddress());
#else
    const auto caller=uintptr_t(__builtin_return_address(0));
#endif
    CpuIdentity phase_owner{};
    bool cpu_phase=false;
    const NativeRenderScopeKey leased{world.frame_pass.native_frame,world.frame_pass.view_key,
        world.frame_pass.window_key,uintptr_t(world.context.Get()),uintptr_t(world.chain.Get())};
    const auto matched=rsf_fallout4_render_match_present(caller,uintptr_t(renderer),world.submission_open,
        &leased,read_present_owner);
    if (matched!=NativePresentMatch::unrelated) {
        if (matched==NativePresentMatch::matched) capture_recovery_output();
        const auto cpu=cpu_identity_abi(world.cpu);
        switch (rsf_fallout4_render_admit_present(matched,host.session_id,&cpu,revalidate_present_screen)) {
        case NativePresentAdmission::complete: {
            close_submission();
            // Close the same CPU phase after the original, even if it reenters.
            phase_owner=world.cpu;
            cpu_phase=begin_cpu_present_phase(phase_owner,world.frame_pass.native_frame,world.frame_pass.window_key);
            static std::atomic_flag announced = ATOMIC_FLAG_INIT;
            if (!announced.test_and_set(std::memory_order_relaxed))
                report("Fallout4 native SUBMISSION end before original outer Present; FRAME/WINDOW remain open.");
            break;
        }
        case NativePresentAdmission::refused:
            diagnostic("Fallout4 pre-Present screen/refusal changed; submission completion withheld from FG.");
            break;
        case NativePresentAdmission::source_changed:
            // Missing completion refuses prepare; the old scope drains after Swap.
            world.refused=true;
            diagnostic("Fallout4 pre-Present source/owner changed; submission completion withheld from FG.");
            break;
        }
    }
    SetLastError(error);
    native_present(renderer);
    const auto native_error=GetLastError();
    if (cpu_phase) end_cpu_present_phase(phase_owner);
    SetLastError(native_error);
}
// NativeRenderHook order. MinHook writes each typed trampoline at creation; the detours call
// them on their own C++ frames. Rust (render_hooks.rs) owns the RVAs, entry bytes and created
// state of these records.
const std::array<void*, RenderHookCount> detours{
    reinterpret_cast<void*>(&observe_dynamic_resolution), reinterpret_cast<void*>(&observe_prepass),
    reinterpret_cast<void*>(&observe_native_present), reinterpret_cast<void*>(&observe_world_begin),
    reinterpret_cast<void*>(&observe_viewport), reinterpret_cast<void*>(&observe_imagespace),
    reinterpret_cast<void*>(&observe_effects), reinterpret_cast<void*>(&observe_taa)};
const std::array<void**, RenderHookCount> originals{
    reinterpret_cast<void**>(&dynamic_resolution), reinterpret_cast<void**>(&prepass),
    reinterpret_cast<void**>(&native_present), reinterpret_cast<void**>(&world_begin),
    reinterpret_cast<void**>(&viewport), reinterpret_cast<void**>(&imagespace),
    reinterpret_cast<void**>(&effects), reinterpret_cast<void**>(&taa_active)};
bool create_hook(uint32_t hook, void* target) noexcept {
    return hook < detours.size() && MH_CreateHook(target, detours[hook], originals[hook]) == MH_OK;
}
bool remove_hook(uint32_t hook, void* target) noexcept {
    if (hook >= originals.size() || MH_RemoveHook(target) != MH_OK) return false;
    *originals[hook] = nullptr;
    return true;
}
bool queue_enable_hook(void* target) noexcept { return MH_QueueEnableHook(target) == MH_OK; }
bool queue_disable_hook(void* target) noexcept { return MH_QueueDisableHook(target) == MH_OK; }
bool apply_queued() noexcept { return MH_ApplyQueued() == MH_OK; }
// The C++ half of busy; Rust adds the resolution owner.
bool scopes_busy() noexcept {
    return entered.load(std::memory_order_acquire) || windows.load(std::memory_order_acquire) || frames.load(std::memory_order_acquire) || auxiliaries.load(std::memory_order_acquire);
}
void bind_exports(const NativeRenderExports* exports) noexcept {
    upscale_auxiliaries=reinterpret_cast<decltype(upscale_auxiliaries)>(exports->upscale_auxiliaries);
    restore_auxiliaries=reinterpret_cast<decltype(restore_auxiliaries)>(exports->restore_auxiliaries);
    reduced_frame=reinterpret_cast<decltype(reduced_frame)>(exports->reduced_frame);
    capture_texture=reinterpret_cast<decltype(capture_texture)>(exports->capture_texture);
    capture_requested=reinterpret_cast<CaptureRequested>(exports->capture_requested);
}
void bind_host(const rsf_game_host_services* services) noexcept { host = *services; }
constexpr NativeRenderHookLeaves leaves{create_hook, remove_hook, queue_enable_hook, queue_disable_hook,
    apply_queued, scopes_busy, bind_exports, bind_host};
}
const NativeRenderHookLeaves* render_hook_leaves() noexcept { return &leaves; }
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
    (void)retire_recovery();
    (void)drain_auxiliaries();
    const bool resolution_finished=finish_resolution_source();
    if (!resolution_finished) world.refused=true;
    report_resolution_state(ResolutionStage::source_retired,world.inputs ? world.pass : world.frame_pass,
        world.context.Get(),world.published,resolution_finished ? RSF_OK : RSF_ERROR_NOT_READY,report);
    if (!resolution_busy() && !world.auxiliary_open && !recovery_owned()) { world = {}; rsf_fallout4_recovery_reset(); }
} catch (...) {}
}
