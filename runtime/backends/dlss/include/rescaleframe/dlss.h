/* SPDX-License-Identifier: GPL-3.0-only */
/* DLSS super resolution, reached through Streamline.

   Streamline is the DLSS SDK. There has been no separate one since DLSS 2, so "integrating DLSS"
   means loading `sl.interposer.dll`, tagging four or five resources, providing per frame camera
   constants, and asking it to evaluate.

   This header is the whole contract. Everything behind it compiles against NVIDIA's C++ headers,
   whose structures are versioned, GUID tagged, and in one case abstract with a virtual operator.
   Transcribing those layouts by hand into another language is a silent corruption waiting to
   happen, so the vendor side stays where the vendor's own headers define it, and this narrow C
   surface is what the rest of the project sees.

   The Streamline SDK is not in this repository. Fetch it into `vendor/streamline/` per
   docs/dependencies.md. Without it these entry points still exist and report
   RSF_DLSS_ERROR_NOT_COMPILED, so a checkout that does not have it still builds and tests. */

#ifndef RSF_DLSS_H
#define RSF_DLSS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_DLSS_ABI_VERSION 2u

/* Which engine the host is. Streamline wants an identity before it will start NGX, and NGX is what
   DLSS runs on, so this is not optional decoration: with none of it supplied the DLSS plugin loads
   and then refuses with "Missing NGX context". */
typedef uint32_t rsf_dlss_engine;
#define RSF_DLSS_ENGINE_CUSTOM ((rsf_dlss_engine)0)
#define RSF_DLSS_ENGINE_UNREAL ((rsf_dlss_engine)1)
#define RSF_DLSS_ENGINE_UNITY ((rsf_dlss_engine)2)

typedef int32_t rsf_dlss_result;
#define RSF_DLSS_OK ((rsf_dlss_result)0)
#define RSF_DLSS_ERROR_INVALID_ARGUMENT ((rsf_dlss_result)-1)
#define RSF_DLSS_ERROR_ABI_MISMATCH ((rsf_dlss_result)-2)
/* Built without the Streamline headers present. Not a runtime failure: this build never had the
   code in it. */
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

/* Progress and diagnostics, one formatted line at a time. Same shape as the texture dump sink and
   for the same reason: this runs inside a game's render thread, where a returned code often never
   arrives. */
typedef void (*rsf_dlss_log_fn)(void* user, const char* message);

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
    /* Application id issued by NVIDIA, when there is one. Zero means identify by engine instead,
       which is the route an injected integration has: the id belongs to the game's publisher, not
       to us. */
    uint32_t application_id;
    /* Engine identity, used when `application_id` is zero. Streamline needs one or the other
       before NGX will start, and for a UE4 title `RSF_DLSS_ENGINE_UNREAL` is simply true. */
    rsf_dlss_engine engine;
    /* Engine version, e.g. "4.18". Required alongside the engine type. */
    const char* engine_version_utf8;
    /* GUID identifying this project, e.g. "a3ed1f08-3542-4698-b85c-e1a9908e861a". */
    const char* project_id_utf8;
    /* Refuse to load an interposer without a valid embedded signature. Recommended: this code
       loads a DLL into a game process, and the path comes from configuration. */
    uint32_t require_signature;
    rsf_dlss_log_fn log;
    void* log_user;
} rsf_dlss_setup;

/* Load and initialise Streamline. Manual hooking is used, which is what allows the device to
   already exist: the orchestrator attaches to a running game and never creates one. */
rsf_dlss_result rsf_dlss_load(const rsf_dlss_setup* setup);

/* Hand over the game's `ID3D11Device*`. D3D11 has no device proxy in Streamline, so this is the
   native device and stays the one the game uses. */
rsf_dlss_result rsf_dlss_set_device(void* d3d11_device);

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

/* Ask DLSS what render size a quality level means at this output size.
   The answer comes from the SDK rather than from a ratio computed here, because DLSS is entitled
   to change it and a mismatch between the size we render and the size it expects is a rejected
   evaluate at best. */
rsf_dlss_result rsf_dlss_plan_render_size(rsf_dlss_plan* plan);

/* Everything one frame needs. Textures are `ID3D11Texture2D*`.

   Units are the ones the Rust model produces: jitter in pixels, and a motion scale that takes the
   game's stored motion into the [-1,1] range Streamline requires. Matrices are row major and must
   carry no jitter, which is why jitter is a separate field rather than folded into them. */
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
    /* Whether camera movement is already folded into the motion buffer. Unreal writes object
       motion only, so this is normally false and Streamline reconstructs the rest from depth. */
    uint32_t camera_motion_included;
    /* No usable history: a cut, a teleport, or the first frame after a resolution change. */
    uint32_t reset;
} rsf_dlss_frame;

/* Run DLSS for this frame. `d3d11_context` is the `ID3D11DeviceContext*` the game renders with,
   and it must be the immediate context on the thread that owns it.

   Streamline does not restore pipeline state, so the caller owns saving and restoring whatever it
   cares about around this call. */
rsf_dlss_result rsf_dlss_evaluate(void* d3d11_context, const rsf_dlss_frame* frame);

/* Release DLSS resources for the viewport while leaving Streamline loaded. Worth doing when the
   render size changes, since the feature is built for a specific pair of sizes. */
rsf_dlss_result rsf_dlss_release_resources(void);

/* Shut Streamline down and unload the interposer. Must happen before the game's device goes. */
rsf_dlss_result rsf_dlss_shutdown(void);

/* Whether this build has the Streamline headers compiled in at all. Reported rather than assumed,
   so a caller can say "not built with DLSS support" instead of "DLSS failed". */
uint32_t rsf_dlss_available(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_DLSS_H */
