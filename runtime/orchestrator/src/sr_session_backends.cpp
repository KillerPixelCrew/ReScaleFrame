// SPDX-License-Identifier: MIT
#include <rescaleframe/sr_session.h>
#include "../../backends/common/sr_helpers.h"

extern "C" rsf_backend_result rsf_core_sr_session_create(const rsf_sr_session_setup*, rsf_sr_session**,
    const rsf_sr_provider* fsr, const rsf_sr_provider* xess, decltype(&rsf::validate_frame) validate);

// Wire the existing provider tables and shared frame validator into the Rust owner.
extern "C" rsf_backend_result rsf_sr_session_create(const rsf_sr_session_setup* setup, rsf_sr_session** out)
{
    return rsf_core_sr_session_create(setup, out, rsf_fsr_sr_provider(), rsf_xess_sr_provider(), rsf::validate_frame);
}
