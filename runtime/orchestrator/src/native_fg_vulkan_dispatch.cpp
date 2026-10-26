// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/native_fg_dlss_vulkan.h>
#include "native_fg_ffx_vulkan.h"
#if defined(RSF_HAVE_FSR4_FG_VK)
#include "native_fg_fsr4_vulkan.h"
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
#include "native_fg_xess_vulkan.h"
#endif
#include <atomic>
#include <memory>

namespace {
std::atomic<bool> occupied{false};
constexpr uint32_t dlss_backend = 1, ffx_backend = 3, fsr4_backend = 4, xess_backend = 5;
constexpr int32_t wsi_initialization_failed = -3;
}
struct rsf_xessgvk;
struct rsf_fsr4gvk;
struct rsf_fgvk {
    uint32_t backend = 0, generated_count = 1;
    rsf_ffxgvk* ffx = nullptr;
    rsf_dlssgvk* dlss = nullptr;
    rsf_xessgvk* xess = nullptr;
    rsf_fsr4gvk* fsr4 = nullptr;
    bool claimed = false;
    ~rsf_fgvk() { if (claimed) occupied = false; }
};
namespace {
#if defined(RSF_HAVE_FSR4_FG_VK)
rsf_backend_result fsr4_result(int32_t result) {
    if (!result) return RSF_BACKEND_OK;
    if (result == RSF_FSR4FG_INVALID_ARGUMENT) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (result == RSF_FSR4FG_ABI_MISMATCH) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (result == RSF_FSR4FG_NOT_SUPPORTED) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    return RSF_BACKEND_ERROR_FEATURE_FAILED;
}
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
rsf_backend_result xess_result(int32_t result) {
    if (!result) return RSF_BACKEND_OK;
    if (result == -10001) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (result == -10002) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    return RSF_BACKEND_ERROR_FEATURE_FAILED;
}
#endif
rsf_backend_result destroy_provider(rsf_fgvk& self) {
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self.backend == fsr4_backend) {
        const auto result = fsr4_result(rsf_fsr4gvk_destroy(self.fsr4));
        if (result == RSF_BACKEND_OK) self.fsr4 = nullptr;
        return result;
    }
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self.backend == xess_backend) {
        const auto result = xess_result(rsf_xessgvk_destroy(self.xess));
        if (result == RSF_BACKEND_OK) self.xess = nullptr;
        return result;
    }
