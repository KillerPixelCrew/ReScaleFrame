// SPDX-License-Identifier: MIT
#include <unknwn.h>

extern "C" void rsf_com_add_ref(void* object)
{
    static_cast<IUnknown*>(object)->AddRef();
}

extern "C" void rsf_com_release(void* object)
{
    static_cast<IUnknown*>(object)->Release();
}
