// SPDX-License-Identifier: MIT
#include "native_fg_vulkan_platform.h"
#include "native_fg_ffx_vulkan.h"
#include "native_fg_source.h"
#include "rust_export.h"
#include <rescaleframe/native_fg_dlss_vulkan.h>
#if defined(RSF_HAVE_FSR4_FG_VK)
#include "native_fg_fsr4_vulkan.h"
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
#include "native_fg_xess_vulkan.h"
#endif
#include <type_traits>

RSF_EXPORT(rsf_fgvk_images)
RSF_EXPORT(rsf_fgvk_acquire)
RSF_EXPORT(rsf_fgvk_enable)
RSF_EXPORT(rsf_fgvk_enable_options)
RSF_EXPORT(rsf_fgvk_backend)
RSF_EXPORT(rsf_fgvk_image_layout)
RSF_EXPORT(rsf_fgvk_max_generated)
RSF_EXPORT(rsf_fgvk_latency_options)
RSF_EXPORT(rsf_fgvk_latency_sleep)
RSF_EXPORT(rsf_fgvk_latency_marker)
RSF_EXPORT(rsf_fgvk_latency_get_status)
RSF_EXPORT(rsf_fgvk_prepare)
RSF_EXPORT(rsf_fgvk_hudless)
RSF_EXPORT(rsf_fgvk_submitted)
RSF_EXPORT(rsf_fgvk_present)
RSF_EXPORT(rsf_fgvk_present_source)
RSF_EXPORT(rsf_fgvk_get_status)
RSF_EXPORT(rsf_fgvk_drain)
RSF_EXPORT(rsf_fgvk_destroy)

