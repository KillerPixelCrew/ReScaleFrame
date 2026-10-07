/* SPDX-License-Identifier: GPL-3.0-only */
/* Process-wide D3D11 compatibility pipeline: Streamline initialization, motion decoding,
   frame assembly, SR provider selection and output/history resources. Game plugins supply
   camera conventions and exact scene inputs; these pipeline calls borrow per-frame textures.
   Graphics operations run on one immediate-context owner. Status/dump requests and the two
   color policy setters are thread-safe. SDK acceptance does not establish image quality.
   Research: docs/research/orchestrator-sr-switching.md and ac7-frame-capture.md. */

#ifndef RSF_DLSS_PIPELINE_H
#define RSF_DLSS_PIPELINE_H

#include <rescaleframe/dlss.h>
#include <rescaleframe/frame_assembly.h>
#include <rescaleframe/motion_decode.h>
#include <rescaleframe/runtime.h>
#include <rescaleframe/sr_session.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ABI 4 adds optional translucency inputs; ABI 2 added the independent layer feature. */
#define RSF_DLSS_PIPELINE_ABI_VERSION 4u

typedef int32_t rsf_dlss_pipeline_result;
#define RSF_DLSS_PIPELINE_OK ((rsf_dlss_pipeline_result)0)
/* A missing argument, a short structure, or a frame whose render size is outside the range DLSS
   reported for this quality level. */
#define RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT ((rsf_dlss_pipeline_result)-1)
/* A structure from another build: the setup, the frame, or the camera inside it. Counted as
   neither an evaluated nor a refused frame, because nothing was offered that could be judged. */
#define RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH ((rsf_dlss_pipeline_result)-2)
#define RSF_DLSS_PIPELINE_ERROR_ALREADY_RUNNING ((rsf_dlss_pipeline_result)-3)
/* Nothing has started, or it has already been stopped. */
#define RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING ((rsf_dlss_pipeline_result)-4)
/* Streamline could not be loaded, initialised, or given the device. `rsf_dlss_result` values reach
   the log sink; they are not returned, because a caller of this header should not have to switch
   on a vendor's failures to know it has none. */
#define RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED ((rsf_dlss_pipeline_result)-5)
/* Streamline loaded and the adapter, driver or OS says DLSS cannot run. Distinct from the above:
   nothing is broken, this machine just cannot do it. */
#define RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED ((rsf_dlss_pipeline_result)-6)
/* A device resource could not be created: the output texture, or the decode pass. */
#define RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED ((rsf_dlss_pipeline_result)-7)
/* The decode pass refused this frame's motion target, normally because it is not the size the pass
   was built for. */
#define RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED ((rsf_dlss_pipeline_result)-8)
/* The frame cannot drive a reconstruction and was refused before reaching DLSS. No jitter is the
   usual reason. See `rsf_frame_assembly_result`. */
#define RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED ((rsf_dlss_pipeline_result)-9)
/* Everything was accepted and Streamline still failed the evaluate. */
#define RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED ((rsf_dlss_pipeline_result)-10)

/* Render-owner diagnostic callback. Messages are transient, and repeated frame failures are
   suppressed until the result/extent changes. user remains valid through stop; status/dump
   request calls do not invoke this sink. A shared presentation host may also call the supplied
   backend sink under that host's callback/threading contract. */
typedef void (*rsf_dlss_pipeline_log_fn)(void* user, const char* message);

typedef struct rsf_dlss_pipeline_setup {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Directory containing sl.interposer.dll and its plugins. Read during synchronous start;
       a trailing separator is accepted. Alternate SDK directory strings below are copied. */
    const char* streamline_directory_utf8;
    /* Optional. Where Streamline writes its own log. Null disables that. */
    const char* streamline_log_directory_utf8;
    /* Refuse an interposer without a valid embedded signature. Recommended: this loads a DLL into
       a game process and the path above comes from configuration. Note that a build which cannot
       verify signatures refuses the load outright rather than claiming a check it never ran. */
    uint32_t require_signature;

    /* Output pixels. The selected backend plans its preferred render dimensions/range. */
    uint32_t output_width;
    uint32_t output_height;
    rsf_dlss_quality quality;

    /* Game-owned encoding convention, copied at start. Initialize motion.struct_size;
       scale_x/y must be nonzero. A zero output_scale on either axis defaults to one. */
    rsf_motion_decode_params motion;

    rsf_dlss_pipeline_log_fn log;
    void* log_user;
    /* Supplied by the game integration, never inferred by the runtime. */
    rsf_dlss_engine engine;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    const char* fsr2_directory_utf8;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    float view_space_to_meters;
} rsf_dlss_pipeline_setup;

