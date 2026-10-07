/* SPDX-License-Identifier: GPL-3.0-only */
/** @file DLSS SR C ABI adapter using official Streamline C++ types internally.
 * Serialize lifecycle/evaluation calls on the graphics owner. A process uses either this adapter's
 * D3D11 Streamline registration or a borrowed D3D12 host, never two registrations. The supplied
 * device, shared host, and diagnostic sink outlive adapter use. SDK headers are optional at build
 * time; unavailable implementations preserve symbols and return NOT_COMPILED.
 */

#ifndef RSF_DLSS_H
#define RSF_DLSS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ABI 4 adds optional translucency hints, motion depth, and encoded-color metadata to ABI 3. */
#define RSF_DLSS_ABI_VERSION 4u

/** Engine identity for NGX initialization, used with engine version/project ID. */
typedef uint32_t rsf_dlss_engine;
#define RSF_DLSS_ENGINE_CUSTOM ((rsf_dlss_engine)0)
#define RSF_DLSS_ENGINE_UNREAL ((rsf_dlss_engine)1)
#define RSF_DLSS_ENGINE_UNITY ((rsf_dlss_engine)2)

typedef int32_t rsf_dlss_result;
#define RSF_DLSS_OK ((rsf_dlss_result)0)
#define RSF_DLSS_ERROR_INVALID_ARGUMENT ((rsf_dlss_result)-1)
#define RSF_DLSS_ERROR_ABI_MISMATCH ((rsf_dlss_result)-2)
/* Built without the Streamline implementation. */
#define RSF_DLSS_ERROR_NOT_COMPILED ((rsf_dlss_result)-3)
/* `sl.interposer.dll` could not be loaded from the path given, or failed signature verification. */
#define RSF_DLSS_ERROR_LOAD_FAILED ((rsf_dlss_result)-4)
/* Loaded, but an entry point this code needs is not exported. A version mismatch looks like this. */
#define RSF_DLSS_ERROR_MISSING_ENTRY_POINT ((rsf_dlss_result)-5)
#define RSF_DLSS_ERROR_INIT_FAILED ((rsf_dlss_result)-6)
/* The driver, OS or adapter says no. `rsf_dlss_query_support` says which. */
#define RSF_DLSS_ERROR_NOT_SUPPORTED ((rsf_dlss_result)-7)
/* A step was skipped: no device set, or evaluate called before support was established. */
#define RSF_DLSS_ERROR_NOT_READY ((rsf_dlss_result)-8)
/* Streamline returned a failure. The log line carries its result code. */
#define RSF_DLSS_ERROR_FEATURE_FAILED ((rsf_dlss_result)-9)

/** Optional UTF-8 diagnostic callback. message is borrowed for the call only. SDK log forwarding
 * can occur on vendor threads; the sink must be thread safe and must not reenter the adapter.
 */
typedef void (*rsf_dlss_log_fn)(void* user, const char* message);

/** Load configuration. Initialize struct_size and RSF_DLSS_ABI_VERSION. Paths and identity are
 * UTF-8; preserve engine-version/project-ID storage through shutdown because it reaches SDK init.
 */
typedef struct rsf_dlss_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    /* Absolute path to `sl.interposer.dll`. Loaded by full path only, never by name, so the
       search path cannot decide which module answers. */
    const char* interposer_path_utf8;
    /* Absolute path to the directory holding `sl.common.dll`, `sl.dlss.dll` and `nvngx_dlss.dll`.
       Streamline looks next to the host executable otherwise, and the host executable here is the
       game, which must not be written to. */
    const char* plugin_directory_utf8;
    /* Optional. Where Streamline writes its own log. Null disables that. */
    const char* log_directory_utf8;
    /* NVIDIA application ID, or zero to identify by engine/project metadata instead. */
    uint32_t application_id;
    /* Engine identity used when application_id is zero. */
    rsf_dlss_engine engine;
    /* Engine version, e.g. "4.18". Required alongside the engine type. */
    const char* engine_version_utf8;
    /* GUID identifying this project, e.g. "a3ed1f08-3542-4698-b85c-e1a9908e861a". */
    const char* project_id_utf8;
    /* Require a valid embedded signature; builds without signature checking refuse this request. */
    uint32_t require_signature;
    rsf_dlss_log_fn log;
    void* log_user;
} rsf_dlss_setup;

/** Load and initialize a D3D11 Streamline registration with manual hooking/frame-based tags.
 * An already initialized adapter returns OK; an independently loaded interposer refuses creation.
 * Loading, missing exports, signature verification, and initialization have distinct error codes.
 */
