// SPDX-License-Identifier: MIT
#include <rescaleframe/native_latency_vulkan.h>
#include "native_vulkan_timing.h"
#include "latency_amd_vulkan.h"
#include "latency_policy.h"
#include "native_latency_vulkan_presentation.h"
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/frame_generation.h>
#include <atomic>
#include <rescaleframe/native_streamline_vulkan.h>
#include <mutex>
#include <string>
#include <windows.h>

extern "C" {
void rsf_core_latencyvk_frames_reset();
void rsf_core_latencyvk_frame_boundary(uint64_t session, uint64_t frame, uint32_t skipped);
int32_t rsf_core_latencyvk_execution(const rsf_game_render_pass*, uint32_t begin);
void rsf_core_latencyvk_legacy_render(uint64_t frame);
uint64_t rsf_core_latencyvk_legacy_present(uint32_t begin);
struct rsf_vk_pcl_status {uint32_t message,available,reports;};
rsf_vk_pcl_status rsf_core_latencyvk_pcl_status();
void rsf_core_latencyvk_pcl_reset();
int32_t rsf_core_latencyvk_pcl_ping(void* window);
}

#if defined(RSF_HAVE_STREAMLINE_VK)
extern "C" {
struct rsf_vk_pcl_sdk {
    int32_t (*bind)();
    uint32_t (*message)();
    void (*reflex)(uint32_t*, uint32_t*);
    void (*report)(int32_t);
};
int32_t rsf_core_latencyvk_pcl_bind(uint32_t use_input_thread, const rsf_vk_pcl_sdk*);
void rsf_core_latencyvk_pcl_pump(const rsf_game_cpu_event*, void (*dispatch)(const rsf_game_cpu_event*));
struct rsf_vulkan_sdk_tokens {
    uintptr_t (*begin)(uint32_t);
    uintptr_t (*current)();
    int32_t (*matches)(uintptr_t, uint32_t);
    int32_t (*mark)(uintptr_t, uint32_t);
};
void rsf_core_latencyvk_token_begin(uint64_t session, uint64_t frame, const rsf_vulkan_sdk_tokens*);
uintptr_t rsf_core_latencyvk_token_lookup(uint64_t session, uint64_t frame, const rsf_vulkan_sdk_tokens*);
int32_t rsf_core_latencyvk_token_pace_begin(uint64_t session, uint64_t frame, const rsf_vulkan_sdk_tokens*);
void rsf_core_latencyvk_token_pace_end();
int32_t rsf_core_latencyvk_token_pacing();
void rsf_core_latencyvk_token_mark(uint64_t session, uint64_t frame, uint32_t kind, const rsf_vulkan_sdk_tokens*);
int32_t rsf_core_latencyvk_token_input(uint64_t session, uint64_t frame, uint32_t kind, uint32_t message, uint32_t pcl_message,
    const rsf_vulkan_sdk_tokens*);

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
bool initialized = false;
uint32_t mode = 0, interval = 0;

std::atomic<uint32_t> active_owner{0};
std::atomic<uint64_t> facts_session{0};
bool xell_owner(const rsf::latency::PresentationLease& lease) {uint32_t backend{}; auto* p=lease.get(); return p && rsf_fgvk_backend(p,&backend)==RSF_BACKEND_OK && backend==5;}
rsf_latencyvk_status status{sizeof(status), RSF_LATENCYVK_ABI_VERSION};
#if defined(RSF_HAVE_STREAMLINE_VK)
bool accepted(int result) {
    status.last_result = result;
    if (result) ++status.refused_calls;
    return result == 0;
}
constexpr rsf_vulkan_sdk_tokens sdk_tokens{
    [](uint32_t index) -> uintptr_t {
        return accepted(VulkanStorySlBeginFrame(index)) ? VulkanStorySlCurrentFrameToken() : 0;
    },
    VulkanStorySlCurrentFrameToken,
    VulkanStorySlTokenMatches,
    [](uintptr_t token, uint32_t kind) {
        const auto result = VulkanStorySlMarkerForToken(token, kind);
        if (accepted(result)) ++status.marker_calls;
        return result;
    },
};
void marker(uint64_t frame, uint32_t kind, uint64_t session = 0) {
    if (rsf_core_latencyvk_pcl_status().message) rsf_core_latencyvk_token_mark(session, frame, kind, &sdk_tokens);
}
constexpr rsf_vk_pcl_sdk pcl_sdk{
    VulkanStorySlBindPcl,
    VulkanStorySlPclWindowMessage,
    [](uint32_t* available, uint32_t* reports) {
        if (VulkanStorySlBindReflex()==0) (void)VulkanStorySlGetReflexState(available,reports);
    },
    [](int32_t result) { (void)accepted(result); },
};
bool bind(bool input_thread = false) {
    return rsf_core_latencyvk_pcl_bind(input_thread, &pcl_sdk) != 0;
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
    rsf_core_latencyvk_pcl_reset();
    mode = interval = 0;
    rsf_core_latencyvk_frames_reset();
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
        token = rsf_core_latencyvk_token_lookup(call->session_id, call->frame_id, &sdk_tokens);
        if (!token) return RSF_BACKEND_ERROR_NOT_READY;
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
static uint32_t choose_owner(const rsf::latency::PresentationLease& presentation_lease) {
    if(presentation_lease.closing()) {active_owner.store(RSF_PACING_NONE);return RSF_PACING_NONE;}
    bool reflex=false;
    {
        std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
        reflex=initialized && bind() && rsf_core_latencyvk_pcl_status().available;
#endif
    }
    const bool amd=rsf::latency::amd_vk_capable();
    auto owner=rsf::latency::pacing_owner(xell_owner(presentation_lease) ? RSF_PACING_XELL : RSF_PACING_NONE,reflex && !amd,amd,RSF_PACING_ANTILAG_VULKAN);
    if (presentation_lease.closing()) owner=RSF_PACING_NONE;
    if (owner && !rsf_native_latency_claim(2)) owner=RSF_PACING_NONE;
    active_owner.store(owner);return owner;
}
extern "C" rsf_backend_result rsf_latencyvk_options(uint32_t requested,uint32_t limit) {
    if (requested>2) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    {std::lock_guard lock(mutex);mode=requested;interval=limit;}
    rsf::latency::PresentationLease presentation_lease;
    const auto owner=choose_owner(presentation_lease);
    if (owner==RSF_PACING_XELL) return rsf_fgvk_latency_options(presentation_lease.get(),1,limit);
    return !owner && (requested || limit) ? RSF_BACKEND_ERROR_NOT_SUPPORTED : RSF_BACKEND_OK;
}
static rsf_backend_result input(uint64_t session, uint64_t frame, uint32_t kind, uint32_t message) {
#if defined(RSF_HAVE_STREAMLINE_VK)
    std::lock_guard lock(mutex);
    const auto pcl_message=rsf_core_latencyvk_pcl_status().message;
    if (!initialized || !pcl_message) return RSF_BACKEND_ERROR_NOT_READY;
    return rsf_core_latencyvk_token_input(session, frame, kind, message, pcl_message, &sdk_tokens);
#else
    (void)session; (void)frame; (void)kind; (void)message; return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#endif
}
static void cpu_fact(void*,const rsf_game_cpu_event* event) {
    if (!event || event->struct_size<sizeof(*event) || !event->session_id || !event->source_frame_id) return;
    if ((event->flags&(RSF_GAME_CPU_INPUT_SUBSTITUTE|RSF_GAME_CPU_DERIVED_BOUNDARY)) &&
        (event->stage==RSF_GAME_CPU_INPUT_SAMPLE || event->stage==RSF_GAME_CPU_SIMULATION_BEGIN ||
         event->stage==RSF_GAME_CPU_SIMULATION_END)) return;
    rsf::latency::PresentationLease presentation_lease;
    const auto id=event->source_frame_id;
#if defined(RSF_HAVE_STREAMLINE_VK)
    // native_cpu invokes this sink only after validating/accepting the boundary.
    // Register its actual thread before any pacing, token or SDK marker work.
    if (event->event_version==RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_KEYED) &&
        (event->stage==RSF_GAME_CPU_FRAME_BEGIN || event->stage==RSF_GAME_CPU_INPUT_SAMPLE)) {
        std::lock_guard lock(mutex);
        if (initialized && !bind(true)) return;
    }
#endif
    if (event->stage==RSF_GAME_CPU_INPUT_EVENT) {
        (void)input(event->session_id,id,event->input_kind,event->message_id);
        return;
    }
    if (event->stage==RSF_GAME_CPU_FRAME_BEGIN) {
        facts_session.store(event->session_id);
        rsf::latency::rsf_core_amd_vk_begin(event->session_id,id);
    }
    const bool keyed=event->event_version==RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_KEYED);
    const bool pacing=keyed ? event->stage==RSF_GAME_CPU_INPUT_SAMPLE && !(event->flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) : event->stage==RSF_GAME_CPU_PACING;
    const auto owner=pacing ? choose_owner(presentation_lease) : active_owner.load();
    uint32_t requested{},limit{};
    {std::lock_guard lock(mutex);requested=mode;limit=interval;}
    // No presentation lock spans a driver sleep. Other frames can still report their real Present.
    if (pacing && rsf_native_latency_has_claim(2)) {
        rsf::latency::rsf_core_amd_vk_options(owner==RSF_PACING_ANTILAG_VULKAN ? requested : 0,owner==RSF_PACING_ANTILAG_VULKAN ? limit : 0);
        if (owner==RSF_PACING_ANTILAG_VULKAN) rsf::latency::amd_vk_input(event->session_id,id);
        if (owner==RSF_PACING_XELL && presentation_lease.get() &&
            rsf_fgvk_latency_options(presentation_lease.get(),1,limit)==RSF_BACKEND_OK)
            (void)rsf_fgvk_latency_sleep(presentation_lease.get(),id);
    }
    if (owner==RSF_PACING_XELL && presentation_lease.get() && event->stage==RSF_GAME_CPU_SIMULATION_BEGIN)
        (void)rsf_fgvk_latency_marker(presentation_lease.get(),id,0);
    if (owner==RSF_PACING_XELL && presentation_lease.get() && event->stage==RSF_GAME_CPU_INPUT_SAMPLE)
        (void)rsf_fgvk_latency_marker(presentation_lease.get(),id,6);
    if (owner==RSF_PACING_XELL && presentation_lease.get() && event->stage==RSF_GAME_CPU_SIMULATION_END)
        (void)rsf_fgvk_latency_marker(presentation_lease.get(),id,1);
    const bool skipped=event->stage==RSF_GAME_CPU_FRAME_END &&
        ((event->flags & RSF_GAME_CPU_RENDER_SKIPPED) || (!event->render_expected && !(event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY)));
    if (skipped) rsf::latency::rsf_core_amd_vk_cancel(event->session_id,id);
    const rsf::VulkanTimingScope timing(pacing ? "cpu_pacing_total" : nullptr,id);
    std::unique_lock lock(mutex);
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN || skipped)
        rsf_core_latencyvk_frame_boundary(event->session_id, id, skipped);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (!initialized || !bind()) return;
    if (event->stage==RSF_GAME_CPU_FRAME_BEGIN) {
        rsf_core_latencyvk_token_begin(event->session_id, id, &sdk_tokens);
    } else if (pacing && owner && rsf_core_latencyvk_token_pace_begin(event->session_id, id, &sdk_tokens)) {
        lock.unlock();
        const auto applied=rsf::vk_timed_call("reflex_options",id,[&] {
            return VulkanStorySlSetReflexInterval(owner==RSF_PACING_REFLEX ? int(requested) : 0,owner==RSF_PACING_REFLEX ? limit : 0);
        });
        const auto slept=applied==0 && owner==RSF_PACING_REFLEX ? rsf::vk_timed_call("reflex_sleep",id,[] {return VulkanStorySlReflexSleep();}) : 0;
        lock.lock();
        rsf_core_latencyvk_token_pace_end();
        if (!accepted(applied) || !accepted(slept)) return;
        status.mode=owner==RSF_PACING_REFLEX ? requested : 0;
        if (owner==RSF_PACING_REFLEX) ++status.sleep_calls;
    }
    if (event->stage==RSF_GAME_CPU_SIMULATION_BEGIN) marker(id,0,event->session_id);
    else if (event->stage==RSF_GAME_CPU_SIMULATION_END) marker(id,1,event->session_id);
