// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
struct rsf_xessvk;
rsf_backend_result rsf_xessvk_create(const rsf_srvk_setup*, rsf_xessvk**);
rsf_backend_result rsf_xessvk_plan(const rsf_xessvk*, uint32_t*, uint32_t*);
rsf_backend_result rsf_xessvk_evaluate(rsf_xessvk*, void*, const rsf_srvk_frame*);
rsf_backend_result rsf_xessvk_destroy(rsf_xessvk*);
