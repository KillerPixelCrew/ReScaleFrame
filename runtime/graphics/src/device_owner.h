/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal to rsf_graphics: whether an object belongs to a device. A resource created on another
   device faults inside the driver rather than failing, so each pass checks what the caller handed
   over before it binds any of it. */
#include <d3d11.h>
#include <stdint.h>

extern "C" int32_t rsf_d3d11_device_owns(void* device, void* child);

namespace rsf {
inline bool same_device(ID3D11DeviceChild* object, ID3D11Device* device)
{
    return rsf_d3d11_device_owns(device, object) != 0;
}
}
