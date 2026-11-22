// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
#include <vector>

namespace rsf::ac7 {
// Contact-shadow and velocity transforms borrow writable storage from the C++ caller.
inline uint8_t* resize_shader_bytes(void* user, size_t size) noexcept try
{
    auto& bytes = *static_cast<std::vector<uint8_t>*>(user);
    bytes.resize(size);
    return bytes.data();
}
catch (...) { return nullptr; }
}
