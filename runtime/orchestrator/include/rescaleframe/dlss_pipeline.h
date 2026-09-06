/* SPDX-License-Identifier: GPL-3.0-only */
/* The sequence that puts DLSS on a frame, and the resources that outlive one.

   `rsf_dlss_load`, `rsf_motion_decode_run` and `rsf_assemble_dlss_frame` each do one part of the
   job and none of them knows the order. That order is the thing this project keeps getting wrong,
   because most of it is invisible until a rendered result exists: Streamline needs an identity
   before NGX will start, a backend cannot read Unreal's motion storage so a decode has to run
   first, and a reconstruction feature is built for one pair of sizes so a render scale change is
   not free. So the order lives in one place, with the reasoning attached, rather than being
   written out again by every caller.

   What the pipeline owns is what has to survive between frames: the loaded interposer, the decode
   pass built for the current render size, and the output texture DLSS evaluates into. Everything
   about a single frame comes in as borrowed pointers and is not retained.

   What it does not own, and will not invent: the camera. Matrices, jitter and the camera basis
   come from whatever read the game's view buffer, and a wrong one of those produces a plausible
   image of a camera that was never rendered. This fills in only the fields that are true because
   this code ran, which is the decode's own product.

   Honesty, since it is easy to lose here. A successful evaluate means Streamline accepted the
   inputs, not that the image is right. The motion vector axis and sign convention has never been
   checked against a rendered result, and neither has the assumption that the scene colour handed
   in is the target temporal AA reads. Both are wrong in ways that read as softness or as a slight
   smear rather than as a failure. `rsf_dlss_pipeline_request_dump` exists because looking is the
   only way to settle either. */

#ifndef RSF_DLSS_PIPELINE_H
#define RSF_DLSS_PIPELINE_H

#include <rescaleframe/dlss.h>
#include <rescaleframe/frame_assembly.h>
#include <rescaleframe/motion_decode.h>
#include <rescaleframe/runtime.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_DLSS_PIPELINE_ABI_VERSION 1u

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

/* Progress and diagnostics, one formatted line at a time. Same shape and same reason as every
   other sink in this project: this runs inside a game's render thread, where a returned code often
   never arrives, so each line is written before the step it names rather than after it.

   Called only from the thread that drives the frame. `rsf_dlss_pipeline_request_dump` and
   `rsf_dlss_pipeline_get_status` deliberately log nothing, so the sink never has to be reentrant
   or thread safe. It is also never called at frame rate: a per-frame failure is written once and
   again only when it changes, since a frame that fails usually fails the same way on the next one
   and a sink that appends and closes per line cannot afford one write per frame.

   The sink is not called after `rsf_dlss_pipeline_stop` returns, so `user` need not outlive it. */
typedef void (*rsf_dlss_pipeline_log_fn)(void* user, const char* message);

typedef struct rsf_dlss_pipeline_setup {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Directory holding `sl.interposer.dll` and the plugins beside it, normally
       `vendor/streamline/bin/x64`. The interposer path is built from this rather than asked for
       separately, because Streamline needs both and they have never differed. A trailing separator
       is accepted. */
    const char* streamline_directory_utf8;
    /* Optional. Where Streamline writes its own log. Null disables that. */
    const char* streamline_log_directory_utf8;
    /* Refuse an interposer without a valid embedded signature. Recommended: this loads a DLL into
       a game process and the path above comes from configuration. Note that a build which cannot
       verify signatures refuses the load outright rather than claiming a check it never ran. */
    uint32_t require_signature;

    /* Presented resolution. The render size is not given here; it is asked of DLSS, because DLSS
       is entitled to decide what a quality level means and a size we picked instead is a rejected
       evaluate at best. */
    uint32_t output_width;
    uint32_t output_height;
    rsf_dlss_quality quality;

    /* How this game stores its motion vectors. A property of the game, constant while it runs, so
       it is settled once here rather than per frame. The caller fills `struct_size` inside it like
       any other structure in this project.

       `scale_x` and `scale_y` must be non-zero. There is no default worth guessing, and a zero
       scale decodes every pixel to no motion at all, which looks like a perfectly stable image
       rather than like a mistake. `output_scale_x` and `output_scale_y` are each taken as 1 when
       left at zero, which is the documented "leave it alone" value for a buffer already in the
       range Streamline wants. A flip is -1; zero is never meant. */
    rsf_motion_decode_params motion;

    rsf_dlss_pipeline_log_fn log;
    void* log_user;
} rsf_dlss_pipeline_setup;

