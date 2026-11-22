/* SPDX-License-Identifier: MIT */
/* Provider registration for the Rust Vulkan FG dispatcher in vulkan_fg.rs.
   The table stays native because CMake header detection decides which
   providers exist (RSF_HAVE_FSR4_FG_VK, RSF_HAVE_XESS_FG_VK) and the dispatch
   fixtures link synthetic providers under the same symbols. Each adapter only
   restores the provider's owner type. Every provider entry forwards to a Rust
   owner, whose foreign boundary aborts rather than unwinds. */
#include "native_fg_vulkan_platform.h"
#include "native_fg_ffx_vulkan.h"
#include "rust_export.h"
#include <rescaleframe/native_fg_dlss_vulkan.h>
#if defined(RSF_HAVE_FSR4_FG_VK)
#include "native_fg_fsr4_vulkan.h"
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
#include "native_fg_xess_vulkan.h"
#endif
#include <stddef.h>

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

/* Operational adapters shared by the providers whose entries take the same
   arguments. The WSI adapters return the provider's VkResult unchanged. */
#define RSF_FGVK_ADAPTERS(prefix, owner)                                                               \
    static int32_t prefix##_images(void* self, uint32_t* count, uint64_t* images)                      \
    {                                                                                                  \
        return prefix##gvk_images((owner*)self, count, images);                                        \
    }                                                                                                  \
    static int32_t prefix##_acquire(void* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, \
        uint32_t* index)                                                                               \
    {                                                                                                  \
        return prefix##gvk_acquire((owner*)self, timeout, semaphore, fence, index);                    \
    }                                                                                                  \
    static rsf_backend_result prefix##_prepare(void* self, void* commands, const rsf_fgvk_frame* frame) \
    {                                                                                                  \
        return prefix##gvk_prepare((owner*)self, commands, frame);                                     \
    }                                                                                                  \
    static rsf_backend_result prefix##_hudless(void* self, void* commands, const rsf_fgvk_frame* frame) \
    {                                                                                                  \
        return prefix##gvk_hudless((owner*)self, commands, frame);                                     \
    }                                                                                                  \
    static void prefix##_submitted(void* self, void* const* commands, uint32_t count)                  \
    {                                                                                                  \
        prefix##gvk_submitted((owner*)self, commands, count);                                          \
    }                                                                                                  \
    static int32_t prefix##_present(void* self, const void* info)                                      \
    {                                                                                                  \
        return prefix##gvk_present((owner*)self, info);                                                \
    }                                                                                                  \
    static int32_t prefix##_present_source(void* self, const void* info, const rsf_frame_record* source) \
    {                                                                                                  \
        return prefix##gvk_present_source((owner*)self, info, source);                                 \
    }                                                                                                  \
    static rsf_backend_result prefix##_drain(void* self)                                               \
    {                                                                                                  \
        return prefix##gvk_drain((owner*)self);                                                        \
    }                                                                                                  \
    static rsf_backend_result prefix##_destroy(void* self)                                             \
    {                                                                                                  \
        return prefix##gvk_destroy((owner*)self);                                                      \
    }

#define RSF_FGVK_OPERATIONS(prefix) \
    prefix##_prepare, prefix##_hudless, prefix##_submitted, prefix##_present, prefix##_present_source

RSF_FGVK_ADAPTERS(rsf_dlss, rsf_dlssgvk)
RSF_FGVK_ADAPTERS(rsf_ffx, rsf_ffxgvk)

static rsf_backend_result dlss_create(const rsf_fgvk_setup* setup, const rsf_fgvk_host_options* host,
    uint32_t count, void** out, uint64_t* chain)
{
    rsf_dlssgvk* owner = NULL;
    const rsf_backend_result result = rsf_dlssgvk_create(setup, &owner, chain);
    (void)host;
    (void)count;
    *out = owner;
    return result;
}

static rsf_backend_result dlss_enable(void* self, uint32_t enabled, uint32_t count)
{
    return rsf_dlssgvk_enable((rsf_dlssgvk*)self, enabled, count);
}

