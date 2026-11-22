// SPDX-License-Identifier: MIT
#include "render_hooks.h"
#include "render_hooks_platform.h"
#include "cpu_events_platform.h"
#include "render_inputs.h"
#include "resolution_platform.h"
#include "screen.h"
#include "depth_domains.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <array>
#include <atomic>
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
    // Candidate owner Context and chain lease. Rust (render_hooks/scope.rs) adopts it into the
    // World or discards it when the begin step returns.
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> owner_context;
    Microsoft::WRL::ComPtr<IDXGISwapChain> owner_chain;
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
// Nested prepass depth; the detour's guard restores it when the original returns or unwinds.
thread_local uint32_t prepass_depth{};
void report(const char* message) noexcept {
    if (host.log) try { host.log(host.user, message); } catch (...) {}
}
struct Entry {
    Entry() { entered.fetch_add(1, std::memory_order_acq_rel); }
    ~Entry() { entered.fetch_sub(1, std::memory_order_acq_rel); }
};
bool same_source() {
    return world.cpu.session == host.session_id && rsf_fallout4_cpu_render_source_is_current(&world.cpu);
}
void notify(const rsf_game_render_pass& pass, void* context, uint32_t begin) noexcept {
    if (host.render_pass) try { host.render_pass(host.user, context, &pass, begin); } catch (...) {}
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
void read_world(NativeWorld* out) noexcept {
    *out={world.cpu,&world.pass,&world.frame_pass,&world.submission_pass,
        &world.screen_basis,world.context.Get(),world.chain.Get(),host.process_upscale!=nullptr,
        reduced_frame!=nullptr,world.in_imagespace,world.inputs,world.published,world.window_open,
        world.frame_open,world.submission_open,world.refused,world.raster_refused,world.auxiliary_open,
        world.depth_snapshot.valid,world.raster_snapshot.raster_valid,world.depth_interrupted,
        world.full_output_resolved};
}
void recovery_refuse() noexcept { world.refused=true; }
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
bool recovery_same_resources(bool owners) noexcept {
    return rsf_fallout4_inputs_same_resources(&world.recovery_candidate,&world.recovery_inputs,owners);
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
bool recovery_raster_current() noexcept { return rsf_fallout4_depth_raster_current(&world.raster_snapshot); }
bool recovery_begin_output() noexcept {
    return rsf_fallout4_resolution_begin_output(&world.raster_snapshot,&world.recovery_inputs.pass);
}
bool recovery_reduce(rsf_d3d11_reduced_frame* request, int32_t* result) noexcept {
    if (!reduced_frame || !world.recovery_inputs.context) return false;
    request->owner=&world.recovery_inputs.pass;
    *result=RSF_ERROR_NOT_READY;
    try { *result=reduced_frame(host.user,world.recovery_inputs.context.Get(),request); } catch (...) {}
    return true;
}
constexpr NativeRecoveryLeaves recovery_leaves{report, recovery_refuse, read_world, recovery_same_source,
    recovery_resolve, recovery_read_raster, recovery_same_resources, recovery_adopt, recovery_discard,
    recovery_release, recovery_lease, rsf_fallout4_resolution_preflight, recovery_raster_current, recovery_begin_output,
    recovery_reduce};
bool recovery_owned() noexcept { return rsf_fallout4_recovery_token()!=0; }
bool reserve_recovery(const rsf_game_render_pass* planned) noexcept {
    return rsf_fallout4_recovery_reserve(&recovery_leaves,planned);
}
bool publication_auxiliary_inputs(NativeAuxiliaryInputs* out) noexcept {
    const auto& inputs=world.sr_inputs;
    if (!rsf_fallout4_render_auxiliary_depth(inputs.depth.Get())) return false;
    const auto& raster=world.depth_snapshot.actual_raster;
    *out={{inputs.depth.Get(),inputs.motion.Get(),inputs.pipboy.Get(),inputs.vats.Get(),
        inputs.refraction_normals.Get()},{raster[0],raster[1],raster[2],raster[3]}};
    return true;
}
bool publication_auxiliary_colour(void* texture) noexcept {
    const auto& inputs=world.sr_inputs;
    return rsf_fallout4_render_auxiliary_colour(texture,inputs.context.Get(),
        inputs.pass.camera.output_width,inputs.pass.camera.output_height);
}
int32_t publication_reconstruct(const rsf_game_render_pass* pass, const rsf_d3d11_auxiliary_surface* surfaces,
    uint32_t count) noexcept try {
    const auto context=world.sr_inputs.context.Get();
    if (count==0) return host.process_upscale(host.user,context,pass);
    const rsf_d3d11_auxiliary_pass request{sizeof(request),RSF_D3D11_HOST_ABI_VERSION,pass,count,surfaces};
    if (!recovery_owned()) {
        world.auxiliary_scope={sizeof(world.auxiliary_scope),RSF_D3D11_HOST_ABI_VERSION,pass->session_id,pass->native_frame,pass->view_key};
        world.auxiliary_open=true; auxiliaries.fetch_add(1,std::memory_order_acq_rel);
    }
    // The host adopts the sealed recovery originals while retaining this actual
    // fractional viewport/jitter temporal mapping. One evaluation, one owner.
    const auto result=upscale_auxiliaries(host.user,context,&request);
    if (result!=RSF_OK && !recovery_owned()) (void)drain_auxiliaries();
    return result;
} catch (...) {
    (void)drain_auxiliaries();
    return RSF_ERROR_NOT_READY;
}
// SR and HUDless publication leaves (render_hooks_platform.h). Rust (render_hooks/publication.rs)
// sequences both seams. These leaves hold the SR input and retained WINDOW references
// in the native World and guard Runtime calls.
bool publication_read_inputs() noexcept {
    world.sr_inputs={};
    return read_native_inputs(world.sr_inputs,report,&world.depth_snapshot);
}
void publication_inputs(NativePublicationInputs* out) noexcept {
    *out={&world.sr_inputs.pass,world.sr_inputs.context.Get(),world.sr_inputs.chain.Get()};
}
void publication_release_inputs() noexcept { world.sr_inputs={}; }
bool publication_same_recovery_resources() noexcept {
    return rsf_fallout4_inputs_same_resources(&world.sr_inputs,&world.recovery_inputs,false);
}
bool publication_depth_domains(rsf_game_render_pass* pass) noexcept {
    return rsf_fallout4_depth_make(&world.depth_snapshot,&world.cpu,pass,&pass->depth_domains,report);
}
bool publication_plan(const rsf_game_render_pass* planning) noexcept {
    return rsf_fallout4_resolution_reconfigure(host.process_upscale,host.user,world.sr_inputs.context.Get(),planning);
}
int32_t publication_render_config(rsf_game_render_config* config) noexcept {
    return host.render_config ? rsf_fallout4_resolution_config(host.render_config,host.user,config) : 0;
}
void publication_retain_inputs(const rsf_game_render_pass* basis) noexcept {
    world.colour=world.sr_inputs.colour; world.depth=world.sr_inputs.depth; world.motion=world.sr_inputs.motion;
    world.screen_basis=*basis;
}
void publication_classify(bool first, NativeScreen* current, NativeScreen* leased) noexcept {
    rsf_fallout4_screen_classify(&world.cpu,&world.screen_basis,true,current);
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
    return rsf_fallout4_resolution_begin_output(&world.depth_snapshot,pass);
}
void publication_report_state(uint32_t stage, const rsf_game_render_pass* pass, void* context, bool published,
    int32_t result) noexcept {
    rsf_fallout4_resolution_report(stage,pass,context,published,result,report);
}
void publication_publish_window(void* texture, const rsf_game_render_pass* window) noexcept {
    world.final_colour=static_cast<ID3D11Texture2D*>(texture);
    world.pass=*window;
    world.window_open=true;
    windows.fetch_add(1,std::memory_order_acq_rel);
}
constexpr NativePublicationLeaves publication_leaves{read_world, rsf_fallout4_render_diagnostic, publication_read_inputs,
    publication_inputs, publication_release_inputs, publication_same_recovery_resources, publication_depth_domains,
    publication_plan, publication_render_config, publication_retain_inputs, publication_classify,
    publication_notify, publication_publish_inputs, publication_begin_output, publication_report_state,
    publication_auxiliary_inputs, publication_auxiliary_colour, rsf_fallout4_render_same_object,
    publication_reconstruct, rsf_fallout4_resolution_end_output, publication_publish_window};
// FRAME, SUBMISSION and WINDOW scope leaves (render_hooks_platform.h). Rust (render_hooks/scope.rs)
// decides the owner lease verdict, the FRAME identity and the begin and close order; these read
// the native owner, compare COM identity, hold the Context and chain leases and keep the counters.
bool scope_lease_owner(const NativeRenderOwner* before) noexcept try {
    world.owner_context=reinterpret_cast<ID3D11DeviceContext*>(before->context);
    world.owner_chain=reinterpret_cast<IDXGISwapChain*>(before->chain);
    return rsf_fallout4_render_check_owner(world.owner_context.Get(),world.owner_chain.Get(),
        before->device,before->window);
} catch (...) { return false; }
void scope_adopt_owner() noexcept {
    world.context=std::move(world.owner_context); world.chain=std::move(world.owner_chain);
}
void scope_discard_owner() noexcept { world.owner_context.Reset(); world.owner_chain.Reset(); }
void scope_open_frame() noexcept { world.frame_open=true; frames.fetch_add(1,std::memory_order_acq_rel); }
void scope_open_submission() noexcept { world.submission_open=true; }
void scope_close_submission() noexcept { world.submission_open=false; }
void scope_close_window() noexcept { world.window_open=false; windows.fetch_sub(1,std::memory_order_acq_rel); }
void scope_close_frame() noexcept { world.frame_open=false; frames.fetch_sub(1,std::memory_order_acq_rel); }
void scope_reset() noexcept { world={}; }
constexpr NativeScopeLeaves scope_leaves{read_world, rsf_fallout4_render_read_owner, scope_lease_owner, scope_adopt_owner,
    scope_discard_owner, scope_open_frame, scope_open_submission, scope_close_submission, scope_close_window,
    scope_close_frame, drain_auxiliaries, rsf_fallout4_resolution_finish_source, scope_reset, publication_notify, rsf_fallout4_render_diagnostic,
    publication_report_state};
// Imagespace, Effects and TAA leaves (render_hooks_platform.h). Rust (render_hooks/imagespace.rs)
// selects the calls and sequences entry, the SR seam, retirement, drain, output resolution and
// the TAA gate; these read the World, read the native manager cell and keep the World flags.
void imagespace_enter() noexcept { world.in_imagespace=true; }
void imagespace_leave() noexcept { world.in_imagespace=world.published=world.full_output_resolved=false; }
constexpr NativeImagespaceLeaves imagespace_leaves{read_world, imagespace_enter, imagespace_leave,
    rsf_fallout4_render_read_manager, drain_auxiliaries, rsf_fallout4_resolution_end_output, publication_report_state, rsf_fallout4_render_diagnostic};
// Viewport, DR and prepass leaves (render_hooks_platform.h). Rust (render_hooks/producer.rs)
// selects the calls and sequences the selector write, the HUDless seam, the DR admission and the
// prepass capture; these read the World, call the typed screen, owner, resolution and depth
// readers and keep the World fields and snapshots.
void producer_invalidate(bool raster) noexcept {
    world.depth_snapshot={};
    if (raster) world.raster_snapshot={};
}
void producer_interrupt_depth(bool interrupted) noexcept { world.depth_interrupted=interrupted; }
void producer_refuse_raster() noexcept { world.raster_refused=world.refused=true; }
uint32_t producer_screen_refusals() noexcept {
    const rsf_game_render_pass candidate{};
    NativeScreen screen{};
    rsf_fallout4_screen_classify(&world.cpu,&candidate,true,&screen);
    return screen.refusals;
}
bool producer_configure() noexcept {
    return rsf_fallout4_resolution_configure(&host,&world.frame_pass,world.context.Get(),reserve_recovery);
}
// The raster capture does not establish temporal admission. The depth capture uses
// the ABI identity recorded across the admitted original prepass call.
bool producer_capture_raster(const rsf_fallout4_cpu_identity* identity) noexcept {
    return rsf_fallout4_depth_capture_raster(identity,&world.raster_snapshot,report);
}
bool producer_capture_depth(const rsf_fallout4_cpu_identity* identity, NativeDepthSnapshot* snapshot) noexcept {
    return rsf_fallout4_depth_capture(identity,snapshot,report);
}
void producer_adopt_depth(const NativeDepthSnapshot* snapshot) noexcept { world.depth_snapshot=*snapshot; }
constexpr NativeProducerLeaves producer_leaves{read_world, producer_invalidate, producer_interrupt_depth,
    producer_refuse_raster, rsf_fallout4_resolution_native_dr, producer_screen_refusals, rsf_fallout4_render_read_owner,
    producer_configure, producer_capture_raster, producer_capture_depth, producer_adopt_depth,
    publication_report_state, rsf_fallout4_render_diagnostic};
void observe_viewport(void* manager, bool enabled) {
    Entry entry;
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    viewport(manager, enabled);
    const auto error = GetLastError();
    // Rust follows the native intent with the owned selector and runs the HUDless seam.
    rsf_fallout4_render_after_viewport(&producer_leaves,&publication_leaves,&recovery_leaves,caller,
        uintptr_t(manager),enabled);
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
        rsf_fallout4_render_begin_scope(&scope_leaves,&recovery_leaves);
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
    rsf_fallout4_render_before_dynamic_resolution(&producer_leaves);
    SetLastError(error);
    dynamic_resolution(manager,a,b,c,d);
    const auto native_error=GetLastError();
    // Rust refuses an overlapping writer and admits the selected configuration.
    rsf_fallout4_render_after_dynamic_resolution(&producer_leaves,&recovery_leaves,caller,uintptr_t(manager));
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
    // Rust records the World source, selects the call and invalidates the previous snapshots; the
    // capture after the original uses that record.
    NativePrepassCall call{};
    rsf_fallout4_render_before_prepass(&producer_leaves,&recovery_leaves,caller,prepass_depth,&call);
    SetLastError(error);
    prepass();
    const auto native_error = GetLastError();
    rsf_fallout4_render_after_prepass(&producer_leaves,&recovery_leaves,&call);
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
    // Rust selects the call and enters imagespace; the exit runs after the original returns or
    // unwinds, between its own LastError save and restore.
    const bool selected = rsf_fallout4_render_enter_imagespace(&imagespace_leaves,&recovery_leaves,caller);
    struct Cleanup {
        bool selected;
        ~Cleanup() {
            if (!selected) return;
            const auto error = GetLastError();
            rsf_fallout4_render_leave_imagespace(&imagespace_leaves,&recovery_leaves);
            SetLastError(error);
        }
    } cleanup{selected};
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
    // Rust verifies the call and arguments against the native manager read, then sequences the
    // pre-SR receipt and the SR seam before the original, and the Effects receipt, RETIRE and
    // auxiliary drain after it.
    const NativeEffectsCall call{caller,uintptr_t(manager),first,last,mode,flags};
    rsf_fallout4_cpu_identity receipt{};
    const bool admitted=rsf_fallout4_render_before_effects(
        &imagespace_leaves,&publication_leaves,&recovery_leaves,&call,&receipt);
    SetLastError(error);
    effects(manager, first, last, mode, flags);
    const auto native_error=GetLastError();
    if (admitted) rsf_fallout4_render_after_effects(&imagespace_leaves,&recovery_leaves,&receipt);
    SetLastError(native_error);
}
bool observe_taa(void* effect) {
    Entry entry;
    const auto result = taa_active(effect);
    const auto error = GetLastError();
    // Rust decides the exclusion and its once-per-combination receipt.
    const bool active = rsf_fallout4_render_gate_taa(&imagespace_leaves,&recovery_leaves,result);
    SetLastError(error);
    return active;
}
bool recovery_capture_requested() noexcept try {
    return capture_texture && capture_requested && capture_requested();
} catch (...) { return false; }

void recovery_capture(void* context, const rsf_d3d11_host_capture* capture) noexcept try {
    (void)capture_texture(host.user,context,capture);
} catch (...) {}
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
    struct PresentPhase {
        CpuIdentity owner{};
        bool active{};
        ~PresentPhase() {
            if (!active) return;
            const auto error=GetLastError();
            rsf_fallout4_cpu_end_present_phase(&owner);
            SetLastError(error);
        }
    } phase;
    const NativeRenderScopeKey leased{world.frame_pass.native_frame,world.frame_pass.view_key,
        world.frame_pass.window_key,uintptr_t(world.context.Get()),uintptr_t(world.chain.Get())};
    const auto matched=rsf_fallout4_render_match_present(caller,uintptr_t(renderer),world.submission_open,
        &leased);
    if (matched!=NativePresentMatch::unrelated) {
        const auto cpu=world.cpu;
        if (matched==NativePresentMatch::matched)
            rsf_fallout4_render_capture_recovery(&world.raster_snapshot,read_world,
                report,recovery_capture_requested,recovery_capture);
        switch (rsf_fallout4_render_admit_present(matched,host.session_id,&cpu,revalidate_present_screen)) {
        case NativePresentAdmission::complete: {
            rsf_fallout4_render_close_submission(&scope_leaves);
            // Close the same CPU phase after the original, even if it reenters.
            phase.owner=cpu;
            phase.active=rsf_fallout4_cpu_begin_present_phase(&phase.owner,leased.frame,leased.window);
            static std::atomic_flag announced = ATOMIC_FLAG_INIT;
            if (!announced.test_and_set(std::memory_order_relaxed))
                report("Fallout4 native SUBMISSION end before original outer Present; FRAME/WINDOW remain open.");
            break;
        }
        case NativePresentAdmission::refused:
            rsf_fallout4_render_diagnostic("Fallout4 pre-Present screen/refusal changed; submission completion withheld from FG.");
            break;
        case NativePresentAdmission::source_changed:
            // Missing completion refuses prepare; the old scope drains after Swap.
            world.refused=true;
            rsf_fallout4_render_diagnostic("Fallout4 pre-Present source/owner changed; submission completion withheld from FG.");
            break;
        }
    }
    SetLastError(error);
    native_present(renderer);
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
}
extern "C" void rsf_fallout4_render_report(const char* message) noexcept { report(message); }
extern "C" bool rsf_fallout4_render_create_hook(uint32_t hook, void* target) noexcept {
    return hook < detours.size() && MH_CreateHook(target, detours[hook], originals[hook]) == MH_OK;
}
extern "C" bool rsf_fallout4_render_remove_hook(uint32_t hook, void* target) noexcept {
    if (hook >= originals.size() || MH_RemoveHook(target) != MH_OK) return false;
    *originals[hook] = nullptr;
    return true;
}
// The C++ half of busy; Rust adds the resolution owner.
extern "C" bool rsf_fallout4_render_scopes_busy() noexcept {
    return entered.load(std::memory_order_acquire) || windows.load(std::memory_order_acquire) || frames.load(std::memory_order_acquire) || auxiliaries.load(std::memory_order_acquire);
}
extern "C" void rsf_fallout4_render_bind_exports(const NativeRenderExports* exports) noexcept {
    upscale_auxiliaries=reinterpret_cast<decltype(upscale_auxiliaries)>(exports->upscale_auxiliaries);
    restore_auxiliaries=reinterpret_cast<decltype(restore_auxiliaries)>(exports->restore_auxiliaries);
    reduced_frame=reinterpret_cast<decltype(reduced_frame)>(exports->reduced_frame);
    capture_texture=reinterpret_cast<decltype(capture_texture)>(exports->capture_texture);
    capture_requested=reinterpret_cast<CaptureRequested>(exports->capture_requested);
}
extern "C" void rsf_fallout4_render_bind_host(const rsf_game_host_services* services) noexcept { host = *services; }
// Rust (render_hooks/scope.rs) sequences the post-Swap scope end.
void finish_render_scope() noexcept {
    rsf_fallout4_render_finish_scope(&scope_leaves,&recovery_leaves);
}
}
