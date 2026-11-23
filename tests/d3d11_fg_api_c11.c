/* SPDX-License-Identifier: MIT */
/* The public header compiled as strict C11. Behaviour is checked by generation/dll.rs. */
#include <rescaleframe/d3d11_fg_api.h>
#include <stddef.h>

_Static_assert(offsetof(rsf_d3d11_fg_api, struct_size) == 0, "size prefix");
_Static_assert(offsetof(rsf_d3d11_fg_api, abi_version) == sizeof(uint32_t), "version prefix");
_Static_assert(offsetof(rsf_d3d11_fg_api, set_log) == 8, "first function offset");

_Static_assert(sizeof(rsf_game_depth_domain) == 104, "copied depth domain layout");
_Static_assert(sizeof(rsf_game_depth_domains) == 240, "copied depth descriptor layout");
_Static_assert(offsetof(rsf_game_render_pass, depth_domains) == offsetof(rsf_game_render_pass, temporal_mask_weights) + 16,
    "append-only depth facts follow ABI22 tail");

_Static_assert(RSF_D3D11_FG_INPUTS_VERSION == 1, "FG input version");
_Static_assert(offsetof(rsf_d3d11_fg_inputs, struct_size) == 0, "FG input size prefix");
_Static_assert(offsetof(rsf_d3d11_fg_inputs, abi_version) == 4, "FG input version prefix");
_Static_assert(sizeof(rsf_d3d11_fg_inputs) == 24, "x64 FG input layout");
