// Assemble a backend's frame from what a plugin knows, and refuse the pairings that cannot work.
//
// Every check here corresponds to something this project got wrong at least once: sending the
// jittered projection, handing over motion still in the game's storage, or accepting a view with no
// jitter at all, which produces an integration that works and is quietly soft.

#include <rescaleframe/frame_assembly.h>

#include <cmath>
#include <cstdio>
#include <cstring>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

// Stand-ins for textures. Assembly never dereferences them, and using distinct values catches a
// field being copied from the wrong place.
void* const colour_in = reinterpret_cast<void*>(0x1000);
void* const colour_out = reinterpret_cast<void*>(0x2000);
void* const depth = reinterpret_cast<void*>(0x3000);
void* const motion = reinterpret_cast<void*>(0x4000);
void* const exposure = reinterpret_cast<void*>(0x5000);

rsf_camera_frame make_camera()
{
    rsf_camera_frame camera{};
    camera.struct_size = sizeof(camera);
    camera.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;

    for (int index = 0; index < 16; ++index) {
        const float diagonal = (index % 5 == 0) ? 1.0f : 0.0f;
        camera.view_to_clip[index] = diagonal;
        camera.clip_to_view[index] = diagonal;
        camera.clip_to_prev_clip[index] = diagonal;
        camera.prev_clip_to_clip[index] = diagonal;
    }
    // A marker in each matrix, so a mix-up between them is visible rather than invisible.
    camera.view_to_clip[3] = 1.5f;
    camera.clip_to_view[3] = 2.5f;
    camera.clip_to_prev_clip[3] = 3.5f;
    camera.prev_clip_to_clip[3] = 4.5f;

    camera.camera_position[0] = 260968.45f;
    camera.camera_forward[2] = 1.0f;
    camera.camera_up[1] = 1.0f;
    camera.camera_right[0] = 1.0f;

    camera.near_plane = 1.0f;
    camera.far_plane = 0.0f;  // infinite, as a reversed-Z projection has
    camera.vertical_fov = 0.6632f;
    camera.aspect_ratio = 16.0f / 9.0f;

    camera.jitter_pixels[0] = -0.430f;
    camera.jitter_pixels[1] = 0.157f;
    camera.has_jitter = 1;

    camera.depth_inverted = 1;
    camera.camera_motion_included = 0;
    camera.motion_decoded = 1;
    camera.has_motion_sentinel = 1;
    camera.motion_sentinel = -1000.0f;
    camera.motion_scale[0] = 1.0f;
    camera.motion_scale[1] = 1.0f;
    camera.reset = 0;
    return camera;
}

rsf_frame_resources make_resources()
{
    rsf_frame_resources resources{};
    resources.struct_size = sizeof(resources);
    resources.color_in = colour_in;
    resources.color_out = colour_out;
    resources.depth = depth;
    resources.motion = motion;
    resources.exposure = exposure;
    resources.render_width = 1024;
    resources.render_height = 576;
    resources.output_width = 2048;
    resources.output_height = 1152;
    resources.quality = RSF_DLSS_QUALITY_PERFORMANCE;
    return resources;
}

} // namespace

