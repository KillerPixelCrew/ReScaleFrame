// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::steam {
// Internal static integration helper, not a DLL ABI. The adapter verifies its
// native owner is cold; this exact Steam profile preserves its native hooks.
// Published forwarding code and services remain valid for process lifetime.
bool prepare_d3d11_chain(const rsf_game_host_services&, bool (*cold_owner)()) noexcept;
}
