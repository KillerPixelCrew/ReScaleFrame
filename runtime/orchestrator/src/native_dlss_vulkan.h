// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
struct rsf_dlssvk;
rsf_backend_result rsf_dlssvk_create(const rsf_srvk_setup*, rsf_dlssvk**);
rsf_backend_result rsf_dlssvk_plan(const rsf_dlssvk*, uint32_t*, uint32_t*);
rsf_backend_result rsf_dlssvk_evaluate(rsf_dlssvk*, void*, const rsf_srvk_frame*);
rsf_backend_result rsf_dlssvk_preset(rsf_dlssvk*, uint32_t);
rsf_backend_result rsf_dlssvk_destroy(rsf_dlssvk*);
