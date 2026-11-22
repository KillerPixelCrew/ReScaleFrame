// SPDX-License-Identifier: MIT
#pragma once

extern "C" bool rsf_skyrim_cs_checkpoint(const char** refusal, void (*withdraw)() noexcept) noexcept;
extern "C" bool rsf_skyrim_cs_render_allowed() noexcept;
extern "C" const char* rsf_skyrim_cs_status() noexcept;
