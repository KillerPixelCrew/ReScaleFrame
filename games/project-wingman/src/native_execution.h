// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>

namespace rsf::wingman {
bool execution_prepare(const rsf_game_host_services& host) noexcept;
bool execution_start() noexcept;
void execution_quiesce() noexcept;
bool execution_stop() noexcept;
}
