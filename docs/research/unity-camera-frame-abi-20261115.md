# Independent camera and frame ABI compatibility, 15 November 2026

## Correction and rationale

The first compatibility pass proposed moving Unity CameraFrameVersion from 1
to 2 because the native validator used RSF_GAME_FRAME_ABI_VERSION for both
the camera and outer frame record. That proposal would unnecessarily break
existing managed producers even though the camera layout and semantics did not
change. The coordinator corrected the scope: appended source provenance belongs
to rsf_frame_record only. The SDK now explicitly defines FRAME_ABI_VERSION 2
and CAMERA_ABI_VERSION 1 under the RSF_GAME_ prefix.

Native camera producers and validators now use RSF_GAME_CAMERA_ABI_VERSION.
Outer frame record producers and validators retain RSF_GAME_FRAME_ABI_VERSION.
This covers native game adapters, common backend validation, SR/FG orchestration,
Godot, bridge code and existing fixtures. Vulkan files received only exact
camera-version macro substitutions, preserving concurrent edits. Unity's
Native.CameraFrameVersion is 1, and UrpAdapter uses that named constant without
changing field layout, matrices, gameplay code or pinned SDK dependencies.
Unity bridge version 2, native API version 5, CPU fact version 1 and the separate
runtime RSF_FRAME_ASSEMBLY_ABI_VERSION remain unchanged.

## Checks and limits

Source reads identified each camera alias before changing it, including Unity's
c, Skyrim's out, native_sr's source and the embedded record.camera fields.
Remaining FRAME_ABI occurrences were reviewed as outer-record checks/producers,
except intentional negative checks in sr_session. That contract now checks that
camera ABI 1 cannot validate an outer frame and frame ABI 2 cannot validate an
embedded camera, then exercises the existing successful frame-2/camera-1 path.
The focused rendered-source device fixture also now reports FG_INPUTS before
SR, matching production's pre-mutation retention order.

The coordinator attempted the managed build against installed public Unity
6000.6.6/URP 17.2.6, which rejects GetInstanceID APIs as errors. This is a reported
SDK mismatch with the pinned 6000.3 API, not evidence of a successful managed
build or a reason to migrate gameplay code in this compatibility correction.

This worker performed source review and git diff --check only, with no builds,
GPU fixtures, game launches or commits. Added contract checks remain unexecuted
until the coordinator's integrated native gate. This change preserves the
camera contract and does not claim new game acceptance or Unity FG provenance.

## Files changed by this compatibility sweep

- `games/ac7/src/native_renderer.cpp`
- `games/anomaly/src/fg_stage.cpp`
- `games/anomaly/src/sr_pass.h`
- `games/minecraft/native/jni.cpp`
- `games/project-wingman/src/native_camera.cpp`
- `games/rdr2/src/render_camera.cpp`
- `games/skyrim-se/src/camera.cpp`
- `games/unity-mono/managed/Native.cs`
- `games/unity-mono/src/bridge.cpp`
- `integrations/godot/modules/rescaleframe/bridge.cpp`
- `runtime/backends/common/fg_helpers.h`
- `runtime/backends/common/sr_helpers.h`
- `runtime/orchestrator/src/native_fg.cpp`
- `runtime/orchestrator/src/native_fg_dlss_vulkan.cpp`
- `runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp`
- `runtime/orchestrator/src/native_fg_vulkan.cpp`
- `runtime/orchestrator/src/native_fg_xess_vulkan_present.cpp`
- `runtime/orchestrator/src/native_sr.cpp`
- `runtime/orchestrator/src/native_sr_d3d12.cpp`
- `runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp`
- `runtime/orchestrator/src/native_sr_split_vulkan.cpp`
- `runtime/orchestrator/src/native_sr_vulkan.cpp`
- `runtime/orchestrator/src/native_xess_vulkan.cpp`
- `runtime/orchestrator/src/sr_bridge.cpp`
- `runtime/orchestrator/src/sr_legacy_adapter.cpp`
- `runtime/vulkan/fsr4-fg/bridge.cpp`
- `runtime/vulkan/xess-fg/bridge.cpp`
- `sdk/game/include/rescaleframe/game_frame.h`
- `tests/anomaly_sr_pass.cpp`
- `tests/d3d11_host_rendered_fg_device.cpp`
- `tests/d3d11_host_shared_sr_device.cpp`
- `tests/d3d11_present_bridge.cpp`
- `tests/exposure_normalization.cpp`
- `tests/fg_present_bridge.cpp`
- `tests/gl_shared_surface.cpp`
- `tests/native_ffx_vulkan_submission.cpp`
- `tests/native_fg_xess_vulkan_present.cpp`
- `tests/native_sr_fsr4_vulkan.cpp`
- `tests/native_sr_split_vulkan.cpp`
- `tests/native_sr_vulkan.cpp`
- `tests/native_xess_vulkan.cpp`
- `tests/sr_bridge.cpp`
- `tests/sr_session.cpp`
- `tests/unity_sr_d3d12.cpp`
- `docs/research/unity-camera-frame-abi-20261115.md`
