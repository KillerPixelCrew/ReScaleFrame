// SPDX-License-Identifier: MIT
#include <rescaleframe/game_renderer.h>

// Rust region preparation and native callers use the same published predicate.
uint32_t rsf_regions_depth_domains_requested(const rsf_game_depth_domains* facts)
{
    return rsf_game_depth_domains_requested(facts);
}
