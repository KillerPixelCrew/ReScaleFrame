/* SPDX-License-Identifier: MIT */
/* The Runtime DLL's exports of public Vulkan entry points that Rust defines (rsf_native_vulkan is
   linked whole, so these directives always reach the DLL). */
#include "rust_export.h"

/* vulkan_latency.rs */
RSF_EXPORT(rsf_latencyvk_initialize)
RSF_EXPORT(rsf_latencyvk_instance_proc)
RSF_EXPORT(rsf_latencyvk_options)
RSF_EXPORT(rsf_latencyvk_presentation)
RSF_EXPORT(rsf_latencyvk_submit_end)
RSF_EXPORT(rsf_latencyvk_ping)
RSF_EXPORT(rsf_latencyvk_cpu)
RSF_EXPORT(rsf_latencyvk_input)
RSF_EXPORT(rsf_latencyvk_render)
RSF_EXPORT(rsf_latencyvk_present)
RSF_EXPORT(rsf_latencyvk_execution)
RSF_EXPORT(rsf_latencyvk_queue_present)
RSF_EXPORT(rsf_latencyvk_wsi_call)
RSF_EXPORT(rsf_latencyvk_get_status)
RSF_EXPORT(rsf_latencyvk_shutdown)
RSF_EXPORT(rsf_latencyvk_cpu_service)
RSF_EXPORT(rsf_latencyvk_require_device)
RSF_EXPORT(rsf_latencyvk_device_created)
RSF_EXPORT(rsf_latencyvk_device_destroyed)
RSF_EXPORT(rsf_slvk_initialize)
RSF_EXPORT(rsf_slvk_invoke)
RSF_EXPORT(rsf_slvk_without_dlssg)

/* vulkan_sr.rs */
RSF_EXPORT(rsf_srvk_create)
RSF_EXPORT(rsf_srvk_plan)
RSF_EXPORT(rsf_srvk_evaluate)
RSF_EXPORT(rsf_srvk_retire_commands)
RSF_EXPORT(rsf_srvk_destroy)
RSF_EXPORT(rsf_srvk_dlss_preset)
RSF_EXPORT(rsf_srvk_spatial_fallback)

/* vulkan_sr_split.rs */
RSF_EXPORT(rsf_srvk_fsr4_create)
RSF_EXPORT(rsf_srvk_fsr4_plan)
RSF_EXPORT(rsf_srvk_fsr4_evaluate)
RSF_EXPORT(rsf_srvk_fsr4_destroy)
RSF_EXPORT(rsf_srvk_fsr4_retire_commands)
RSF_EXPORT(rsf_srvk_fsr4_submitted)
RSF_EXPORT(rsf_srvk_fsr4_info)

/* vulkan_sr/xess.rs */
RSF_EXPORT(rsf_xessvk_initialize)
RSF_EXPORT(rsf_xessvk_instance_requirements)
RSF_EXPORT(rsf_xessvk_device_requirements)
RSF_EXPORT(rsf_xessvk_shutdown)