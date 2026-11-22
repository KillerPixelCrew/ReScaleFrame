// SPDX-License-Identifier: GPL-3.0-only
// Native access to the existing shader sources and GPL constant mapping.
#include "options.h"
#include "rcas.h"
#include "da_rcas.h"
#include "da_das.h"
#include <cstring>

extern "C" const char* rsf_sharpening_hlsl(uint32_t mode)
{
    using namespace rsf::sharpening;
    switch (static_cast<Mode>(mode)) {
    case Mode::rcas: return rcas;
    case Mode::depth_rcas: return da_rcas;
    case Mode::depth_das: return da_das;
    default: return nullptr;
    }
}

extern "C" void rsf_sharpening_pack(const void* options, const void* facts,
    const uint32_t* extent, uint32_t exposure_texture, uint32_t mode, uint32_t* packed)
{
    using namespace rsf::sharpening;
    auto frame = *static_cast<const Frame*>(facts);
    frame.width = extent[0];
    frame.height = extent[1];
    frame.motion_width = extent[2];
    frame.motion_height = extent[3];
    frame.depth_width = extent[4];
    frame.depth_height = extent[5];
    frame.exposure_texture = exposure_texture;
    const auto data = constants(*static_cast<const Options*>(options), frame, static_cast<Mode>(mode));
    std::memcpy(packed, &data, sizeof(data));
}