rsf_dlss_result rsf_dlss_load(const rsf_dlss_setup* setup);

/** Register the game's native ID3D11Device and retain a COM reference until shutdown.
 * Call after load and before support queries/evaluation; resolve feature functions on this device.
 */
rsf_dlss_result rsf_dlss_set_device(void* d3d11_device);
/* Borrow the existing D3D12 Streamline owner. The host outlives SR and owns shutdown. */
rsf_dlss_result rsf_dlss_share_host(void* streamline_host, rsf_dlss_log_fn log, void* user);

/* Requested model for viewport0, independent of render quality. Zero lets NVIDIA choose.
   E/F are legacy; J/K/L/M are current. Driver overrides may take precedence. */
typedef uint32_t rsf_dlss_preset;
#define RSF_DLSS_PRESET_AUTO ((rsf_dlss_preset)0)
#define RSF_DLSS_PRESET_E ((rsf_dlss_preset)5)
#define RSF_DLSS_PRESET_F ((rsf_dlss_preset)6)
#define RSF_DLSS_PRESET_J ((rsf_dlss_preset)10)
#define RSF_DLSS_PRESET_K ((rsf_dlss_preset)11)
#define RSF_DLSS_PRESET_L ((rsf_dlss_preset)12)
#define RSF_DLSS_PRESET_M ((rsf_dlss_preset)13)
/** Validate and remember a model preset for subsequent viewport-zero evaluations. */
rsf_dlss_result rsf_dlss_set_preset(rsf_dlss_preset preset);

/** Support-query output; initialize struct_size and clear optional driver fields before querying. */
typedef struct rsf_dlss_support {
    uint32_t struct_size;
    uint32_t supported;
    /* Which requirement failed, when it did. More than one can be set. */
    uint32_t driver_out_of_date;
    uint32_t os_out_of_date;
    uint32_t no_supported_adapter;
    /* What the feature asks for, when Streamline reports it. */
    uint32_t required_driver_major;
    uint32_t required_driver_minor;
    uint32_t detected_driver_major;
    uint32_t detected_driver_minor;
} rsf_dlss_support;

/* Ask whether DLSS can run on the adapter behind the device that was set. Requires
   rsf_dlss_set_device first, because the adapter comes from it. */
rsf_dlss_result rsf_dlss_query_support(rsf_dlss_support* support);

/* Quality levels, in the same order as the Rust side's `Quality`, which is where the decision of
   which to use is made. */
typedef uint32_t rsf_dlss_quality;
#define RSF_DLSS_QUALITY_NATIVE ((rsf_dlss_quality)0)
#define RSF_DLSS_QUALITY_QUALITY ((rsf_dlss_quality)1)
#define RSF_DLSS_QUALITY_BALANCED ((rsf_dlss_quality)2)
#define RSF_DLSS_QUALITY_PERFORMANCE ((rsf_dlss_quality)3)
#define RSF_DLSS_QUALITY_ULTRA_PERFORMANCE ((rsf_dlss_quality)4)

typedef struct rsf_dlss_plan {
    uint32_t struct_size;
    /* In. */
    uint32_t output_width;
    uint32_t output_height;
    rsf_dlss_quality quality;
    /* Out: the render size DLSS wants, and the range it will accept if the scale moves. */
    uint32_t render_width;
    uint32_t render_height;
    uint32_t render_width_min;
    uint32_t render_height_min;
    uint32_t render_width_max;
    uint32_t render_height_max;
} rsf_dlss_plan;

/** Query SDK render size and accepted dynamic-resolution bounds for the supplied output pixels.
 * Requires initialized feature functions; caller initializes plan.struct_size and input fields.
 */
rsf_dlss_result rsf_dlss_plan_render_size(rsf_dlss_plan* plan);

/** One SR evaluation's borrowed textures and camera data. Textures are ID3D11Texture2D on the
 * native path or ID3D12Resource on the shared-host path. Initialize size/version. Dimensions and
 * jitter use pixels; motion scales convert stored values to normalized screen displacement.
 * Matrices are row major without jitter. Texture leases last through submitted GPU completion.
 */
