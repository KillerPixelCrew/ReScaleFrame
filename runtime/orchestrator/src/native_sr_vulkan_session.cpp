// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr_vulkan_session.h>

namespace {
struct Route {
    int32_t (*open)(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, void**);
    int32_t (*plan)(void*, uint32_t*, uint32_t*);
    int32_t (*record)(void*, void*, const rsf_srvk_frame*, void**);
    void (*retire)(void*, void*);
    int32_t (*close)(void*);
};
struct Routes {
    Route native, split;
    void (*preset)(void*, uint32_t);
    int32_t (*fallback)(const rsf_srvk_fallback*);
};
constexpr Routes routes{
    {
        [](const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff*, void** out) -> int32_t {
            rsf_srvk* owner = nullptr;
            const auto result = rsf_srvk_create(setup, &owner);
            *out = owner;
            return result;
        },
        [](void* owner, uint32_t* w, uint32_t* h) -> int32_t { return rsf_srvk_plan(static_cast<rsf_srvk*>(owner), w, h); },
        [](void* owner, void* commands, const rsf_srvk_frame* frame, void**) -> int32_t {
            return rsf_srvk_evaluate(static_cast<rsf_srvk*>(owner), commands, frame);
        },
        [](void* owner, void* commands) { rsf_srvk_retire_commands(static_cast<rsf_srvk*>(owner), commands); },
        [](void* owner) -> int32_t { return rsf_srvk_destroy(static_cast<rsf_srvk*>(owner)); },
    },
    {
        [](const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, void** out) -> int32_t {
            rsf_fsr4vk* owner = nullptr;
            const auto result = rsf_srvk_fsr4_create(setup, handoff, &owner);
            *out = owner;
            return result;
        },
        [](void* owner, uint32_t* w, uint32_t* h) { return rsf_srvk_fsr4_plan(static_cast<rsf_fsr4vk*>(owner), w, h); },
        [](void* owner, void* commands, const rsf_srvk_frame* frame, void** next) {
            return rsf_srvk_fsr4_evaluate(static_cast<rsf_fsr4vk*>(owner), commands, frame, next);
        },
        [](void* owner, void* commands) { rsf_srvk_fsr4_retire_commands(static_cast<rsf_fsr4vk*>(owner), commands); },
        [](void* owner) { return rsf_srvk_fsr4_destroy(static_cast<rsf_fsr4vk*>(owner)); },
    },
    [](void* owner, uint32_t preset) { (void)rsf_srvk_dlss_preset(static_cast<rsf_srvk*>(owner), preset); },
    [](const rsf_srvk_fallback* fallback) -> int32_t { return rsf_srvk_spatial_fallback(fallback); },
};
}

extern "C" {
int32_t rsf_core_srvk_session_open(const Routes*, const rsf_srvk_setup*, const rsf_fsr4vk_handoff*,
    const char*, uint32_t, rsf_srvk_session**);
int32_t rsf_core_srvk_session_plan(const rsf_srvk_session*, uint32_t*, uint32_t*);
int32_t rsf_core_srvk_session_record(const Routes*, rsf_srvk_session*, const rsf_srvk_frame*,
    const rsf_srvk_fallback*, uint32_t, int32_t*, void**);
void rsf_core_srvk_session_retire(rsf_srvk_session*, void*);
int32_t rsf_core_srvk_session_close(rsf_srvk_session*);

int32_t rsf_srvk_session_open(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff,
    const char* split_directory, uint32_t preset, rsf_srvk_session** out)
{
    return rsf_core_srvk_session_open(&routes, setup, handoff, split_directory, preset, out);
}
int32_t rsf_srvk_session_plan(const rsf_srvk_session* owner, uint32_t* w, uint32_t* h)
{
    return rsf_core_srvk_session_plan(owner, w, h);
}
int32_t rsf_srvk_session_record(rsf_srvk_session* owner, const rsf_srvk_frame* frame,
    const rsf_srvk_fallback* fallback, uint32_t split_recording, int32_t* provider, void** next)
{
    return rsf_core_srvk_session_record(&routes, owner, frame, fallback, split_recording, provider, next);
}
void rsf_srvk_session_retire(rsf_srvk_session* owner, void* commands)
{
    rsf_core_srvk_session_retire(owner, commands);
}
int32_t rsf_srvk_session_close(rsf_srvk_session* owner)
{
    return rsf_core_srvk_session_close(owner);
}
}
