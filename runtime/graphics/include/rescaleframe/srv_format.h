/* SPDX-License-Identifier: MIT */
#pragma once
/* The format a shader-resource view of a texture has to be created with. Depth formats map to their
   readable plane (the typed D* aliases and the typeless R* resources alike); typeless colour maps to
   the typed format the passes sample as (FLOAT for the 16/32-bit families, UNORM for 8888 and 1010102,
   never sRGB, which is a decision the creator of the texture makes). Everything else is already
   typed and is returned unchanged. Pairs with is_depth_format() in resource_roles.h.

   Sharing is a separate question: R32G32_FLOAT is a valid view here but is not D3D11-shareable. */
#include <dxgiformat.h>

extern "C" unsigned int rsf_d3d11_output_srv_format(unsigned int format);

namespace rsf {
inline DXGI_FORMAT srv_format(DXGI_FORMAT format)
{
    return static_cast<DXGI_FORMAT>(rsf_d3d11_output_srv_format(static_cast<unsigned int>(format)));
}
}