/* One frame's inputs, all borrowed for the duration of the call and none of them retained.
   Textures are `ID3D11Texture2D*`. */
typedef struct rsf_dlss_pipeline_frame {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Pre-tonemap scene colour at render resolution.

       Which texture this is cannot be settled from a descriptor. Ace Combat 7 allocates many full
       resolution `R16G16B16A16_FLOAT` targets and the one temporal AA reads is distinguishable
       only by what it is bound alongside, so `rsf_classify_texture` reports scene colour as a
       candidate rather than picking one. The caller resolves that, today by taking slot 0 of the
       bound set, and that assumption is unverified until a rendered result exists. */
    void* scene_color;
    void* depth;
    /* The game's own velocity target, still in its own encoding. The decode runs here; handing a
       backend this texture directly gives every static pixel a large constant motion. */
    void* game_motion;
    /* Optional 1x1 exposure target. Null puts DLSS in auto exposure, at some cost to quality. */
    void* exposure;

    uint32_t render_width;
    uint32_t render_height;

    /* The camera, as whatever read the game's view buffer saw it. Everything in it is used as
       given except the motion fields this pipeline is the one to know: see
       `rsf_dlss_pipeline_on_frame`. */
    const rsf_camera_frame* camera;
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
    /* Frames that reached a successful evaluate, and frames that did not for any reason. They sum
       to the frames seen. The pair is the point: a pipeline that is running and has evaluated
       nothing is the normal outcome of an unjittered projection, and it should be possible to say
       so rather than to report that DLSS is active. */
    uint64_t frames_evaluated;
    uint64_t frames_refused;
    rsf_dlss_pipeline_result last_result;
} rsf_dlss_pipeline_status;

/* Load Streamline, hand over the game's device, and find out whether DLSS can run on it.

   `d3d11_device` is the `ID3D11Device*` the game renders with; it is retained until
   `rsf_dlss_pipeline_stop`. The identity given to Streamline is Unreal 4.18, which is what Ace
   Combat 7 is: without an identity the DLSS plugin loads and then refuses with a message that
   reads exactly like unsupported hardware.

   Refuses with `RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED` when the adapter cannot run DLSS, leaving
   nothing loaded. Call on the thread that will drive the frames. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_start(
    void* d3d11_device, const rsf_dlss_pipeline_setup* setup);

/* Run one frame: decode the motion, assemble the frame, evaluate.

   `d3d11_context` is the `ID3D11DeviceContext*` the game renders with, and must be the immediate
   context on the thread that owns it. Streamline does not restore pipeline state, so a caller that
   cares about its own bindings saves and restores them around this.

   The fields of `frame->camera` this fills in, and no others, because they are the ones that are
   true because this code ran: `motion_decoded`, the sentinel pair from the decode parameters in
   use, `motion_scale` when the caller left it at zero, and `reset` on the first frame a rebuilt
   feature sees, which is a rebuild the caller has no way to observe. `reset` is only ever set,
   never cleared, so the caller's own reasons for one still stand. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_on_frame(
    void* d3d11_context, const rsf_dlss_pipeline_frame* frame);

/* The texture DLSS evaluates into, as an `ID3D11Texture2D*` at output resolution. Owned by the
   pipeline, valid until it stops, and not reference counted for the caller. Call it from the
   thread that drives the frames, which is the only one that knows whether it still exists. */
RSF_RUNTIME_API void* rsf_dlss_pipeline_output_texture(void);

/* Ask for the next frame's output to be written out under this path prefix.

   Safe from any thread, and the only entry point here that is. It records the request and returns;
   the dump happens inside the next `rsf_dlss_pipeline_on_frame`. This split is not tidiness: the
   caller is a key polling thread, an immediate context cannot be used from two threads at once,
   and doing the copy from the polling thread reads whatever the staging resource happened to hold
   and has taken this process down once already. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_request_dump(const char* prefix_utf8);

/* Snapshot of what has actually happened. Safe from any thread. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_get_status(
    rsf_dlss_pipeline_status* status);

/* Release everything in the reverse of the order it was acquired, and before the caller's device
   goes. Safe to call when nothing is running. */
RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_dlss_pipeline_stop(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_DLSS_PIPELINE_H */