/* One frame's inputs, all borrowed for the duration of the call and none of them retained.
   Textures are `ID3D11Texture2D*`. */
typedef struct rsf_dlss_pipeline_frame {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Exact pre-tonemap temporal reconstruction input at render resolution. Texture formats
       alone cannot establish which engine target owns this phase; the producer identifies it. */
    void* scene_color;
    void* depth;
    /* The game's own velocity target, still in its own encoding. The decode runs here; handing a
       backend this texture directly gives every static pixel a large constant motion. */
    void* game_motion;
    /* Optional 1x1 exposure target. Null puts DLSS in auto exposure, at some cost to quality. */
    void* exposure;

    uint32_t render_width;
    uint32_t render_height;

    /* Borrowed camera metadata. The pipeline copies it and supplies only decode/sentinel
       metadata, default motion scales and reset causes introduced by feature recreation. */
    const rsf_pipeline_camera_frame* camera;

    /* Appended in ABI 4. Optional translucency hints, rect-local at render size like the inputs
       above. Each backend consumes the ones it documents; the rest are ignored. */
    /* Scene colour before translucency (DLSS ColorBeforeTransparency, FSR opaque-only colour). */
    void* color_before_transparency;
    /* Offscreen translucency layer at render size, when the game drew one. */
    void* transparency_layer;
    /* R32_FLOAT [0,0.9]: FSR reactive, XeSS responsive pixel mask. */
    void* reactive_mask;
    /* R32_FLOAT [0,1]: FSR transparency and composition, DLSS transparency hint. */
    void* transparency_mask;
    /* R32_FLOAT {0,1}: DLSS bias current colour. */
    void* bias_mask;
    /* R32_FLOAT device depth layer (volumetric clouds). Pixels without object velocity are
       reprojected at the nearer of it and scene depth. The depth handed to backends is unchanged. */
    void* motion_depth_layer;
} rsf_dlss_pipeline_frame;

typedef struct rsf_dlss_pipeline_status {
    uint32_t struct_size;
    uint32_t running;
    /* Whether the adapter, driver and OS accept DLSS. Meaningful only once running. */
    uint32_t dlss_supported;
    /* What DLSS asked for at the requested quality and output size. A frame may carry a different
       render size and is accepted while it stays inside the range DLSS reported. */
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;
    /* Successful submissions and refused well-formed calls while running. Missing frame/short
       frame or mismatched frame/camera ABI are excluded. Acceptance is not image validation. */
    uint64_t frames_evaluated;
    uint64_t frames_refused;
    rsf_dlss_pipeline_result last_result;
    /* Appended in ABI 2: the layer feature's frames, and the size it is built for. */
    uint64_t layer_frames_evaluated;
    uint64_t layer_frames_refused;
    uint32_t layer_width;
    uint32_t layer_height;
    rsf_dlss_pipeline_result layer_last_result;
    uint32_t backend;
    uint32_t requested_backend;
    int32_t last_switch_result;
} rsf_dlss_pipeline_status;

/* Independent DLSS viewport 1, native-size temporal integration of premultiplied RGBA16_FLOAT
   translucency with alpha retained. The caller supplies layer depth and size-adjusted jitter. */
typedef struct rsf_dlss_pipeline_layer {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The layer, at `width` x `height` or larger. */
    void* color;
    /* A depth for it at the same size: the scene's depth with the layer's own geometry drawn
       over it. */
    void* depth;
    uint32_t width;
    uint32_t height;
    /* The view the layer was drawn with: the scene's camera at the layer's size, jitter in the
       layer's pixels. */
    const rsf_pipeline_camera_frame* camera;
} rsf_dlss_pipeline_layer;

/* Build the layer feature's output and its zero motion at this size, or rebuild them when the
   size moves. Returns the output texture, `ID3D11Texture2D*` at `width` x `height`, owned by the
   pipeline and valid until the next size change or stop. Call it from the thread that drives the
   frames, before the first `rsf_dlss_pipeline_on_layer` and whenever the layer's size changes. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_prepare_layer(uint32_t width,
                                                                          uint32_t height,
                                                                          void** output);

/* Integrate the layer for this frame into the prepared output. Zero motion is submitted for the
   whole layer and the camera's motion is left to Streamline to derive from the depth, which is
   what the scene does for its static geometry as well. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_on_layer(
    void* d3d11_context, const rsf_dlss_pipeline_layer* layer);

/* Borrowed viewport-1 ID3D11Texture2D, graphics owner only. Valid until a layer-size change or
   stop, and current only after successful on_layer. Null before successful prepare_layer. */
