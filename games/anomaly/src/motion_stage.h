// SPDX-License-Identifier: MIT
#pragma once
#include "motion_frame_target.h"
// Motion MRT for the standard 1.5.3 DX11 executable. Opt-in, rides on Mode=Observe:
//   [Anomaly] Motion=1                 load motion shader variants, bind RTV2 during the G-buffer pass
//   Pack: <INI directory>\ReScaleFrame\patches\motion.rsfpatch. No shipped game shader bodies.
//   MotionShaderDir is retired; original source is acquired from the engine.
//   MotionInvalidate=1                 replay occluding draws without a variant as invalid motion (default 1)
//   MotionFoliage=1                    tree and grass history (default 1): reads each foliage draw's staged VS constants,
//                                      keeps the previous frame's by stable identity and binds it in b13. 0 binds the
//                                      all-invalid foliage buffer (validity 0). Grass also needs the engine's unused
//                                      instance rows poisoned after each read (motion/foliage.rs).
//   MotionCapture=1                    dump the motion target and G-buffer through the core frame dump service;
//   MotionCaptureStart=600, MotionCaptureCount=3 (max 16), MotionCaptureInterval=300 (main-view frames), and F10
//                                      (refused while SRStageCapture owns the dump service)
//
// Pipeline:
// * create_shader detour (shader_loader.rs): the engine creates, reflects and signature-extracts its stock shader
//   unchanged; for an allowlisted root RSF builds the verified motion variant of that exact permutation and records
//   stock -> variant. Every refusal leaves the stock shader in use.
// * phase_scene_begin/end detours: the G-buffer window. The first open of a frame starts the draw-time history
//   frame (motion.rs), sizes the RGBA16F target to the bound rt_Position texture (so it follows an SR render-
//   size twin) and clears it to zero.
// * CBackend::constants.flush (the geometry producer's draw-time site, rsf_anomaly_geometry_set_flush): after every
//   native upload, keep RTV2 bound across engine binds (motion_target.rs), record the draw's camera and instance
//   snapshot, and for a G-buffer draw whose VS and PS both have linkable variants upload VS/PS b13 and bind the
//   variants. A tessellated draw (r4_enable_tessellation: DX11\tess.hs/ds bound) swaps the whole VS+HS+DS+PS chain, with
//   the DS b13 previous view/projection split, only when all four stages have verified variants and every interface
//   links (shader_loader/reflection.rs). Other occluding draws are marked for the invalidation replay
//   (motion_device/replay.rs).
// * ID3D11DeviceContext Draw/DrawIndexed/DrawInstanced/DrawIndexedInstanced detours on the game's immediate
//   context: run the engine's draw, then the invalidation replay if marked, then put the stock shaders (VS, PS and, for
//   a swapped chain, HS and DS) back so the engine's shader cache matches the device again.
// The SR seam and SR-off FG read the published target with its engine frame and view identity.
#include "build_profile.h"
#include <rescaleframe/game_api.h>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::motion {
// Configure startup and live controls through the shared overlay registry.
void configure(const wchar_t* ini, rsf_game_log_fn log, void* user) noexcept;
// After the observer and geometry hooks have selected and verified the image.
bool install(std::byte* image, std::uint32_t image_size, const BuildProfile& build) noexcept;
bool quiesce() noexcept;
bool active() noexcept;
void report(void (*emit)(void*, const char*), void* user) noexcept;

// Read-only view of the motion target the last G-buffer window cleared and wrote, for SR and FG. Render thread
// only. `texture` is the R16G16B16A16_FLOAT target (ID3D11Texture2D*, owned here, not reference counted, valid until
// the next window resizes it or quiesce); `frame` and `main` identify that window (engine frame, main view);
// `written` is false when the window did not get a usable target. All zero while the stage is inactive.
using FrameTarget = rsf_anomaly_motion_frame_target;
FrameTarget frame_target() noexcept;
}