#endif
}
extern "C" void rsf_latencyvk_cpu(const rsf_game_cpu_event* event) {
    if (!event || event->struct_size < sizeof(*event)) return;
    if (event->event_version==RSF_GAME_CPU_EVENT_VERSION) {
        if (!rsf_native_cpu_dispatch(event,cpu_fact,nullptr)) return;
#if defined(RSF_HAVE_STREAMLINE_VK)
        rsf_core_latencyvk_pcl_pump(event, [](const rsf_game_cpu_event* fact) {
            (void)rsf_native_cpu_dispatch(fact,cpu_fact,nullptr);
        });
#endif
    } else cpu_fact(nullptr,event); // legacy compatibility, pending source-version migration
}
extern "C" rsf_backend_result rsf_latencyvk_presentation(void* context) {
    if (!context) return rsf::latency::presentation_unbind();
    const auto attached=rsf::latency::presentation_attach(static_cast<rsf_fgvk*>(context));
    if(attached!=RSF_BACKEND_OK) return attached;
    uint32_t requested{},limit{};
    {std::lock_guard lock(mutex);requested=mode;limit=interval;}
    return rsf_latencyvk_options(requested,limit);
}
extern "C" void rsf_latencyvk_submit_end(uint64_t frame) {
    rsf::latency::PresentationLease presentation_lease;
    if (active_owner.load()==RSF_PACING_XELL && presentation_lease.get()) (void)rsf_fgvk_latency_marker(presentation_lease.get(),frame,3);
    if (active_owner.load()==RSF_PACING_ANTILAG_VULKAN) rsf::latency::amd_vk_present(0,frame);
}
extern "C" int rsf_latencyvk_ping(void* window) {
    std::lock_guard lock(mutex);
    return rsf_core_latencyvk_pcl_ping(window);
}
extern "C" rsf_backend_result rsf_latencyvk_input(uint64_t frame, uint32_t kind, uint32_t message) {
    return input(0, frame, kind, message);
}
extern "C" void rsf_latencyvk_render(uint64_t frame) {
    rsf::latency::PresentationLease presentation_lease;
    if (active_owner.load()==RSF_PACING_XELL && presentation_lease.get()) (void)rsf_fgvk_latency_marker(presentation_lease.get(),frame,2);
    std::lock_guard lock(mutex); rsf_core_latencyvk_legacy_render(frame);
#if defined(RSF_HAVE_STREAMLINE_VK)
    marker(frame, 2);
#endif
}
extern "C" void rsf_latencyvk_present(uint32_t begin) {
    uint64_t frame{};
    {
        std::lock_guard lock(mutex);
        frame = rsf_core_latencyvk_legacy_present(begin);
#if defined(RSF_HAVE_STREAMLINE_VK)
        if (begin) marker(frame,4); else marker(frame,5);
#endif
    }
    if (begin && frame && active_owner.load()==RSF_PACING_ANTILAG_VULKAN) rsf::latency::amd_vk_present(0,frame);
}
extern "C" rsf_backend_result rsf_latencyvk_execution(const rsf_game_render_pass* pass, uint32_t begin) try {
    rsf::latency::PresentationLease presentation_lease;
    uint32_t kind;
    {
        std::lock_guard lock(mutex);
        const auto admitted = rsf_core_latencyvk_execution(pass, begin);
        if (admitted <= 0) return admitted;
        kind = static_cast<uint32_t>(admitted);
#if defined(RSF_HAVE_STREAMLINE_VK)
        marker(pass->source_frame_id, kind, pass->session_id);
#endif
    }
    const auto owner = active_owner.load();
    if (owner == RSF_PACING_XELL && presentation_lease.get())
        (void)rsf_fgvk_latency_marker(presentation_lease.get(), pass->source_frame_id, kind);
    if (kind == 4 && owner == RSF_PACING_ANTILAG_VULKAN)
        rsf::latency::amd_vk_present(pass->session_id,pass->source_frame_id);
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" rsf_backend_result rsf_latencyvk_get_status(rsf_latencyvk_status* output) {
    rsf::latency::PresentationLease presentation_lease;
    if (!output || output->struct_size < sizeof(*output)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_LATENCYVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    {
        std::lock_guard lock(mutex); *output=status;
        const auto pcl=rsf_core_latencyvk_pcl_status();
        output->pcl_message=pcl.message; output->low_latency_available=pcl.available; output->report_available=pcl.reports;
        if (!initialized && !xell_owner(presentation_lease) && !rsf::latency::amd_vk_capable()) return RSF_BACKEND_ERROR_NOT_READY;
        output->owner=active_owner.load();
        if (!output->owner) output->low_latency_available=output->mode=0;
    }
    if (active_owner.load()==RSF_PACING_XELL && presentation_lease.get()) {
        rsf_fgvk_latency_status pacing{sizeof(pacing),RSF_FGVK_LATENCY_ABI_VERSION};
        if (rsf_fgvk_latency_get_status(presentation_lease.get(),&pacing)==RSF_BACKEND_OK) {
            output->owner=RSF_PACING_XELL;output->mode=pacing.mode;output->low_latency_available=pacing.available;
            output->sleep_calls=pacing.sleep_calls;output->marker_calls+=pacing.marker_calls;
        }
    }
    if (active_owner.load()==RSF_PACING_ANTILAG_VULKAN) {
        const auto amd=rsf::latency::rsf_core_amd_vk_status();output->owner=RSF_PACING_ANTILAG_VULKAN;
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
    rsf::latency::PresentationShutdown presentation_shutdown;
    if(!presentation_shutdown) return RSF_BACKEND_ERROR_NOT_READY;
    rsf::latency::AmdVulkanShutdown amd_shutdown;
    if(!amd_shutdown) return RSF_BACKEND_ERROR_NOT_READY;
    std::lock_guard lock(mutex);
#if defined(RSF_HAVE_STREAMLINE_VK)
    if (rsf_core_latencyvk_token_pacing()) return RSF_BACKEND_ERROR_NOT_READY;
    if (initialized && !accepted(VulkanStorySlShutdownChecked())) return RSF_BACKEND_ERROR_FEATURE_FAILED;
#endif
    presentation_shutdown.commit();
    amd_shutdown.commit();active_owner=0;
    (void)rsf_native_cpu_release_session(facts_session.exchange(0));

    initialized = false;
    rsf_core_latencyvk_pcl_reset();
    rsf_core_latencyvk_frames_reset();
    return RSF_BACKEND_OK;
}
