// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

// Private XeSS Vulkan leaves. Rust owns the SDK session, contexts and policy.
namespace rsf::xess_vk {
struct Ops;
#if defined(RSF_HAVE_XESS_VK)
extern const Ops native_ops;
inline constexpr const Ops* platform = &native_ops;
#else
inline constexpr const Ops* platform = nullptr;
#endif
}
