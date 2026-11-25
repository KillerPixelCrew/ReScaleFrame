/* SPDX-License-Identifier: MIT */
#pragma once
/* SR stages behind [Anomaly] SRStage (docs/research/anomaly-sr-seam.md sections 9 to 11). Opt-in, Observe mode only.
     SRStage=1  generic0/generic_temp FP16 twins at output size, f^-1/f round trip at the seam.
     SRStage=2  every world target at render size (SRScale, default 0.67, clamped to [0.33, 1.0]), Device size and
                HW.pBaseZB swapped for the world window, bilinear upscale in linear space at the seam.
     SRStage=3  Stage 2 plus a Halton(2,3) subpixel jitter of the world and first-person projections and a lowered
                texture mip bias inside the world window (section 11). The spatial upscale is unchanged.
     SRStage=4  Stage 3 plus the SR inputs prepared at the seam (linear colour, depth twin, motion target with the
                first-person rule) and dumped by SRStageCapture; the output is still the bilinear pass (section 12).
     SRStage=5  Stage 4 with the shared RSF SR pipeline (rsf_get_d3d11_sr_api) evaluating at the seam: DLSS, FSR3, FSR4
                or XeSS as the shared settings select, the reconstruction re-tonemapped into the engine's generic0.
                The render size follows the selected backend's quality plan. Any refusal draws the bilinear pass.
   Stages 1 to 4 call no SR provider.

   Configuration ([Anomaly] keys, sr_stage.rs). SRStage and SRStageCapture select the hooks. SRScale (default 0.67) is
   re-read from the same INI after each device reset (vid_restart applies a new value). SRJitter (default 1), SRMipBias
   (an offset added to log2(render/output), clamped to [-2, 1], or "off"), SRStageCaptureStart/Count/Interval (600,
   3, 300; Stage 3 captures 8, at most 32, consecutive world frames). Stages 4 and 5: SRExposure (constant or auto),
   SRColourTransport (default 0: linear, HDR on), SRFirstPersonZero (default 1), SRCutDistance (metres, default 4),
   SRCutAngle (degrees, default 45), SRRequireSignature (default 0), SR/SRBackend/SRQuality/DLSSPreset (the shared
   frontend keys, with Auto as the default SRBackend). The vendor runtimes are the
   vendor package beside the INI (ReScaleFrame\streamline, fidelityfx, xess). The panel changes the live options and
   saves them.

   Jitter. The jitter published at the last phase_scene_prepare (all zero for scope frames and below Stage 3). The SR
   pass reads it directly; the motion stage reads Stage::jitter_uv for its PS b13
   rsf_jitter on every draw. A sample is in RENDER pixels, +X right, +Y down (rsf_camera_frame::jitter_pixels); in NDC
   it is (2x / W, -2y / H) and in the motion b13 it is (x / W, y / H) in viewport UV, Y down. */
#include "sr_stage_platform.h"
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stdint.h>
#include <wchar.h>

/* Bootstrap configures the stage and the native observer installs it (sr_stage/native.rs), binding the leaves first.
   Install verifies
   every guard of the selected stage on the verified image and installs the hooks, or refuses and leaves the game stock;
   it returns true when nothing is selected. */
