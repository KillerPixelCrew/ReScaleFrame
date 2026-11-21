// SPDX-License-Identifier: MIT
#pragma once
// Private boundary for the ordinary World and outer Present guards.
#include "cpu_events_platform.h"
#include <cstddef>

namespace rsf::fallout4 {
struct NativeWorldAdmission {
    uintptr_t caller;
    uint64_t session;
    bool live, ordinary_world, frame_open, auxiliary_open, recovery_owned;
};

struct NativeRenderScopeKey {
    uint64_t frame, view, window;
    uintptr_t context, chain;
};

enum class NativePresentMatch : uint32_t { unrelated = 0, owner_changed = 1, matched = 2 };
enum class NativePresentAdmission : uint32_t { source_changed = 0, refused = 1, complete = 2 };

using ReadPresentOwner = bool (*)(NativeRenderScopeKey*, uintptr_t* outer_chain) noexcept;
using RevalidatePresentScreen = bool (*)() noexcept;

static_assert(sizeof(NativeWorldAdmission) == 24 && offsetof(NativeWorldAdmission, live) == 16 &&
    offsetof(NativeWorldAdmission, recovery_owned) == 20);
static_assert(sizeof(NativeRenderScopeKey) == 40 && offsetof(NativeRenderScopeKey, context) == 24 &&
    offsetof(NativeRenderScopeKey, chain) == 32);

extern "C" {
bool rsf_fallout4_render_admit_world(const NativeWorldAdmission*, const rsf_fallout4_cpu_identity*) noexcept;
NativePresentMatch rsf_fallout4_render_match_present(uintptr_t caller, uintptr_t renderer,
    bool submission_open, const NativeRenderScopeKey*, ReadPresentOwner) noexcept;
NativePresentAdmission rsf_fallout4_render_admit_present(NativePresentMatch, uint64_t session,
    const rsf_fallout4_cpu_identity*, RevalidatePresentScreen) noexcept;
}
}
