// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
namespace rsf::skyrim {
// Scope generic inner-chain creation at the guarded game call; preserve outer wrappers.
// The Rust crate rsf-skyrim-se owns this boundary and the cold-only Steam Present forwarding;
// native_creation.cpp lends it the native leaves and keeps these entry points.
bool native_creation_prepare(const rsf_game_host_services&);
bool native_creation_start();
void native_creation_quiesce();
bool native_creation_stop();
bool native_creation_busy();
}
