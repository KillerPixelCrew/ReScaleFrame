// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

struct rsf_sampler_bias_cache;

// Private Rust entry points. The host serializes access and admits the graphics owner.
extern "C" rsf_sampler_bias_cache* rsf_sampler_bias_create();
extern "C" void rsf_sampler_bias_destroy(rsf_sampler_bias_cache*);
extern "C" int32_t rsf_sampler_bias_apply(rsf_sampler_bias_cache*, void* immediate_context,
    void* const* baselines, uint32_t slot_mask, float bias, uint32_t* applied_mask);
