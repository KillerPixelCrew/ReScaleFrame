// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
extern "C" bool rsf_skyrim_cpu_prepare() noexcept;
extern "C" bool rsf_skyrim_cpu_start() noexcept;
extern "C" bool rsf_skyrim_call_log(const rsf_game_host_services*, const char*) noexcept;
