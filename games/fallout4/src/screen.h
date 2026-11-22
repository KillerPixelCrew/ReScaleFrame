// SPDX-License-Identifier: MIT
#pragma once
#include "cpu_events.h"
#include <rescaleframe/game_renderer.h>
namespace rsf::fallout4 {
enum NativeScreenFact : uint32_t {
    ScreenLoading=1u, ScreenMainMenu=2u, ScreenPause=4u, ScreenPipboy=8u,
    ScreenMap=16u, ScreenVatsMenu=32u, ScreenFrozenBackground=64u,
    ScreenFrozenPause=128u, ScreenFreeCamera=256u, ScreenVatsCamera=512u
};
enum NativeScreenRefusal : uint32_t {
    ScreenQueryUnavailable=1u, ScreenSourceChanged=2u, ScreenNotOrdinaryWorld=4u,
    ScreenMainIneligible=8u, ScreenWorldLeaseInvalid=16u, ScreenCameraIneligible=32u,
    ScreenMenuOrFreeze=64u, ScreenObservationChanged=128u
};
struct NativeScreen {
    uint32_t screen{RSF_SCREEN_UNKNOWN}, facts{}, refusals{};
    uint32_t camera_index{UINT32_MAX};
    uintptr_t camera_state{};
};
// Called only in an engine-owned callback with the reader's synchronous world
// resource/camera lease. ordinary_world is the guarded parent C382A5 fact.
// This returns facts only; it neither clears NO_FG nor admits an input owner.
NativeScreen classify_native_screen(const CpuIdentity&, const rsf_game_render_pass&,
    bool ordinary_world) noexcept;
}
