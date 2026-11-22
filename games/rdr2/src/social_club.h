// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

extern "C" bool rsf_rdr2_social_club_current_index(void* swapchain, uint32_t* index) noexcept;
extern "C" bool rsf_rdr2_social_club_install(bool (*current_index)(void*, uint32_t*), bool trace) noexcept;

inline bool rsf_rdr2_install_social_club_fix(bool trace) noexcept
{
    return rsf_rdr2_social_club_install(rsf_rdr2_social_club_current_index, trace);
}