typedef struct rsf_dlss_frame {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Scene colour before tonemapping, at render resolution. */
    void* color_in;
    /* Where the upscaled result goes, at output resolution. */
    void* color_out;
    void* depth;
    void* motion;
    /* Optional 1x1 exposure texture. Null puts DLSS in auto exposure, at some cost to quality. */
    void* exposure;

    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;
    rsf_dlss_quality quality;

    /* Sub-pixel projection offset in pixels. */
    float jitter_x;
    float jitter_y;
    /* Multiplied into the stored motion so the result lands in [-1,1]. For a buffer already in
       that range this is 1,1; for one in pixels it is the reciprocal of the render size. */
    float motion_scale_x;
    float motion_scale_y;

    /* Row major, without jitter. */
    float camera_view_to_clip[16];
    float clip_to_camera_view[16];
    float clip_to_prev_clip[16];
    float prev_clip_to_clip[16];

    float camera_position[3];
    float camera_up[3];
    float camera_right[3];
    float camera_forward[3];

    float near_plane;
    float far_plane;
    /* Radians. */
    float vertical_fov;
    float aspect_ratio;

    /* The stored value meaning "nothing wrote motion at this pixel". Unreal reserves zero for it,
       and Streamline needs to know so it can supply camera motion there itself. */
    float motion_invalid_value;
    /* Depth closer to the camera holds the larger value. Unreal's reversed-Z does. */
    uint32_t depth_inverted;
    /* Non-zero when supplied vectors include camera movement. Otherwise Streamline can
       reconstruct unwritten motion using depth and clip_to_prev_clip. */
    uint32_t camera_motion_included;
    /* No usable history: a cut, a teleport, or the first frame after a resolution change. */
    uint32_t reset;

    /* Appended in ABI 3. Which of the two viewports this frame is for: 0 is the scene, 1 is a
       second feature with its own history, such as a translucency layer integrated at one to one.
       Each viewport keeps its own options, constants and resources. */
    uint32_t viewport;
    /* Carry the alpha channel through the reconstruction as well as the colour. For a layer
       composited by its alpha afterwards. */
    uint32_t alpha;
    /* The frame this evaluate belongs to, so two viewports evaluated in one frame share a frame
       token. Zero lets Streamline count frames itself. */
    uint32_t frame_index;

    /* Appended in ABI 4. Optional translucency hints at render size, on the same API as the
       textures above; null leaves a hint untagged. Streamline 2.14 hands DLSS SR only the
       transparency and bias hints, and the DLSS 310 guide limits bias to preset F. The colour
       before transparency and the layer are Ray Reconstruction inputs. The reactive mask is for
       the FSR/XeSS adapter, which takes this structure too. */
    void* color_before_transparency;
    void* transparency_layer;
    /* [0,0.9]. */
    void* reactive_mask;
    /* [0,1]: translucent coverage. */
    void* transparency_hint;
    /* {0,1}. */
    void* bias_current_color;
    /* Device depth layer for reprojecting unwritten motion, consumed by the motion resolve
       before any backend sees the frame. DLSS itself never reads it. */
    void* motion_depth_layer;
    /* Non-zero when color_in carries the bounded colour transport (colour_transport.h) rather than
       linear HDR: DLSS then runs with HDR input off and ignores the exposure texture. The caller
       decodes color_out afterwards. */
    uint32_t color_encoded;
} rsf_dlss_frame;

/* Run DLSS for this frame. `d3d11_context` is the `ID3D11DeviceContext*` the game renders with,
   and it must be the immediate context on the thread that owns it.

   Streamline does not restore pipeline state, so the caller owns saving and restoring whatever it
   cares about around this call. */
rsf_dlss_result rsf_dlss_evaluate(void* d3d11_context, const rsf_dlss_frame* frame);
/* D3D12 resources and command list on the borrowed host, with its already minted CPU token.
   Inputs are NON_PIXEL_SHADER_RESOURCE; output is UNORDERED_ACCESS. */
rsf_dlss_result rsf_dlss_evaluate_shared(void* d3d12_list, const rsf_dlss_frame* frame,
                                       uint64_t source_frame_id);

/** Release viewport-zero feature allocations while retaining the registration. The shared-host
 * path drains its queue first; native D3D11 callers ensure prior evaluations have completed.
 */
rsf_dlss_result rsf_dlss_release_resources(void);

/** Release one viewport's feature history/allocations; invalid indices currently map to zero. */
rsf_dlss_result rsf_dlss_release_viewport(uint32_t viewport);

/** Release adapter references after GPU completion. Owned D3D11 mode shuts down/unloads the SDK;
 * shared mode detaches while leaving host shutdown to its owner. The device/host must remain live.
 */
rsf_dlss_result rsf_dlss_shutdown(void);

/** Return build capability only, without loading a runtime or checking adapter support. */
uint32_t rsf_dlss_available(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_DLSS_H */
