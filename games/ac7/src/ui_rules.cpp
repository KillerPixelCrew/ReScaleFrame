/* SPDX-License-Identifier: MIT */
#include <rescaleframe/ac7_ui_rules.h>
#include <rescaleframe/ui_identify.h>

extern "C" int rsf_core_ac7_ui_is_hud_producer(const void* bytecode, uint32_t bytes,
    uint32_t (*shader_hash)(const void*, uint32_t));

extern "C" int rsf_ac7_ui_is_hud_producer(const void* bytecode, uint32_t bytes)
{
    return rsf_core_ac7_ui_is_hud_producer(bytecode, bytes, rsf_ui_shader_hash);
}