static rsf_backend_result dlss_image_layout(void* self, uint32_t* layout)
{
    return rsf_dlssgvk_image_layout((const rsf_dlssgvk*)self, layout);
}

static rsf_backend_result dlss_status(void* self, rsf_dlssgvk_status* status)
{
    status->struct_size = sizeof(*status);
    status->abi_version = RSF_DLSSGVK_ABI_VERSION;
    return rsf_dlssgvk_get_status((rsf_dlssgvk*)self, status);
}

static rsf_backend_result dlss_max_generated(void* self, uint32_t* count)
{
    rsf_dlssgvk_status status = {0};
    const rsf_backend_result result = dlss_status(self, &status);
    if (result == RSF_BACKEND_OK) *count = status.max_generated_frames;
    return result;
}

static rsf_backend_result dlss_presentation_status(void* self, rsf_fgvk_status* out)
{
    rsf_dlssgvk_status status = {0};
    const rsf_backend_result result = dlss_status(self, &status);
    if (result == RSF_BACKEND_OK) *out = status.presentation;
    return result;
}

static const rsf_fgvk_provider dlss_provider = {
    dlss_create, dlss_enable, rsf_dlss_images, rsf_dlss_acquire, dlss_image_layout, dlss_max_generated,
    RSF_FGVK_OPERATIONS(rsf_dlss), dlss_presentation_status, rsf_dlss_drain, rsf_dlss_destroy,
    NULL, NULL, NULL, NULL,
};

static rsf_backend_result ffx_create(const rsf_fgvk_setup* setup, const rsf_fgvk_host_options* host,
    uint32_t count, void** out, uint64_t* chain)
{
    rsf_ffxgvk* owner = NULL;
    const rsf_backend_result result = rsf_ffxgvk_create(setup, &owner, chain);
    (void)host;
    (void)count;
    *out = owner;
    return result;
}

/* FidelityFX 3 generates one frame, so the requested count is not forwarded. */
static rsf_backend_result ffx_enable(void* self, uint32_t enabled, uint32_t count)
{
    (void)count;
    return rsf_ffxgvk_enable((rsf_ffxgvk*)self, enabled);
}

static rsf_backend_result ffx_status(void* self, rsf_fgvk_status* status)
{
    return rsf_ffxgvk_get_status((rsf_ffxgvk*)self, status);
}

static const rsf_fgvk_provider ffx_provider = {
    ffx_create, ffx_enable, rsf_ffx_images, rsf_ffx_acquire, NULL, NULL,
    RSF_FGVK_OPERATIONS(rsf_ffx), ffx_status, rsf_ffx_drain, rsf_ffx_destroy,
    NULL, NULL, NULL, NULL,
};

#if defined(RSF_HAVE_FSR4_FG_VK)
RSF_FGVK_ADAPTERS(rsf_fsr4, rsf_fsr4gvk)

static rsf_backend_result fsr4_create(const rsf_fgvk_setup* setup, const rsf_fgvk_host_options* host,
    uint32_t count, void** out, uint64_t* chain)
{
    const rsf_fsr4gvk_options options = {sizeof(options), 1, host->window, host->timeline_enabled, count};
    rsf_fsr4gvk* owner = NULL;
    const rsf_backend_result result =
        rsf_core_fgvk_provider_result(4, rsf_fsr4gvk_create(setup, &options, &owner, chain));
    *out = owner;
    return result;
}

static rsf_backend_result fsr4_enable(void* self, uint32_t enabled, uint32_t count)
{
    return rsf_fsr4gvk_enable_options((rsf_fsr4gvk*)self, enabled, count);
}

static rsf_backend_result fsr4_max_generated(void* self, uint32_t* count)
{
    *count = rsf_fsr4gvk_max_generated((const rsf_fsr4gvk*)self);
    return RSF_BACKEND_OK;
}

static rsf_backend_result fsr4_status(void* self, rsf_fgvk_status* status)
{
    return rsf_fsr4gvk_status((rsf_fsr4gvk*)self, status);
}

