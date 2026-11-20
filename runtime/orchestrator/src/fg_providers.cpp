// SPDX-License-Identifier: MIT
// Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
// work (see docs/representation-plan.md). Do not delete as dead code.
#include <rescaleframe/fg_session.h>
// The presentation bridge's table, keyed by the single backend mapping in d3d11_present.rs.
extern "C" const rsf_generation_provider* rsf_d3d11p_provider_for(uint32_t backend);
extern "C" const rsf_generation_provider* rsf_fg_get_provider(uint32_t backend)
{
    return rsf_d3d11p_provider_for(backend);
}
