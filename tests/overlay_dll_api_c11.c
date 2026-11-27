/* SPDX-License-Identifier: MIT */
/* The public header compiled as strict C11. Behaviour is checked by overlay-service. */
#include <rescaleframe/overlay_api.h>
#include <stddef.h>

_Static_assert(offsetof(rsf_overlay_api, struct_size) == 0, "size prefix");
_Static_assert(offsetof(rsf_overlay_api, abi_version) == sizeof(uint32_t), "version prefix");
_Static_assert(offsetof(rsf_overlay_api, configure) == 8, "first function offset");
_Static_assert(offsetof(rsf_overlay_service_setup, abi_version) == sizeof(uint32_t), "setup version prefix");
_Static_assert(offsetof(rsf_overlay_capture, struct_size) == 0, "capture size prefix");
