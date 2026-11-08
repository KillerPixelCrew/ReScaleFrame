// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_split_vulkan.h>
#include "../../vulkan/fsr4/bridge_api.h"

/* Private service. These calls install no generic SR routing and do not extend
   SR ABI 1. Graphics backend must be 4; runtime_directory_utf8 locates the signed
   FSR4 DX12 runtime. The handoff and callbacks remain live until destroy. */
int32_t rsf_fsr4vk_create(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*, rsf_fsr4vk**);
int32_t rsf_fsr4vk_plan(const rsf_fsr4vk*, uint32_t*, uint32_t*);
/* Always returns the continuation after a successful split, even when SDK
   evaluation fails. On SDK failure record spatial fallback there, submit both
   sections, and preserve the provider failure in status. One primary view/frame. */
int32_t rsf_fsr4vk_evaluate(rsf_fsr4vk*, void* current_commands, const rsf_srvk_frame*, void** continuation_commands);
int32_t rsf_fsr4vk_info(const rsf_fsr4vk*, rsf_fsr4_bridge_info*);
/* Drain only after all recorded leased producer/continuation sections were
   submitted. Failure retains the owner, imported resources and SDK module. */
int32_t rsf_fsr4vk_destroy(rsf_fsr4vk*);
