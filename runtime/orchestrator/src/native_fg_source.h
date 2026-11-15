// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_frame.h>
#include <cstddef>

namespace rsf {
inline bool fg_present_identity_valid(const rsf_frame_record* source) {
    return !source || (source->struct_size >= offsetof(rsf_frame_record, phase) &&
        source->abi_version == RSF_GAME_FRAME_ABI_VERSION);
}
inline bool fg_present_identity_matches(const rsf_frame_record& stored, const rsf_frame_record* source, bool explicit_source) {
    if (!explicit_source) return true;
    return source && source->frame_id && source->session_id && stored.frame_id == source->frame_id &&
        stored.session_id == source->session_id && stored.view_id == source->view_id &&
        stored.resource_generation == source->resource_generation &&
        !(stored.flags & (RSF_FRAME_FLAG_NO_FG | RSF_FRAME_FLAG_AMBIGUOUS_ID));
}
inline uint64_t fg_present_frame(uint64_t latest, const rsf_frame_record* source, bool explicit_source) {
    return explicit_source ? (source ? source->frame_id : 0) : latest;
}
}
