/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_STREAMLINE_HOST_H
#define RSF_STREAMLINE_HOST_H
#include <rescaleframe/frame_generation.h>
#if defined(RSF_STREAMLINE_HOST_BUILD)
#define RSF_SL_HOST_API __declspec(dllexport)
#else
#define RSF_SL_HOST_API __declspec(dllimport)
#endif
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_streamline_host rsf_streamline_host;
typedef struct rsf_streamline_host_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    const char* runtime_directory_utf8;
    void* dxgi_adapter;
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    uint32_t require_signature;
    uint32_t development_runtime;
    rsf_backend_log_fn log;
    void* log_user;
} rsf_streamline_host_setup;
typedef struct rsf_streamline_graphics {
    uint32_t struct_size;
    /* Borrowed until host destruction. Use proxies for Streamline presentation/graphics;
       native aliases are for interoperability SDKs. They identify one physical device. */
    void* device;
    void* queue;
    void* factory;
    void* native_device;
    void* native_queue;
} rsf_streamline_graphics;
/* Creates and immediately upgrades its device, factory and queue before any caller uses them.
   Refuses an already loaded interposer rather than starting a second SDK/device registration.
   The host outlives every provider/context/COM object created from it. */
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_create(const rsf_streamline_host_setup*, rsf_streamline_host**);
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_graphics(rsf_streamline_host*, rsf_streamline_graphics*);
/* Call only after destroying presentation and draining/releasing all graphics objects. */
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_destroy(rsf_streamline_host*);
#ifdef __cplusplus
}
#endif
#endif
