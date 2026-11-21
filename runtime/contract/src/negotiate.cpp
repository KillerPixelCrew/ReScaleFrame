// SPDX-License-Identifier: MIT
#include <rescaleframe/backend_registry.h>
#include <stddef.h>

static_assert(sizeof(rsf_negotiate_request) == 44 && offsetof(rsf_negotiate_request, abi_version) == 4);
static_assert(sizeof(rsf_backend_caps) == 80 && offsetof(rsf_backend_caps, refusal_utf8) == 72);
static_assert(sizeof(rsf_backend_choice) == 36 && offsetof(rsf_backend_choice, reasons) == 32);

extern "C" rsf_backend_result rsf_contract_negotiate(const rsf_negotiate_request*,
    const rsf_backend_caps*, uint32_t caps_count, rsf_backend_choice*) noexcept;

extern "C" rsf_backend_result rsf_negotiate(const rsf_negotiate_request* request,
    const rsf_backend_caps* caps, uint32_t caps_count, rsf_backend_choice* choice)
{
    return rsf_contract_negotiate(request, caps, caps_count, choice);
}