#endif
    const auto result = self.backend == dlss_backend ? rsf_dlssgvk_destroy(self.dlss) : rsf_ffxgvk_destroy(self.ffx);
    if (result == RSF_BACKEND_OK) { self.dlss = nullptr; self.ffx = nullptr; }
    return result;
}
}
extern "C" rsf_backend_result rsf_fgvk_create(const rsf_fgvk_setup* setup, rsf_fgvk** out, uint64_t* chain) {
    return rsf_fgvk_create_for_backend(setup, ffx_backend, 1, out, chain);
}
extern "C" rsf_backend_result rsf_fgvk_create_for_backend(const rsf_fgvk_setup* setup, uint32_t backend,
    uint32_t count, rsf_fgvk** out, uint64_t* chain) {
    return rsf_fgvk_create_with_host(setup, backend, count, nullptr, out, chain);
}
extern "C" rsf_backend_result rsf_fgvk_create_with_host(const rsf_fgvk_setup* setup, uint32_t backend,
    uint32_t count, const rsf_fgvk_host_options* host, rsf_fgvk** out, uint64_t* chain) try {
    if (out) *out = nullptr;
    if (chain) *chain = 0;
    if (!setup || !out || !chain || setup->struct_size < sizeof(*setup))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_FGVK_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->graphics.struct_size < sizeof(setup->graphics))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->graphics.abi_version != RSF_SRVK_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if ((backend != dlss_backend && backend != ffx_backend && backend != fsr4_backend && backend != xess_backend) ||
        ((backend == ffx_backend || backend == fsr4_backend) && count != 1)) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (host && host->struct_size < sizeof(*host)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (host && host->abi_version != RSF_FGVK_HOST_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (backend == fsr4_backend) {
#if !defined(RSF_HAVE_FSR4_FG_VK)
        return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
        if (!host || !host->window || host->timeline_enabled != 1 ||
            !setup->graphics.runtime_directory_utf8 || !setup->graphics.runtime_directory_utf8[0])
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#endif
    }
    if (backend == xess_backend) {
#if !defined(RSF_HAVE_XESS_FG_VK)
        return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
        if (!host || !host->window || host->timeline_enabled != 1 || !host->intel_runtime_directory || !host->intel_runtime_directory[0])
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#endif
    }
    auto self = std::make_unique<rsf_fgvk>(); self->backend = backend;
    if (occupied.exchange(true)) return RSF_BACKEND_ERROR_NOT_READY;
    self->claimed = true;
    uint64_t owned_chain = 0;
    rsf_backend_result created;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (backend == fsr4_backend) {
        const rsf_fsr4gvk_options options{sizeof(options), 1, host->window, host->timeline_enabled, count};
        created = fsr4_result(rsf_fsr4gvk_create(setup, &options, &self->fsr4, &owned_chain));
    } else
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (backend == xess_backend) {
        const rsf_xessgvk_options options{sizeof(options), 1, host->window, host->timeline_enabled, count, host->intel_runtime_directory};
        created = xess_result(rsf_xessgvk_create(setup, &options, &self->xess, &owned_chain));
    } else
#endif
    created = backend == dlss_backend ? rsf_dlssgvk_create(setup, &self->dlss, &owned_chain) : rsf_ffxgvk_create(setup, &self->ffx, &owned_chain);
    if (created != RSF_BACKEND_OK) {
        if (self->dlss || self->ffx || self->fsr4 || self->xess) {
            const auto retired = destroy_provider(*self);
            if (retired != RSF_BACKEND_OK) { *chain = owned_chain; *out = self.release(); return retired; }
        }
        return created;
    }
    const auto configured = rsf_fgvk_enable_options(self.get(), 0, count);
    if (configured != RSF_BACKEND_OK) {
        const auto retired = destroy_provider(*self);
        if (retired != RSF_BACKEND_OK) { *chain = owned_chain; *out = self.release(); return retired; }
        return configured;
    }
    *chain = owned_chain; *out = self.release(); return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }
extern "C" int32_t rsf_fgvk_images(rsf_fgvk* self, uint32_t* count, uint64_t* images) {
    if (!self || !count) return wsi_initialization_failed;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return rsf_fsr4gvk_images(self->fsr4, count, images);
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return rsf_xessgvk_images(self->xess, count, images);
#endif
    return self->backend == dlss_backend ? rsf_dlssgvk_images(self->dlss, count, images) : rsf_ffxgvk_images(self->ffx, count, images);
}
extern "C" int32_t rsf_fgvk_acquire(rsf_fgvk* self, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    if (!self || !index) return wsi_initialization_failed;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return rsf_fsr4gvk_acquire(self->fsr4, timeout, semaphore, fence, index);
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return rsf_xessgvk_acquire(self->xess, timeout, semaphore, fence, index);
#endif
    return self->backend == dlss_backend ? rsf_dlssgvk_acquire(self->dlss, timeout, semaphore, fence, index) :
        rsf_ffxgvk_acquire(self->ffx, timeout, semaphore, fence, index);
}
extern "C" rsf_backend_result rsf_fgvk_enable(rsf_fgvk* self, uint32_t enabled) {
    return self ? rsf_fgvk_enable_options(self, enabled, self->generated_count) : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
extern "C" rsf_backend_result rsf_fgvk_enable_options(rsf_fgvk* self, uint32_t enabled, uint32_t count) {
    if (!self || enabled > 1 || !count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if ((self->backend == ffx_backend || self->backend == fsr4_backend) && count != 1) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    rsf_backend_result result;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) result = fsr4_result(rsf_fsr4gvk_enable_options(self->fsr4, enabled, count));
    else
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) result = xess_result(rsf_xessgvk_enable_options(self->xess, enabled, count));
    else
#endif
    result = self->backend == dlss_backend ? rsf_dlssgvk_enable(self->dlss, enabled, count) : rsf_ffxgvk_enable(self->ffx, enabled);
    if (result == RSF_BACKEND_OK) self->generated_count = count;
    return result;
}
extern "C" rsf_backend_result rsf_fgvk_backend(const rsf_fgvk* self, uint32_t* backend) {
    if (!self || !backend) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *backend = self->backend; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fgvk_image_layout(const rsf_fgvk* self, uint32_t* layout) {
    if (!self || !layout) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (self->backend == dlss_backend) return rsf_dlssgvk_image_layout(self->dlss, layout);
    *layout = 5; return RSF_BACKEND_OK; // FFX/FSR4/XeSS: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL
}
extern "C" rsf_backend_result rsf_fgvk_max_generated(rsf_fgvk* self, uint32_t* count) {
    if (!self || !count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (self->backend == ffx_backend) { *count = 1; return RSF_BACKEND_OK; }
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) { *count = rsf_fsr4gvk_max_generated(self->fsr4); return RSF_BACKEND_OK; }
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) { *count = rsf_xessgvk_max_generated(self->xess); return RSF_BACKEND_OK; }
#endif
    rsf_dlssgvk_status status{}; status.struct_size = sizeof(status); status.abi_version = RSF_DLSSGVK_ABI_VERSION;
    const auto result = rsf_dlssgvk_get_status(self->dlss, &status);
    if (result == RSF_BACKEND_OK) *count = status.max_generated_frames;
    return result;
}
extern "C" rsf_backend_result rsf_fgvk_latency_options(rsf_fgvk* self, uint32_t mode, uint32_t minimum_interval_us) {
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (mode > 1) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_latency_options(self->xess, mode, minimum_interval_us));
#else
    (void)minimum_interval_us;
#endif
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
}
extern "C" rsf_backend_result rsf_fgvk_latency_sleep(rsf_fgvk* self, uint64_t frame) {
    if (!self || !frame || frame > UINT32_MAX) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_sleep(self->xess, static_cast<uint32_t>(frame)));
