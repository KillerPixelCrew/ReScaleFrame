/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/input_dump.h>
#include <rescaleframe/input_dump_d3d12.h>
#include <rescaleframe/input_dump_vulkan.h>

/* New integrations can consume all capture entry points as C, without C++ types or graphics headers. */
uint32_t rsf_capture_header_abi(void) { return RSF_INPUT_DUMP_ABI_VERSION; }
