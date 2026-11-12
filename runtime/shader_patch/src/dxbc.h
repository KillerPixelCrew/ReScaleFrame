// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
#include <span>
#include <string>
#include <string_view>
namespace rsf::patch {
// Initial synthetic-only operation: an exact instruction, with unchanged token count/interface.
std::string rewrite_dxbc(std::string_view,std::string_view expected_sha256,
    std::span<const uint32_t> instruction,std::span<const uint32_t> replacement);
}