static const rsf_fgvk_provider fsr4_provider = {
    fsr4_create, fsr4_enable, rsf_fsr4_images, rsf_fsr4_acquire, NULL, fsr4_max_generated,
    RSF_FGVK_OPERATIONS(rsf_fsr4), fsr4_status, rsf_fsr4_drain, rsf_fsr4_destroy,
    NULL, NULL, NULL, NULL,
};
#define RSF_FSR4_PROVIDER (&fsr4_provider)
#else
#define RSF_FSR4_PROVIDER NULL
#endif

#if defined(RSF_HAVE_XESS_FG_VK)
RSF_FGVK_ADAPTERS(rsf_xess, rsf_xessgvk)

static rsf_backend_result xess_create(const rsf_fgvk_setup* setup, const rsf_fgvk_host_options* host,
    uint32_t count, void** out, uint64_t* chain)
{
    const rsf_xessgvk_options options = {
        sizeof(options), 1, host->window, host->timeline_enabled, count, host->intel_runtime_directory,
    };
    rsf_xessgvk* owner = NULL;
    const rsf_backend_result result =
        rsf_core_fgvk_provider_result(5, rsf_xessgvk_create(setup, &options, &owner, chain));
    *out = owner;
    return result;
}

static rsf_backend_result xess_enable(void* self, uint32_t enabled, uint32_t count)
{
    return rsf_xessgvk_enable_options((rsf_xessgvk*)self, enabled, count);
}

static rsf_backend_result xess_max_generated(void* self, uint32_t* count)
{
    *count = rsf_xessgvk_max_generated((const rsf_xessgvk*)self);
    return RSF_BACKEND_OK;
}

static rsf_backend_result xess_status(void* self, rsf_fgvk_status* status)
{
    return rsf_xessgvk_status((rsf_xessgvk*)self, status);
}

static rsf_backend_result xess_latency_options(void* self, uint32_t mode, uint32_t minimum_interval_us)
{
    return rsf_xessgvk_latency_options((rsf_xessgvk*)self, mode, minimum_interval_us);
}

static rsf_backend_result xess_sleep(void* self, uint32_t frame)
{
    return rsf_xessgvk_sleep((rsf_xessgvk*)self, frame);
}

static rsf_backend_result xess_marker(void* self, uint32_t frame, uint32_t marker)
{
    return rsf_xessgvk_latency((rsf_xessgvk*)self, frame, marker);
}

static rsf_backend_result xess_latency_status(void* self, rsf_fgvk_latency_status* status)
{
    return rsf_xessgvk_latency_status((rsf_xessgvk*)self, status);
}

static const rsf_fgvk_provider xess_provider = {
    xess_create, xess_enable, rsf_xess_images, rsf_xess_acquire, NULL, xess_max_generated,
    RSF_FGVK_OPERATIONS(rsf_xess), xess_status, rsf_xess_drain, rsf_xess_destroy,
    xess_latency_options, xess_sleep, xess_marker, xess_latency_status,
};
#define RSF_XESS_PROVIDER (&xess_provider)
#else
#define RSF_XESS_PROVIDER NULL
#endif

static const rsf_fgvk_platform platform = {&dlss_provider, &ffx_provider, RSF_FSR4_PROVIDER, RSF_XESS_PROVIDER};

rsf_backend_result rsf_fgvk_create(const rsf_fgvk_setup* setup, rsf_fgvk** out, uint64_t* chain)
{
    return rsf_fgvk_create_for_backend(setup, 3, 1, out, chain);
}

rsf_backend_result rsf_fgvk_create_for_backend(const rsf_fgvk_setup* setup, uint32_t backend, uint32_t count,
    rsf_fgvk** out, uint64_t* chain)
{
    return rsf_fgvk_create_with_host(setup, backend, count, NULL, out, chain);
}

rsf_backend_result rsf_fgvk_create_with_host(const rsf_fgvk_setup* setup, uint32_t backend, uint32_t count,
    const rsf_fgvk_host_options* host, rsf_fgvk** out, uint64_t* chain)
{
    return rsf_core_fgvk_create(&platform, setup, backend, count, host, out, chain);
}
