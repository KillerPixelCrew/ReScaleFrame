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
#define RSF_STREAMLINE_HOST_ABI_VERSION 2u
#define RSF_SL_PROFILE_DLSS_FG 0u
#define RSF_SL_PROFILE_REFLEX 1u
#define RSF_SL_PROFILE_PCL 2u
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
    /* Cold-start feature profile. PCL excludes Reflex and DLSS-G so telemetry has no sleeper. */
    uint32_t profile;
    /* PCL profile requests no NGX feature unless shared D3D12 DLSS SR is explicitly requested. */
    uint32_t load_dlss_sr;
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
typedef struct rsf_streamline_latency_status {
    uint32_t struct_size;
    uint32_t profile;
    rsf_pacing_owner pacing_owner;
    rsf_reflex_mode reflex_mode;
    uint32_t frame_limit_us;
    uint32_t low_latency_available;
    uint32_t latency_report_available;
    uint32_t pcl_message_id;
    uint64_t sleep_calls;
    uint64_t marker_calls;
    uint64_t last_begin_id;
} rsf_streamline_latency_status;
/* Creates and immediately upgrades its device, factory and queue before any caller uses them.
   Refuses an already loaded interposer rather than starting a second SDK/device registration.
   The host outlives every provider/context/COM object created from it. */
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_create(const rsf_streamline_host_setup*, rsf_streamline_host**);
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_graphics(rsf_streamline_host*, rsf_streamline_graphics*);
/* Independent of an FG provider. The caller carries one ID from actual input through Present.
   For Reflex profiles begin sleeps once, even in Off; PCL profile only mints the shared token. */
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_begin(rsf_streamline_host*, uint64_t frame_id);
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_marker(rsf_streamline_host*, uint64_t frame_id,
    rsf_latency_marker marker, uint32_t controller_input);
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_abort(rsf_streamline_host*, uint64_t frame_id);
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_reflex(rsf_streamline_host*, rsf_reflex_mode, uint32_t frame_limit_us);
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_latency_status(rsf_streamline_host*, rsf_streamline_latency_status*);
/* Immediately after chain creation. Recognizes an already upgraded factory-created interface.
   The pointer owns one COM reference before/after; all references must precede host destruction. */
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_upgrade_chain(rsf_streamline_host*, void** idxgi_chain);
/* Call only after destroying presentation and draining/releasing all graphics objects. */
RSF_SL_HOST_API rsf_backend_result rsf_streamline_host_destroy(rsf_streamline_host*);
#ifdef __cplusplus
}
#endif
#endif
