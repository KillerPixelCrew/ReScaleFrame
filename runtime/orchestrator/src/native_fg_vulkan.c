/* SPDX-License-Identifier: MIT */
/* FidelityFX 3 Vulkan frame-generation provider entries. The owner and the
   SDK leaf are Rust (vulkan_ffx_fg.rs and vulkan_ffx_fg/sdk.rs). This file keeps
   three native duties: the RSF_HAVE_FFX_VK compile gate that CMake sets from
   header detection, provider symbols that the dispatch fixtures replace at link
   time, and containment of C++ exceptions thrown by host Vulkan entry points.
   MSVC raises a C++ exception as SEH code 0xE06D7363, so the filter stops what
   the former catch-all stopped under /EHsc. Other faults still propagate. */
#include "native_fg_ffx_vulkan.h"
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL

#if defined(_MSC_VER)
#define RSF_FOREIGN_TRY __try
#define RSF_FOREIGN_CATCH(on_exception) \
    __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER \
                                                           : EXCEPTION_CONTINUE_SEARCH) { on_exception; }
#else
/* Without structured exception handling a foreign exception terminates. */
#define RSF_FOREIGN_TRY
#define RSF_FOREIGN_CATCH(on_exception)
#endif

/* vulkan_ffx_fg/sdk.rs asserts the same eight-pointer Host layout. */
_Static_assert(sizeof(rsf_ffxgvk_host) == 8 * sizeof(void*), "rsf_ffxgvk_host layout");

typedef void (*rsf_vk_memory_query)(void* physical_device, void* properties);

int32_t rsf_ffxgvk_guard_resolve(rsf_vk_get_proc resolve, void* object, const char* name, rsf_vk_function* out)
{
    RSF_FOREIGN_TRY {
        *out = resolve(object, name);
        return RSF_BACKEND_OK;
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_INIT_FAILED)
}

int32_t rsf_ffxgvk_guard_memory(rsf_vk_function query, void* physical_device, void* properties)
{
    RSF_FOREIGN_TRY {
        /* Admission resolved this entry by the name vkGetPhysicalDeviceMemoryProperties. */
        ((rsf_vk_memory_query)query)(physical_device, properties);
        return RSF_BACKEND_OK;
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_INIT_FAILED)
}

#if defined(RSF_HAVE_FFX_VK)
static const rsf_ffxgvk_host host = {
    rsf_core_vk_sdk_load, rsf_core_vk_sdk_unload, rsf_vkffx_symbol,
    rsf_ffxgvk_guard_resolve, rsf_ffxgvk_guard_memory,
    rsf_slvk_without_dlssg, rsf_latencyvk_wsi_call, rsf_latencyvk_queue_present,
};
#define RSF_FFXGVK_HOST (&host)
#else
/* Disabled build: Rust validates the arguments and reports NOT_COMPILED, so no
   owner reaches the other entries. */
#define RSF_FFXGVK_HOST NULL
#endif

rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup* setup, rsf_ffxgvk** out, uint64_t* chain)
{
    return rsf_core_ffxgvk_create_sdk(RSF_FFXGVK_HOST, setup, out, chain);
}

int32_t rsf_ffxgvk_images(rsf_ffxgvk* self, uint32_t* count, uint64_t* images)
{
    return rsf_core_ffxgvk_images(self, count, images);
}

int32_t rsf_ffxgvk_acquire(rsf_ffxgvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index)
{
    return rsf_core_ffxgvk_acquire(self, timeout, semaphore, fence, index);
}

rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk* self, uint32_t enabled)
{
    return rsf_core_ffxgvk_enable(self, enabled);
}

rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame)
{
    return rsf_core_ffxgvk_prepare(self, commands, frame);
}

rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk* self, void* commands, const rsf_fgvk_frame* frame)
{
    return rsf_core_ffxgvk_hudless(self, commands, frame);
}

void rsf_ffxgvk_submitted(rsf_ffxgvk* self, void* const* commands, uint32_t count)
{
    rsf_core_ffxgvk_submitted(self, commands, count);
}

int32_t rsf_ffxgvk_present(rsf_ffxgvk* self, const void* info)
{
    return rsf_core_ffxgvk_present_native(self, info, NULL, 0);
}

int32_t rsf_ffxgvk_present_source(rsf_ffxgvk* self, const void* info, const rsf_frame_record* source)
{
    return rsf_core_ffxgvk_present_native(self, info, source, 1);
}

rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk* self, rsf_fgvk_status* status)
{
    return rsf_core_ffxgvk_status(self, status);
}

rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk* self)
{
    return rsf_core_ffxgvk_drain(self);
}

rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk* self)
{
    return rsf_core_ffxgvk_destroy(self);
}
