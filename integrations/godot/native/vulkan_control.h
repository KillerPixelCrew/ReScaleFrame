// SPDX-License-Identifier: MIT
#pragma once
#include "godot_platform.h"
namespace rsf::godot_vk {
// The Rust Vulkan session (vulkan.rs) is booted.
bool booted();
void window(void*);
void cpu(const rsf_game_cpu_event*);
void render_begin(uint64_t);
void capture_present(const void* present_info, int32_t result, uint32_t begin);

// Leaves lent to the Rust Vulkan session through rsf_godot_platform (godot_platform.h). Each
// catches its own exceptions. The entries after locked are called only inside its body.
uint32_t leaf_supported();
uint32_t leaf_latency_facts(rsf_godot_vk_latency_facts* facts);
int32_t leaf_locked(int32_t (*body)(void* user), void* user);
int32_t leaf_start(const rsf_godot_setup* setup);
int32_t leaf_stop();
void leaf_release(uint64_t session_id);
void leaf_facts(rsf_godot_vk_facts* facts);
int32_t leaf_presenter_max(uint32_t* count);
uint32_t leaf_fsr4_fg_probe();
int32_t leaf_latency_options(uint32_t mode, uint32_t frame_limit_us);
// SR context leaves for native/vulkan/sr.rs, called with the guard held.
int32_t leaf_presenter_drain(uint32_t disable);
uint32_t leaf_device_idle(void* device, rsf_godot_vk_proc device_proc);
int32_t leaf_sr_destroy(void* context);
int32_t leaf_fsr4_destroy(void* context);
uint32_t leaf_sr_create(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created);
uint32_t leaf_fsr4_create(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created);
uint32_t leaf_dlss_preset(void* context, uint32_t preset);
uint32_t leaf_sr_plan(void* call, void* context, uint32_t* width, uint32_t* height);
uint32_t leaf_fsr4_plan(void* call, void* context, uint32_t* width, uint32_t* height);
uint32_t leaf_sr_evaluate(void* call, void* context, const rsf_sharpening_options* sharpening, int32_t* result);
uint32_t leaf_fsr4_evaluate(void* call, void* context, const rsf_sharpening_options* sharpening, int32_t* result,
    void** next);
uint32_t leaf_sr_fallback(void* call, int32_t* result);
uint32_t leaf_sr_log(void* call, uint32_t kind, int32_t provider_result, int32_t fallback_result);
uint32_t leaf_sr_dump_result(void* call, uint32_t backend, int32_t result);
}
