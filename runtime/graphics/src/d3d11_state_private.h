/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_state.h>

/* Take a full snapshot even inside another scope on the same context. */
uint32_t rsf_d3d11_state_save_independent(void* context, rsf_d3d11_state* storage);