int main()
{
    const rsf_camera_frame camera = make_camera();
    const rsf_frame_resources resources = make_resources();

    rsf_dlss_frame frame{};
    frame.struct_size = sizeof(frame);
    check(rsf_assemble_dlss_frame(&camera, &resources, &frame) == RSF_FRAME_ASSEMBLY_OK,
          "A usable frame must assemble.");

    check(frame.abi_version == RSF_DLSS_ABI_VERSION,
          "The assembled frame must carry the backend's ABI version.");
    check(frame.struct_size == sizeof(rsf_dlss_frame),
          "Assembly must not lose the struct size it was given.");

    check(frame.color_in == colour_in && frame.color_out == colour_out &&
              frame.depth == depth && frame.motion == motion && frame.exposure == exposure,
          "Every resource must land in its own field.");
    check(frame.render_width == 1024 && frame.output_width == 2048, "The sizes must carry over.");
    check(frame.quality == RSF_DLSS_QUALITY_PERFORMANCE, "The quality level must carry over.");

    // The matrix that matters. A projection still carrying the jitter makes the reconstruction
    // correct for a camera that was never rendered, which reads as softness rather than as a bug,
    // so the marker values exist to catch exactly this.
    check(std::fabs(frame.camera_view_to_clip[3] - 1.5f) < 1e-6f,
          "cameraViewToClip must come from the projection the plugin supplied.");
    check(std::fabs(frame.clip_to_camera_view[3] - 2.5f) < 1e-6f,
          "clipToCameraView must not be confused with the projection.");
    check(std::fabs(frame.clip_to_prev_clip[3] - 3.5f) < 1e-6f,
          "clipToPrevClip must be the reprojection matrix.");
    check(std::fabs(frame.prev_clip_to_clip[3] - 4.5f) < 1e-6f,
          "prevClipToClip must be its inverse, not a copy of it.");

    check(std::fabs(frame.jitter_x - -0.430f) < 1e-6f, "The jitter passes through in pixels.");
    check(std::fabs(frame.jitter_y - 0.157f) < 1e-6f, "Both jitter axes pass through.");
    check(frame.depth_inverted == 1u, "Reversed Z must be declared.");
    check(frame.camera_motion_included == 0u,
          "Unreal writes object motion only, and saying otherwise loses the camera reconstruction.");
    check(std::fabs(frame.motion_invalid_value - -1000.0f) < 1e-6f,
          "The sentinel a decode pass wrote is what marks unwritten pixels.");
    check(std::fabs(frame.near_plane - 1.0f) < 1e-6f, "The near plane passes through.");
    check(frame.far_plane > 1.0e6f,
          "An infinite far plane must become a number a backend can use.");

    // Refusals. Each of these is a frame that would assemble into something that renders and is
    // wrong, which is worse than one that refuses.
    rsf_camera_frame broken = camera;
    broken.has_jitter = 0;
    check(rsf_assemble_dlss_frame(&broken, &resources, &frame) ==
              RSF_FRAME_ASSEMBLY_ERROR_NOT_USABLE,
          "Without a jittered projection there is nothing to reconstruct from.");

    broken = camera;
    broken.motion_decoded = 0;
    check(rsf_assemble_dlss_frame(&broken, &resources, &frame) ==
              RSF_FRAME_ASSEMBLY_ERROR_MOTION_NOT_DECODED,
          "Motion still in the game's storage must be refused: no backend can subtract a bias.");

    broken = camera;
    broken.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION + 1u;
    check(rsf_assemble_dlss_frame(&broken, &resources, &frame) ==
              RSF_FRAME_ASSEMBLY_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");

    broken = camera;
    broken.struct_size = 0;
    check(rsf_assemble_dlss_frame(&broken, &resources, &frame) ==
              RSF_FRAME_ASSEMBLY_ERROR_INVALID_ARGUMENT,
          "A short camera frame must be rejected.");

    rsf_frame_resources smaller = resources;
    smaller.output_width = 512;
    smaller.output_height = 288;
    check(rsf_assemble_dlss_frame(&camera, &smaller, &frame) ==
              RSF_FRAME_ASSEMBLY_ERROR_NOT_USABLE,
          "An output smaller than the render size is not upscaling.");

    rsf_frame_resources missing = resources;
    missing.depth = nullptr;
    check(rsf_assemble_dlss_frame(&camera, &missing, &frame) ==
              RSF_FRAME_ASSEMBLY_ERROR_INVALID_ARGUMENT,
          "A missing depth buffer must be rejected.");

    // Exposure is the one input a backend can do without, at some cost to quality.
    rsf_frame_resources no_exposure = resources;
    no_exposure.exposure = nullptr;
    check(rsf_assemble_dlss_frame(&camera, &no_exposure, &frame) == RSF_FRAME_ASSEMBLY_OK,
          "A frame without exposure must still assemble.");
    check(frame.exposure == nullptr, "The absent exposure must stay absent.");

    // Rendering at output resolution is antialiasing without upscaling, which is a real mode.
    rsf_frame_resources native = resources;
    native.render_width = native.output_width;
    native.render_height = native.output_height;
    native.quality = RSF_DLSS_QUALITY_NATIVE;
    check(rsf_assemble_dlss_frame(&camera, &native, &frame) == RSF_FRAME_ASSEMBLY_OK,
          "Rendering at output size must be allowed.");

    return passed ? 0 : 1;
}
