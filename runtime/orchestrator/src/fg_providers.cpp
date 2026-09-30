// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/fg_session.h>
extern "C" const rsf_generation_provider* rsf_fg_get_provider(uint32_t backend)
{
    switch (backend) {
    case RSF_FG_BACKEND_DLSS: return rsf_generation_dlss();
    case RSF_FG_BACKEND_FSR3:
    case RSF_FG_BACKEND_FSR4: return rsf_generation_fsr();
    case RSF_FG_BACKEND_XESS: return rsf_generation_xess();
    default: return nullptr;
    }
}
