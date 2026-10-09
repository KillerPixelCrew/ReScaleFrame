/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* Internal to rsf_graphics: whether an object belongs to a device. A resource created on another
   device faults inside the driver rather than failing, so each pass checks what the caller handed
   over before it binds any of it. */
#include <d3d11.h>

namespace rsf {
inline bool same_device(ID3D11DeviceChild* object, ID3D11Device* device)
{
    if (!object || !device) {
        return false;
    }
    ID3D11Device* owner = nullptr;
    object->GetDevice(&owner);
    const bool same = owner == device;
    if (owner) {
        owner->Release();
    }
    return same;
}
}
