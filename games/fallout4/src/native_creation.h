// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::fallout4 {
inline constexpr char executable_sha256[] =
    "fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8";
bool verify_executable();
bool preflight_creation();
bool prepare_creation(const rsf_game_host_services&);
bool start_creation();
void quiesce_creation();
bool stop_creation();
bool creation_busy();
int accept_window(void*, void*);
}
