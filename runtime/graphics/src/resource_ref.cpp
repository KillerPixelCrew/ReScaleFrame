// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/resource_ref.h>

#include <windows.h>

#include <unknwn.h>

extern "C" void rsf_resource_retain(void* resource)
{
    if (resource) {
        static_cast<IUnknown*>(resource)->AddRef();
    }
}

extern "C" void rsf_resource_release(void* resource)
{
    if (resource) {
        static_cast<IUnknown*>(resource)->Release();
    }
}