namespace {
constexpr int32_t wsi_initialization_failed = -3;
constexpr uint32_t shader_read_layout = 5; // VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL
constexpr int32_t unchanged(int32_t result) { return result; }

// Adapt the typed provider pointer without reinterpreting any function pointer.
template <auto Function, auto Map = unchanged, int32_t Failure = RSF_BACKEND_ERROR_FEATURE_FAILED>
struct Call;

template <typename Owner, typename Result, typename... Args, Result (*Function)(Owner*, Args...),
    auto Map, int32_t Failure>
struct Call<Function, Map, Failure> {
    static Result invoke(void* owner, Args... args) noexcept try {
        if constexpr (std::is_void_v<Result>) Function(static_cast<Owner*>(owner), args...);
        else return Map(Function(static_cast<Owner*>(owner), args...));
    } catch (...) {
        if constexpr (!std::is_void_v<Result>) return Failure;
    }
};

template <auto Function>
constexpr auto wsi_call = Call<Function, unchanged, wsi_initialization_failed>::invoke;

template <auto Function>
int32_t present_source(void* owner, const void* info, const rsf_frame_record* identity) {
    if (!rsf::fg_present_identity_valid(identity)) return wsi_initialization_failed;
    return wsi_call<Function>(owner, info, identity);
}

template <typename Owner, uint32_t (*Max)(const Owner*)>
rsf_backend_result max_generated(void* owner, uint32_t* count) noexcept try {
    *count = Max(static_cast<Owner*>(owner));
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

template <typename Owner, typename Create>
rsf_backend_result create_provider(void** out, Create create) noexcept {
    Owner* owner = nullptr;
    auto result = RSF_BACKEND_ERROR_INIT_FAILED;
    try { result = create(&owner); } catch (...) {}
    *out = owner;
    return result;
}

rsf_backend_result shader_read(void*, uint32_t* layout) {
    *layout = shader_read_layout;
    return RSF_BACKEND_OK;
}

rsf_backend_result dlss_status(rsf_dlssgvk* owner, rsf_dlssgvk_status& status) noexcept try {
    status.struct_size = sizeof(status);
    status.abi_version = RSF_DLSSGVK_ABI_VERSION;
    return rsf_dlssgvk_get_status(owner, &status);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

constexpr rsf_fgvk_provider dlss_provider{
    [](const rsf_fgvk_setup* setup, const rsf_fgvk_host_options*, uint32_t, void** out, uint64_t* chain) {
        return create_provider<rsf_dlssgvk>(out, [&](auto** owner) { return rsf_dlssgvk_create(setup, owner, chain); });
    },
    Call<rsf_dlssgvk_enable>::invoke, wsi_call<rsf_dlssgvk_images>, wsi_call<rsf_dlssgvk_acquire>,
    Call<rsf_dlssgvk_image_layout>::invoke,
    [](void* owner, uint32_t* count) {
        rsf_dlssgvk_status status{};
        const auto result = dlss_status(static_cast<rsf_dlssgvk*>(owner), status);
        if (result == RSF_BACKEND_OK) *count = status.max_generated_frames;
        return result;
    },
    Call<rsf_dlssgvk_prepare>::invoke, Call<rsf_dlssgvk_hudless>::invoke, Call<rsf_dlssgvk_submitted>::invoke,
    wsi_call<rsf_dlssgvk_present>, present_source<rsf_dlssgvk_present_source>,
    [](void* owner, rsf_fgvk_status* out) {
        rsf_dlssgvk_status status{};
        const auto result = dlss_status(static_cast<rsf_dlssgvk*>(owner), status);
        if (result == RSF_BACKEND_OK) *out = status.presentation;
        return result;
    },
    Call<rsf_dlssgvk_drain>::invoke, Call<rsf_dlssgvk_destroy>::invoke,
    nullptr, nullptr, nullptr, nullptr,
};

constexpr rsf_fgvk_provider ffx_provider{
    [](const rsf_fgvk_setup* setup, const rsf_fgvk_host_options*, uint32_t, void** out, uint64_t* chain) {
        return create_provider<rsf_ffxgvk>(out, [&](auto** owner) { return rsf_ffxgvk_create(setup, owner, chain); });
    },
    [](void* owner, uint32_t enabled, uint32_t) { return Call<rsf_ffxgvk_enable>::invoke(owner, enabled); },
    wsi_call<rsf_ffxgvk_images>, wsi_call<rsf_ffxgvk_acquire>, shader_read,
    [](void*, uint32_t* count) { *count = 1; return RSF_BACKEND_OK; },
    Call<rsf_ffxgvk_prepare>::invoke, Call<rsf_ffxgvk_hudless>::invoke, Call<rsf_ffxgvk_submitted>::invoke,
    wsi_call<rsf_ffxgvk_present>, present_source<rsf_ffxgvk_present_source>, Call<rsf_ffxgvk_get_status>::invoke,
    Call<rsf_ffxgvk_drain>::invoke, Call<rsf_ffxgvk_destroy>::invoke,
    nullptr, nullptr, nullptr, nullptr,
};

#if defined(RSF_HAVE_FSR4_FG_VK)
rsf_backend_result fsr4_result(int32_t result) {
    switch (result) {
    case 0: return RSF_BACKEND_OK;
    case RSF_FSR4FG_INVALID_ARGUMENT: return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    case RSF_FSR4FG_ABI_MISMATCH: return RSF_BACKEND_ERROR_ABI_MISMATCH;
    case RSF_FSR4FG_NOT_SUPPORTED: return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    default: return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
}
constexpr rsf_fgvk_provider fsr4_provider{
    [](const rsf_fgvk_setup* setup, const rsf_fgvk_host_options* host, uint32_t count, void** out, uint64_t* chain) {
        const rsf_fsr4gvk_options options{sizeof(options), 1, host->window, host->timeline_enabled, count};
        return create_provider<rsf_fsr4gvk>(out, [&](auto** owner) { return fsr4_result(rsf_fsr4gvk_create(setup, &options, owner, chain)); });
    },
    Call<rsf_fsr4gvk_enable_options, fsr4_result>::invoke, wsi_call<rsf_fsr4gvk_images>, wsi_call<rsf_fsr4gvk_acquire>, shader_read,
    max_generated<rsf_fsr4gvk, rsf_fsr4gvk_max_generated>,
    Call<rsf_fsr4gvk_prepare, fsr4_result>::invoke, Call<rsf_fsr4gvk_hudless, fsr4_result>::invoke, Call<rsf_fsr4gvk_submitted>::invoke,
    wsi_call<rsf_fsr4gvk_present>, present_source<rsf_fsr4gvk_present_source>, Call<rsf_fsr4gvk_status, fsr4_result>::invoke,
    Call<rsf_fsr4gvk_drain, fsr4_result>::invoke, Call<rsf_fsr4gvk_destroy, fsr4_result>::invoke,
    nullptr, nullptr, nullptr, nullptr,
};
#endif

#if defined(RSF_HAVE_XESS_FG_VK)
rsf_backend_result xess_result(int32_t result) {
    switch (result) {
    case 0: return RSF_BACKEND_OK;
    case -10001: return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    case -10002: return RSF_BACKEND_ERROR_ABI_MISMATCH;
    default: return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
}
constexpr rsf_fgvk_provider xess_provider{
    [](const rsf_fgvk_setup* setup, const rsf_fgvk_host_options* host, uint32_t count, void** out, uint64_t* chain) {
        const rsf_xessgvk_options options{sizeof(options), 1, host->window, host->timeline_enabled, count, host->intel_runtime_directory};
        return create_provider<rsf_xessgvk>(out, [&](auto** owner) { return xess_result(rsf_xessgvk_create(setup, &options, owner, chain)); });
    },
    Call<rsf_xessgvk_enable_options, xess_result>::invoke, wsi_call<rsf_xessgvk_images>, wsi_call<rsf_xessgvk_acquire>, shader_read,
    max_generated<rsf_xessgvk, rsf_xessgvk_max_generated>,
    Call<rsf_xessgvk_prepare, xess_result>::invoke, Call<rsf_xessgvk_hudless, xess_result>::invoke, Call<rsf_xessgvk_submitted>::invoke,
    wsi_call<rsf_xessgvk_present>, present_source<rsf_xessgvk_present_source>, Call<rsf_xessgvk_status, xess_result>::invoke,
    Call<rsf_xessgvk_drain, xess_result>::invoke, Call<rsf_xessgvk_destroy, xess_result>::invoke,
    Call<rsf_xessgvk_latency_options, xess_result>::invoke, Call<rsf_xessgvk_sleep, xess_result>::invoke,
    Call<rsf_xessgvk_latency, xess_result>::invoke, Call<rsf_xessgvk_latency_status, xess_result>::invoke,
};
#endif

constexpr rsf_fgvk_platform platform{
    {nullptr, &dlss_provider, nullptr, &ffx_provider,
#if defined(RSF_HAVE_FSR4_FG_VK)
        &fsr4_provider,
#else
        nullptr,
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
        &xess_provider,
#else
        nullptr,
#endif
    },
};
}

extern "C" rsf_backend_result rsf_fgvk_create(const rsf_fgvk_setup* setup, rsf_fgvk** out, uint64_t* chain) {
    return rsf_fgvk_create_for_backend(setup, 3, 1, out, chain);
}

extern "C" rsf_backend_result rsf_fgvk_create_for_backend(const rsf_fgvk_setup* setup, uint32_t backend,
    uint32_t count, rsf_fgvk** out, uint64_t* chain) {
    return rsf_fgvk_create_with_host(setup, backend, count, nullptr, out, chain);
}

extern "C" rsf_backend_result rsf_fgvk_create_with_host(const rsf_fgvk_setup* setup, uint32_t backend,
    uint32_t count, const rsf_fgvk_host_options* host, rsf_fgvk** out, uint64_t* chain) {
    return rsf_core_fgvk_create(&platform, setup, backend, count, host, out, chain);
}
