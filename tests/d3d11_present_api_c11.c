/* SPDX-License-Identifier: MIT */
/* The public header compiled as strict C11. Behaviour is checked by presentation/api. */
#include <rescaleframe/d3d11_present_api.h>
#include <stddef.h>

_Static_assert(offsetof(rsf_d3d11_present_api, struct_size) == 0, "size prefix");
_Static_assert(offsetof(rsf_d3d11_present_api, abi_version) == sizeof(uint32_t), "version prefix");
_Static_assert(offsetof(rsf_d3d11_present_api, install) == 8, "first function offset");
