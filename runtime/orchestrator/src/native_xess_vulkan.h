// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_xess_vulkan_requirements.h>

/* Private XeSS Vulkan SR platform. Rust owns the SDK session, contexts, policy and
   the production table (vulkan_sr/xess/native.rs). The table layout is private to
   Rust; C callers only select it and pass it back. */
#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_xessvk_ops rsf_xessvk_ops;

#if defined(RSF_HAVE_XESS_VK)
extern const rsf_xessvk_ops rsf_xessvk_native_ops;
#define RSF_XESSVK_NATIVE_PLATFORM (&rsf_xessvk_native_ops)
#else
#define RSF_XESSVK_NATIVE_PLATFORM ((const rsf_xessvk_ops*)0)
#endif

#ifdef __cplusplus
}
#endif