RSF_RUNTIME_API void* rsf_dlss_pipeline_layer_output(void);

/* Load/share Streamline, retain the D3D11 device, probe DLSS and create its output.
   Run on the graphics owner. Engine/project identity comes from setup, and must agree with the
   integration. Failure unwinds partial ownership; start still requires DLSS support even if the
   caller intends to select another provider later. Use direct sessions for independent startup. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_start(
    void* d3d11_device, const rsf_dlss_pipeline_setup* setup);

/* Decode motion, assemble camera/resources, evaluate the selected provider and optional color
   passes. The immediate-context owner calls this with caller-managed binding save/restore.
   reset is added for feature recreation/pending changes without clearing caller reset. Dense
   camera motion may be resolved for unwritten pixels before evaluation and independent FG copy.
   120 consecutive evaluate failures suspend attempts until restart, resize or accepted backend
   selection (DLSS quality changes also resume). Output is usable only after an OK result in the
   same context's GPU command order. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_on_frame(
    void* d3d11_context, const rsf_dlss_pipeline_frame* frame);

/* Borrowed output-resolution ID3D11Texture2D for the active provider. Graphics owner only;
   valid until successful output resize or stop. No AddRef is transferred to the caller. */
RSF_RUNTIME_API void* rsf_dlss_pipeline_output_texture(void);

/* Queue a copied UTF-8 prefix from any thread. The next successful frame performs paired
   input/output/motion/depth/exposure readbacks and writes exact assembled constants. A later
   request replaces a pending prefix. Diagnostic write failure does not fail evaluation. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_request_dump(const char* prefix_utf8);

/* Snapshot of what has actually happened. Safe from any thread. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_get_status(
    rsf_dlss_pipeline_status* status);

/* The DLSS-only colour correction after evaluation (colour_fidelity). It pulls the output toward
   the current jittered frame where depth is continuous, and measured hangar output oscillates
   with it, so it is off unless enabled. Thread-safe; takes effect from the next frame. */
RSF_RUNTIME_API void rsf_dlss_pipeline_set_colour_correction(uint32_t enabled);
/* DLSS receives an invertible display-range encoding of the scene with HDR input off, and its output
   is decoded back to linear (colour_transport.h). On by default: DLSS 310's auto-exposing presets
   band in AC7's dark linear HDR, independently of which preset a driver override selects. Thread-safe. */
RSF_RUNTIME_API void rsf_dlss_pipeline_set_colour_transport(uint32_t enabled);
/* Graphics owner between frames. Plan quality on the active provider and return optional
   render dimensions on success. Refusal preserves prior quality; success resets history and
   the DLSS failure suspension. The engine applies the new dimensions to its next view. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_set_quality(rsf_dlss_quality quality,
                                                                       uint32_t* render_width,
                                                                       uint32_t* render_height);

/* Change the output extent on the graphics execution owner. Prepares the replacement texture
   and active backend before committing. Refusal preserves the current configuration. Input
   planning is returned for the next engine-owned view; history resets on the new output. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_resize_output(uint32_t output_width,
    uint32_t output_height, uint32_t* render_width, uint32_t* render_height);

/* Compatibility selector for the existing frame pipeline. Run at the render-thread command
   boundary. 1 DLSS, 2 FSR2, 3 FSR3, 4 FSR4, 5 XeSS. Refusals preserve the active backend.
   New integrations can use sr_session.h/sr_bridge.h directly with SDK frame records. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_select_backend(uint32_t backend,
    uint32_t* render_width, uint32_t* render_height);

/* These declared preset entry points have no exported implementation in this source tree.
   The compatibility pipeline currently uses its internal AUTO request. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_select_preset(rsf_dlss_preset preset);
RSF_RUNTIME_API rsf_dlss_preset rsf_dlss_pipeline_get_preset(void);

/* Graphics owner after producers/borrowers stop. Release SR/native caches before the retained
   device. Returns NOT_RUNNING if no pipeline is owned; otherwise unwinds resources and logging. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_stop(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_DLSS_PIPELINE_H */
