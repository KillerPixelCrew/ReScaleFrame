// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_latency_vulkan.h>
#include "native_vulkan_timing.h"
#include "latency_amd_vulkan.h"
#include "latency_policy.h"
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/frame_generation.h>
#include <atomic>
#include <rescaleframe/native_streamline_vulkan.h>
#include <array>
#include <mutex>
#include <string>
#include <windows.h>

#if defined(RSF_HAVE_STREAMLINE_VK)
extern "C" {
int VulkanStorySlInitializeFeatures(const wchar_t*, const char*, uint32_t, uint32_t, uint32_t);
void* VulkanStorySlModule();
int VulkanStorySlShutdownChecked();
rsf_vk_function VulkanStorySlGetInstanceProcAddr(void*, const char*);
int VulkanStorySlBindReflex();
int VulkanStorySlBindPcl();
uint32_t VulkanStorySlPclWindowMessage();
int VulkanStorySlGetReflexState(uint32_t*, uint32_t*);
int VulkanStorySlSetReflexInterval(int, uint32_t);
int VulkanStorySlBeginFrame(uint32_t);
uintptr_t VulkanStorySlCurrentFrameToken();
int VulkanStorySlTokenMatches(uintptr_t, uint32_t);
int VulkanStorySlReflexSleep();
int VulkanStorySlMarkerForToken(uintptr_t, uint32_t);
int VulkanStorySlQueuePresentSynchronized(uintptr_t, void*, const void*);
int VulkanStorySlVulkanWsiCall(rsf_vk_wsi_call, void*);
int VulkanStorySlWithoutDlssG(rsf_vk_wsi_call, void*);
}
#endif
namespace {
std::mutex mutex;
bool initialized = false, bound = false;
uint32_t mode = 0, interval = 0;
std::atomic<rsf_fgvk*> presentation{};
std::atomic<uint32_t> active_owner{0};
std::atomic<uint64_t> facts_session{0};
bool xell_owner() {uint32_t backend{}; auto* p=presentation.load(); return p && rsf_fgvk_backend(p,&backend)==RSF_BACKEND_OK && backend==5;}
uint64_t render_frame = 0, present_frame = 0, session = 0;
struct Token { uint64_t frame = 0; uintptr_t token = 0; uint32_t stages = 0; };
// The pinned SDK retains six handles. Validate its current index before every
// use, since a new acquisition can replace an older SDK-owned handle.
std::array<Token, 6> tokens{};
rsf_latencyvk_status status{sizeof(status), RSF_LATENCYVK_ABI_VERSION};
#if defined(RSF_HAVE_STREAMLINE_VK)
bool accepted(int result) {
    status.last_result = result;
    if (result) ++status.refused_calls;
    return result == 0;
}
void marker(uint64_t frame, uint32_t kind) {
    auto& slot = tokens[frame % tokens.size()];
    if (!bound || !frame || slot.frame != frame || !VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(frame)) || (slot.stages & (1u << kind))) return;
    if (accepted(VulkanStorySlMarkerForToken(slot.token, kind))) {
        slot.stages |= 1u << kind; ++status.marker_calls;
    }
}
bool bind() {
    if (bound) return true;
    if (!accepted(VulkanStorySlBindPcl())) return false;
    status.pcl_message = VulkanStorySlPclWindowMessage();
    rsf_native_cpu_ping_message(status.pcl_message);
    status.low_latency_available=status.report_available=0;
    if (VulkanStorySlBindReflex()==0)
        (void)VulkanStorySlGetReflexState(&status.low_latency_available,&status.report_available);
    bound = true;
    return true;
}
#endif
}
extern "C" rsf_backend_result rsf_latencyvk_initialize(const rsf_latencyvk_setup* setup) try {
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_LATENCYVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const rsf_slvk_setup shared{sizeof(shared), RSF_SLVK_ABI_VERSION, setup->runtime_directory_utf8, setup->project_id, 0};
    return rsf_slvk_initialize(&shared);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_slvk_initialize(const rsf_slvk_setup* setup) try {
    if (!setup || setup->struct_size < sizeof(*setup)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SLVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->features & ~(RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!setup->runtime_directory_utf8 || !setup->project_id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(mutex);
    if (initialized) return RSF_BACKEND_ERROR_NOT_READY;
#if defined(RSF_HAVE_STREAMLINE_VK)
    const int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, nullptr, 0);
    if (!count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::wstring path(static_cast<size_t>(count), 0);
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, setup->runtime_directory_utf8, -1, path.data(), count)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!accepted(VulkanStorySlInitializeFeatures(path.c_str(), setup->project_id, 1,
        (setup->features & RSF_SLVK_DLSS_FG) ? 1u : 0u,
        (setup->features & RSF_SLVK_DLSS_SR) ? 1u : 0u))) return RSF_BACKEND_ERROR_LOAD_FAILED;
    bound = false; mode = interval = 0;
    tokens = {}; session = render_frame = present_frame = 0;
    status = {sizeof(status), RSF_LATENCYVK_ABI_VERSION};
    initialized = true;
    return RSF_BACKEND_OK;
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call* call) try {
    if (!call || call->struct_size < sizeof(*call)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (call->abi_version != RSF_SLVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!call->callback || ((call->session_id == 0) != (call->frame_id == 0))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(mutex);
    if (!initialized) return RSF_BACKEND_ERROR_NOT_READY;
#if defined(RSF_HAVE_STREAMLINE_VK)
    uintptr_t token = 0;
    if (call->frame_id) {
        const auto& slot = tokens[call->frame_id % tokens.size()];
        if (session != call->session_id || slot.frame != call->frame_id ||
            !VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(call->frame_id))) return RSF_BACKEND_ERROR_NOT_READY;
        token = slot.token;
    }
    void* module = VulkanStorySlModule();
    return module ? call->callback(module, token, call->user) : RSF_BACKEND_ERROR_NOT_READY;
#else
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int32_t rsf_slvk_without_dlssg(int32_t (*callback)(void*), void* user) {
    if (!callback) return -3;
#if defined(RSF_HAVE_STREAMLINE_VK)
    return VulkanStorySlWithoutDlssG(callback, user);
#else
    return callback(user);
#endif
}
extern "C" rsf_vk_function rsf_latencyvk_instance_proc(void* instance, const char* name) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    // Resolver remains usable after slShutdown for Vulkan object destruction.
    return name ? VulkanStorySlGetInstanceProcAddr(instance, name) : nullptr;
#else
    (void)instance; (void)name; return nullptr;
#endif
}
static uint32_t choose_owner() {
    bool reflex=false;
    {
        std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
        reflex=initialized && bind() && status.low_latency_available;
#endif
    }
    const bool amd=rsf::latency::amd_vk_capable();
    auto owner=rsf::latency::pacing_owner(xell_owner() ? RSF_PACING_XELL : RSF_PACING_NONE,reflex && !amd,amd,RSF_PACING_ANTILAG_VULKAN);
    if (owner && !rsf_native_latency_claim(2)) owner=RSF_PACING_NONE;
    active_owner.store(owner);return owner;
}
extern "C" rsf_backend_result rsf_latencyvk_options(uint32_t requested,uint32_t limit) {
    if (requested>2) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    {std::lock_guard lock(mutex);mode=requested;interval=limit;}
    const auto owner=choose_owner();
    if (owner==RSF_PACING_XELL) return rsf_fgvk_latency_options(presentation.load(),1,limit);
    return !owner && (requested || limit) ? RSF_BACKEND_ERROR_NOT_SUPPORTED : RSF_BACKEND_OK;
}
static void cpu_fact(void*,const rsf_game_cpu_event* event) {
    if (!event || event->struct_size<sizeof(*event) || !event->session_id || !event->source_frame_id) return;
    const auto id=event->source_frame_id;
    if (event->stage==RSF_GAME_CPU_INPUT_EVENT) {
        (void)rsf_latencyvk_input(id,event->input_kind,event->message_id);
        if (event->input_kind && active_owner.load()==RSF_PACING_XELL)
            (void)rsf_fgvk_latency_marker(presentation.load(),id,6);
        return;
    }
    if (event->stage==RSF_GAME_CPU_FRAME_BEGIN) {facts_session.store(event->session_id);rsf::latency::amd_vk_begin(id);}
    const bool keyed=event->event_version==RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_KEYED);
    const bool pacing=keyed ? event->stage==RSF_GAME_CPU_INPUT_SAMPLE && !(event->flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) : event->stage==RSF_GAME_CPU_PACING;
    const auto owner=pacing ? choose_owner() : active_owner.load();
    uint32_t requested{},limit{};
    {std::lock_guard lock(mutex);requested=mode;limit=interval;}
    // No presentation lock spans a driver sleep. Other frames can still report their real Present.
    if (pacing && rsf_native_latency_has_claim(2)) {
        rsf::latency::amd_vk_options(owner==RSF_PACING_ANTILAG_VULKAN ? requested : 0,owner==RSF_PACING_ANTILAG_VULKAN ? limit : 0);
        if (owner==RSF_PACING_ANTILAG_VULKAN) rsf::latency::amd_vk_input(id);
        if (owner==RSF_PACING_XELL && rsf_fgvk_latency_sleep(presentation.load(),id)==RSF_BACKEND_OK)
            (void)rsf_fgvk_latency_marker(presentation.load(),id,0);
    }
    if (owner==RSF_PACING_XELL && event->stage==RSF_GAME_CPU_SIMULATION_END)
        (void)rsf_fgvk_latency_marker(presentation.load(),id,1);
    const bool skipped=event->stage==RSF_GAME_CPU_FRAME_END &&
        ((event->flags & RSF_GAME_CPU_RENDER_SKIPPED) || (!event->render_expected && !(event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY)));
    if (skipped) rsf::latency::amd_vk_cancel(id);
    const rsf::VulkanTimingScope timing(pacing ? "cpu_pacing_total" : nullptr,id);
    std::unique_lock lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!initialized || !bind()) return;
    if (event->stage==RSF_GAME_CPU_FRAME_BEGIN) {
        if (session!=event->session_id) {tokens={};session=event->session_id;render_frame=present_frame=0;}
        auto& slot=tokens[id%tokens.size()];
        if (slot.frame==id) return;
        slot={};
        if (accepted(VulkanStorySlBeginFrame(static_cast<uint32_t>(id)))) slot={id,VulkanStorySlCurrentFrameToken(),0};
    } else if (pacing && owner) {
        auto& slot=tokens[id%tokens.size()];
        if (slot.frame==id && slot.token==VulkanStorySlCurrentFrameToken() &&
            VulkanStorySlTokenMatches(slot.token,static_cast<uint32_t>(id)) && !(slot.stages & (1u<<31))) {
            slot.stages|=1u<<31;
            lock.unlock();
            const auto applied=rsf::vk_timed_call("reflex_options",id,[&] {
                return VulkanStorySlSetReflexInterval(owner==RSF_PACING_REFLEX ? int(requested) : 0,owner==RSF_PACING_REFLEX ? limit : 0);
            });
            const auto slept=applied==0 && owner==RSF_PACING_REFLEX ? rsf::vk_timed_call("reflex_sleep",id,[] {return VulkanStorySlReflexSleep();}) : 0;
            lock.lock();
            if (!accepted(applied) || !accepted(slept)) return;
            status.mode=owner==RSF_PACING_REFLEX ? requested : 0;
            if (owner==RSF_PACING_REFLEX) ++status.sleep_calls;
        }
    }
    if (event->stage==(keyed ? RSF_GAME_CPU_INPUT_SAMPLE : RSF_GAME_CPU_SIMULATION_BEGIN)) marker(id,0);
    else if (event->stage==RSF_GAME_CPU_SIMULATION_END) marker(id,1);
    else if (skipped) tokens[id%tokens.size()]={};
#endif
}
extern "C" void rsf_latencyvk_cpu(const rsf_game_cpu_event* event) {
    if (!event) return;
    if (event->event_version==RSF_GAME_CPU_EVENT_VERSION) (void)rsf_native_cpu_dispatch(event,cpu_fact,nullptr);
    else cpu_fact(nullptr,event); // legacy compatibility, pending source-version migration
}
extern "C" rsf_backend_result rsf_latencyvk_presentation(void* context) {
    presentation.store(static_cast<rsf_fgvk*>(context)); return rsf_latencyvk_options(mode,interval);
}
extern "C" void rsf_latencyvk_submit_end(uint64_t frame) {
    if (active_owner.load()==RSF_PACING_XELL) (void)rsf_fgvk_latency_marker(presentation.load(),frame,3);
    if (active_owner.load()==RSF_PACING_ANTILAG_VULKAN) rsf::latency::amd_vk_present(frame);
}
extern "C" int rsf_latencyvk_ping(void* window) {
    std::lock_guard lock(mutex);
    return window && status.pcl_message>=0xc000 && PostMessageW(static_cast<HWND>(window),status.pcl_message,0,0);
}
extern "C" rsf_backend_result rsf_latencyvk_input(uint64_t frame, uint32_t kind, uint32_t message) {
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    auto& slot = tokens[frame % tokens.size()];
    if (!initialized || !bound || !frame || slot.frame != frame || !VulkanStorySlTokenMatches(slot.token, static_cast<uint32_t>(frame))) return RSF_BACKEND_ERROR_NOT_READY;
    if (kind & ~7u) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if ((kind & 4u) && message && !(slot.stages & (1u<<13))) {
        if (!accepted(VulkanStorySlMarkerForToken(slot.token,13))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        slot.stages|=1u<<13; ++status.marker_calls;
    }
    uint32_t marker_kind = 0;
    if (message) {
        if (!status.pcl_message || message != status.pcl_message) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        if (slot.stages & (1u<<8)) return RSF_BACKEND_OK;
        marker_kind = 8;
    } else if (kind & 4u) {
        if (slot.stages & (1u << 13)) return RSF_BACKEND_OK;
        marker_kind = 13;
    } else {
        // Keyboard/mouse timing uses real input dequeue and registered PCL
        // pings. Marker 6 is deprecated; marker 13 is controller-only.
        return kind ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (!accepted(VulkanStorySlMarkerForToken(slot.token, marker_kind))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    slot.stages |= 1u << marker_kind;
    ++status.marker_calls;
    return RSF_BACKEND_OK;
#else
    (void)frame; (void)kind; (void)message; return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
}
extern "C" void rsf_latencyvk_render(uint64_t frame) {
    if (active_owner.load()==RSF_PACING_XELL) (void)rsf_fgvk_latency_marker(presentation.load(),frame,2);
    std::lock_guard lock(mutex); render_frame = frame;
#if defined(RSF_HAVE_STREAMLINE_VK)
    marker(frame, 2);
#endif
}
extern "C" void rsf_latencyvk_present(uint32_t begin) {
    uint64_t frame{};
    {
        std::lock_guard lock(mutex);
        if (begin) {present_frame=render_frame;frame=present_frame;} else frame=present_frame;
#if defined(RSF_HAVE_STREAMLINE_VK)
        if (begin) {marker(frame,3);marker(frame,4);} else marker(frame,5);
#endif
        if (!begin) present_frame=render_frame=0;
    }
    if (begin && active_owner.load()==RSF_PACING_ANTILAG_VULKAN) rsf::latency::amd_vk_present(frame);
}
extern "C" rsf_backend_result rsf_latencyvk_get_status(rsf_latencyvk_status* output) {
    if (!output || output->struct_size < sizeof(*output)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_LATENCYVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    {
        std::lock_guard lock(mutex); *output=status;
        if (!initialized && !xell_owner() && !rsf::latency::amd_vk_capable()) return RSF_BACKEND_ERROR_NOT_READY;
        output->owner=active_owner.load();
        if (!output->owner) output->low_latency_available=output->mode=0;
    }
    if (active_owner.load()==RSF_PACING_XELL) {
        rsf_fgvk_latency_status pacing{sizeof(pacing),RSF_FGVK_LATENCY_ABI_VERSION};
        if (rsf_fgvk_latency_get_status(presentation.load(),&pacing)==RSF_BACKEND_OK) {
            output->owner=RSF_PACING_XELL;output->mode=pacing.mode;output->low_latency_available=pacing.available;
            output->sleep_calls=pacing.sleep_calls;output->marker_calls+=pacing.marker_calls;
        }
    }
    if (active_owner.load()==RSF_PACING_ANTILAG_VULKAN) {
        const auto amd=rsf::latency::amd_vk_status();output->owner=RSF_PACING_ANTILAG_VULKAN;
        output->mode=amd.enabled ? 1u : 0u;output->low_latency_available=amd.available;
        output->sleep_calls=amd.input_updates;output->marker_calls+=amd.input_updates+amd.present_updates;
    }
    return RSF_BACKEND_OK;
}
extern "C" int32_t rsf_latencyvk_queue_present(rsf_vk_function callback, void* queue, const void* info) {
    if (!callback || !queue || !info) return -3;
#if defined(RSF_HAVE_STREAMLINE_VK)
    return VulkanStorySlQueuePresentSynchronized(reinterpret_cast<uintptr_t>(callback), queue, info);
#else
    using Present = int32_t (*)(void*, const void*);
    return reinterpret_cast<Present>(callback)(queue, info);
#endif
}
extern "C" int32_t rsf_latencyvk_wsi_call(rsf_vk_wsi_call callback, void* user) {
    if (!callback || !user) return -3;
#if defined(RSF_HAVE_STREAMLINE_VK)
    return VulkanStorySlVulkanWsiCall(callback, user);
#else
    return callback(user);
#endif
}
extern "C" rsf_backend_result rsf_latencyvk_shutdown() {
    rsf::latency::amd_vk_shutdown();active_owner=0;
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (initialized && !accepted(VulkanStorySlShutdownChecked())) return RSF_BACKEND_ERROR_FEATURE_FAILED;
#endif
    rsf_native_latency_release_claim(2);
    (void)rsf_native_cpu_release_session(facts_session.exchange(0));
    presentation.store(nullptr);
    initialized = bound = false; tokens = {}; render_frame = present_frame = session = 0;
    return RSF_BACKEND_OK;
}