#endif
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
}
extern "C" rsf_backend_result rsf_fgvk_latency_marker(rsf_fgvk* self, uint64_t frame, uint32_t marker) {
    if (!self || !frame || frame > UINT32_MAX || marker > 6) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_latency(self->xess, static_cast<uint32_t>(frame), marker));
#endif
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
}
extern "C" rsf_backend_result rsf_fgvk_latency_get_status(rsf_fgvk* self, rsf_fgvk_latency_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (status->abi_version != RSF_FGVK_LATENCY_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_latency_status(self->xess, status));
#endif
    return RSF_BACKEND_ERROR_NOT_SUPPORTED;
}
extern "C" rsf_backend_result rsf_fgvk_prepare(rsf_fgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return fsr4_result(rsf_fsr4gvk_prepare(self->fsr4, commands, frame));
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_prepare(self->xess, commands, frame));
#endif
    return self->backend == dlss_backend ? rsf_dlssgvk_prepare(self->dlss, commands, frame) : rsf_ffxgvk_prepare(self->ffx, commands, frame);
}
extern "C" rsf_backend_result rsf_fgvk_hudless(rsf_fgvk* self, void* commands, const rsf_fgvk_frame* frame) {
    if (!self) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return fsr4_result(rsf_fsr4gvk_hudless(self->fsr4, commands, frame));
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_hudless(self->xess, commands, frame));
#endif
    return self->backend == dlss_backend ? rsf_dlssgvk_hudless(self->dlss, commands, frame) : rsf_ffxgvk_hudless(self->ffx, commands, frame);
}
extern "C" void rsf_fgvk_submitted(rsf_fgvk* self, void* const* commands, uint32_t count) {
    if (!self) return;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) { rsf_fsr4gvk_submitted(self->fsr4, commands, count); return; }
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) { rsf_xessgvk_submitted(self->xess, commands, count); return; }
#endif
    if (self->backend == dlss_backend) rsf_dlssgvk_submitted(self->dlss, commands, count);
    else rsf_ffxgvk_submitted(self->ffx, commands, count);
}
extern "C" int32_t rsf_fgvk_present(rsf_fgvk* self, const void* info) {
    if (!self || !info) return wsi_initialization_failed;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return rsf_fsr4gvk_present(self->fsr4, info);
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return rsf_xessgvk_present(self->xess, info);
#endif
    return self->backend == dlss_backend ? rsf_dlssgvk_present(self->dlss, info) : rsf_ffxgvk_present(self->ffx, info);
}
extern "C" rsf_backend_result rsf_fgvk_get_status(rsf_fgvk* self, rsf_fgvk_status* status) {
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (status->abi_version != RSF_FGVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (self->backend == ffx_backend) return rsf_ffxgvk_get_status(self->ffx, status);
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return fsr4_result(rsf_fsr4gvk_status(self->fsr4, status));
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_status(self->xess, status));
#endif
    rsf_dlssgvk_status dlss{}; dlss.struct_size = sizeof(dlss); dlss.abi_version = RSF_DLSSGVK_ABI_VERSION;
    const auto result = rsf_dlssgvk_get_status(self->dlss, &dlss);
    if (result == RSF_BACKEND_OK) *status = dlss.presentation;
    return result;
}
extern "C" rsf_backend_result rsf_fgvk_drain(rsf_fgvk* self) {
    if (!self) return RSF_BACKEND_OK;
#if defined(RSF_HAVE_FSR4_FG_VK)
    if (self->backend == fsr4_backend) return fsr4_result(rsf_fsr4gvk_drain(self->fsr4));
#endif
#if defined(RSF_HAVE_XESS_FG_VK)
    if (self->backend == xess_backend) return xess_result(rsf_xessgvk_drain(self->xess));
#endif
    return self->backend == dlss_backend ? rsf_dlssgvk_drain(self->dlss) : rsf_ffxgvk_drain(self->ffx);
}
extern "C" rsf_backend_result rsf_fgvk_destroy(rsf_fgvk* self) try {
    if (!self) return RSF_BACKEND_OK;
    const auto result = destroy_provider(*self);
    if (result == RSF_BACKEND_OK) delete self;
    return result;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
