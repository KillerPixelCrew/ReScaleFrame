/* SPDX-License-Identifier: GPL-3.0-only */
/* What a reconstruction or a frame generator has to be, said once, for all three vendors.
 *
 * Today's DLSS path is written against Streamline's own shapes, which worked while there was one
 * vendor and stops working the moment there are three. FidelityFX and XeSS ask for the same
 * information in different words, and the differences that matter are few and specific:
 *
 *   - Who owns the swap chain. FidelityFX replaces it, XeFG wraps it and hands back a proxy,
 *     Streamline upgrades the factory that made it. All three own it; none of them share it.
 *   - What a tagged resource's lifetime is. Streamline can be told a tag is valid until the next
 *     present; FidelityFX double buffers internally when asked; XeFG copies. Getting this wrong
 *     produces a frame interpolated from a buffer the game has already overwritten, which looks
 *     like a smear and not like an error.
 *   - Which API the work runs on. Only XeSS reconstructs on D3D11 at all, and only on Intel.
 *
 * Everything else is common, so it is expressed once here and each backend translates. The point is
 * not abstraction for its own sake: it is that the orchestrator can ask "can you do this" and get a
 * refusal with a reason, instead of every caller knowing which vendor is loaded.
 */

#ifndef RSF_BACKEND_H
#define RSF_BACKEND_H

#include <rescaleframe/game_frame.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_BACKEND_ABI_VERSION 1u

/* Ordered as the Rust capability model orders them, so the two can be compared field by field
   rather than through a mapping nobody maintains. */
typedef uint32_t rsf_vendor;
#define RSF_VENDOR_NONE ((rsf_vendor)0)
#define RSF_VENDOR_NVIDIA ((rsf_vendor)1)
#define RSF_VENDOR_INTEL ((rsf_vendor)2)
#define RSF_VENDOR_AMD ((rsf_vendor)3)

/* A bitmask, because a backend may support several and the question asked of it is "which of these
   can you do", not "which one are you". */
typedef uint32_t rsf_gfx_api;
#define RSF_API_D3D11 0x1u
#define RSF_API_D3D12 0x2u
#define RSF_API_VULKAN 0x4u

typedef uint32_t rsf_quality;
#define RSF_QUALITY_NATIVE ((rsf_quality)0)
#define RSF_QUALITY_QUALITY ((rsf_quality)1)
#define RSF_QUALITY_BALANCED ((rsf_quality)2)
#define RSF_QUALITY_PERFORMANCE ((rsf_quality)3)
#define RSF_QUALITY_ULTRA_PERFORMANCE ((rsf_quality)4)
/* FidelityFX has one above native that the others do not. Named rather than folded into native,
   because a caller asking for it and silently getting native is a quality setting that lies. */
#define RSF_QUALITY_ULTRA_QUALITY ((rsf_quality)5)

/* How long a tagged resource stays the vendor's to read.
 *
 * ONLY_NOW means the vendor consumes it during the call and the caller may reuse the memory
 * immediately. UNTIL_NEXT_PRESENT means it may not, which is what frame generation needs, because
 * interpolation happens after the call returns. A backend that cannot honour the second says so in
 * its capabilities rather than accepting it and reading freed memory. */
typedef uint32_t rsf_lifetime;
#define RSF_LIFETIME_ONLY_NOW ((rsf_lifetime)0)
#define RSF_LIFETIME_UNTIL_NEXT_PRESENT ((rsf_lifetime)1)

/* Which surfaces a frame generator wants for the interface, as a bitmask of what it accepts. The
   three vendors name these differently and mean the same things. */
typedef uint32_t rsf_ui_mode;
/* No interface handling: the caller composites and the generator interpolates the result, which
   smears the interface along with the scene. */
#define RSF_UI_MODE_NONE 0x1u
/* A premultiplied interface layer, which the generator composites onto every frame it makes. */
#define RSF_UI_MODE_UI_LAYER 0x2u
/* The finished frame plus a HUD-less copy, from which the generator works out the interface. */
#define RSF_UI_MODE_BACKBUFFER_HUDLESS 0x4u
/* Both: the HUD-less colour and the interface layer. Intel recommends this for Unreal titles and it
   is the one that asks the caller for exactly what extraction already produces. */
#define RSF_UI_MODE_BACKBUFFER_HUDLESS_UI 0x8u

typedef uint32_t rsf_latency_mode;
#define RSF_LATENCY_NONE ((rsf_latency_mode)0)
#define RSF_LATENCY_XELL ((rsf_latency_mode)1)
#define RSF_LATENCY_PCL ((rsf_latency_mode)2)

/* Where a reconstruction runs, which is not always where the game does.
 *
 * Streamline allows one graphics API per process, so when DLSS-G owns frame generation on D3D12 the
 * reconstruction has to move there too, and the scene colour makes a round trip across the bridge
 * to be reconstructed on a device the game never used. That is a real cost and naming the route is
 * how it stays visible rather than becoming an accident of load order. */
typedef uint32_t rsf_sr_route;
#define RSF_SR_ROUTE_NONE ((rsf_sr_route)0)
#define RSF_SR_ROUTE_NATIVE_D3D11 ((rsf_sr_route)1)
#define RSF_SR_ROUTE_BRIDGE_D3D12 ((rsf_sr_route)2)

typedef int32_t rsf_backend_result;
#define RSF_BACKEND_OK ((rsf_backend_result)0)
#define RSF_BACKEND_ERROR_INVALID_ARGUMENT ((rsf_backend_result)-1)
#define RSF_BACKEND_ERROR_ABI_MISMATCH ((rsf_backend_result)-2)
/* Built without this vendor's headers. Distinct from every other failure: it is a fact about the
   build and not about the machine, and a user chasing a missing feature needs to know which. */
#define RSF_BACKEND_ERROR_NOT_COMPILED ((rsf_backend_result)-3)
/* Compiled in, and the runtime library could not be loaded. The path is in the refusal. */
#define RSF_BACKEND_ERROR_LOAD_FAILED ((rsf_backend_result)-4)
#define RSF_BACKEND_ERROR_MISSING_ENTRY_POINT ((rsf_backend_result)-5)
#define RSF_BACKEND_ERROR_INIT_FAILED ((rsf_backend_result)-6)
/* Loaded and working, and this hardware or this configuration is not something it does. */
#define RSF_BACKEND_ERROR_NOT_SUPPORTED ((rsf_backend_result)-7)
#define RSF_BACKEND_ERROR_NOT_READY ((rsf_backend_result)-8)
#define RSF_BACKEND_ERROR_FEATURE_FAILED ((rsf_backend_result)-9)
/* The resources belong to an older generation than the session: a resize or a render size change
   happened between tagging and evaluating. Refused rather than evaluated, because the alternative
   is reconstructing from a buffer of the wrong shape. */
#define RSF_BACKEND_ERROR_STALE_RESOURCES ((rsf_backend_result)-10)
/* The change asked for cannot be made without rebuilding the swap chain, which cannot be done
   underneath a running frame. */
#define RSF_BACKEND_ERROR_NEEDS_RESTART ((rsf_backend_result)-11)
/* Asked to work on an API this backend does not do here, such as reconstruction on D3D11 from a
   vendor that only offers it on Intel hardware. */
#define RSF_BACKEND_ERROR_WRONG_API ((rsf_backend_result)-12)

typedef void (*rsf_backend_log_fn)(void* user, const char* message);

/* What the orchestrator knows when it asks a backend whether it can work here. */
typedef struct rsf_backend_probe_desc {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The API the game itself renders with. */
    rsf_gfx_api game_api;
    /* Which adapter, so a backend can refuse hardware it does not run on without creating anything.
       Two 32 bit halves rather than a LUID, so this header needs no Windows types. */
    uint32_t adapter_luid_low;
    int32_t adapter_luid_high;
    /* Either may be null. A backend that needs a device it was not given says NOT_READY rather than
       creating one of its own, because a second device cannot share resources with the game's. */
    void* d3d11_device;
    void* d3d12_device;
    /* Where the vendor runtime is, as UTF-8. Loaded by absolute path so that a library beside the
       game's executable cannot answer instead. */
    const char* runtime_directory_utf8;
    rsf_backend_log_fn log;
    void* log_user;
} rsf_backend_probe_desc;

/* What a backend can do, filled by probe. Every field exists because some vendor differs on it. */
typedef struct rsf_backend_caps {
    uint32_t struct_size;
    rsf_vendor vendor;
    /* Immutable, plugin-owned, valid until the module unloads. */
    const char* name;
    /* Non-zero when this backend can do anything at all here. When zero, `refusal_utf8` says why in
       a sentence a user can act on, which is the difference between a feature that is missing and
       one that is broken. */
    uint32_t available;
    /* Which APIs reconstruction and generation are offered on. Separate, because they differ:
       XeSS reconstructs on D3D11 and generates only on D3D12. */
    rsf_gfx_api sr_apis;
    rsf_gfx_api fg_apis;
    /* Non-zero when the backend derives camera motion itself and does not need it supplied. */
    uint32_t reconstructs_camera_motion;
    uint32_t needs_exposure;
    /* Zero when this backend does not generate frames. Above one means multi frame generation. */
    uint32_t max_generated_frames;
    rsf_ui_mode ui_modes;
    uint32_t supported_lifetimes;
    /* Non-zero when the backend must create or wrap the swap chain itself. All three do for frame
       generation, which is why the presentation side asks rather than assuming. */
    uint32_t fg_owns_swapchain;
    /* Non-zero when reconstruction and generation must share one session, as Streamline requires,
       so choosing this vendor for one constrains the other. */
    uint32_t sr_fg_share_session;
    rsf_latency_mode latency_modes;
    /* Non-zero when generation needs latency markers to place frames, which decides whether a frame
       with an ambiguous identifier can be interpolated around. */
    uint32_t fg_requires_latency_markers;
    /* Non-zero when the number of generated frames can change without rebuilding the chain. */
    uint32_t supports_dynamic_fg;
    const char* refusal_utf8;
} rsf_backend_caps;

/* One resource handed to a backend, with everything it needs to use it without asking.
 *
 * The state is included because D3D12 has no way to find out: a resource is in whatever state the
 * caller last transitioned it to, and a vendor that guesses wrong corrupts the frame in a way that
 * only shows on some hardware. */
typedef struct rsf_backend_resource {
    uint32_t struct_size;
    /* An `ID3D11Resource*` or `ID3D12Resource*`, matching the API the call is made on. */
    void* resource;
    /* A `D3D12_RESOURCE_STATES` value. Ignored on D3D11, which tracks its own. */
    uint32_t state;
    /* The region actually used, which is not always the whole surface: Unreal renders into a
       sub-rectangle of a pooled target, so at a reduced scale the surface is larger than the view. */
    uint32_t x;
    uint32_t y;
    uint32_t width;
    uint32_t height;
    uint32_t format;
    rsf_lifetime lifetime;
    /* Which surface generation this belongs to. A resource from an older one is stale and refused
       rather than read. */
    uint32_t generation;
} rsf_backend_resource;

typedef struct rsf_sr_open_desc {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_gfx_api api;
    void* device;
    uint32_t output_width;
    uint32_t output_height;
    rsf_quality quality;
    /* Non-zero when the render extent varies between evaluations. Set unconditionally by this
       project: the screen policy changes the scale at every transition, and a feature created at a
       fixed render extent would be destroyed and rebuilt at each one, losing the history that a
       reconstruction exists to accumulate. */
    uint32_t dynamic_resolution;
    uint32_t hdr;
    uint32_t inverted_depth;
    uint32_t motion_jittered;
    uint32_t auto_exposure;
    rsf_backend_log_fn log;
    void* log_user;
} rsf_sr_open_desc;

/* What a reconstruction is given for one frame. Always ONLY_NOW: it consumes them during the call,
   which is the whole reason reconstruction is simpler to wire than generation. */
typedef struct rsf_sr_frame {
    uint32_t struct_size;
    const rsf_frame_record* record;
    rsf_backend_resource color;
    rsf_backend_resource depth;
    rsf_backend_resource motion;
    rsf_backend_resource exposure;
    rsf_backend_resource output;
    /* Pixels of the render extent, sign already applied. */
    float jitter_x;
    float jitter_y;
    /* What multiplies the stored motion to reach pixels of the render extent. */
    float motion_scale_x;
    float motion_scale_y;
    uint32_t reset;
} rsf_sr_frame;

typedef struct rsf_sr_provider {
    uint32_t struct_size;
    rsf_backend_result (*probe)(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps);
    rsf_backend_result (*open)(const rsf_sr_open_desc* desc, void** session);
    /* The render extent for a quality level, which only the vendor knows: the ratios differ between
       them and are not always the obvious fractions. */
    rsf_backend_result (*plan)(void* session, rsf_quality quality, uint32_t* render_width,
                               uint32_t* render_height);
    rsf_backend_result (*evaluate)(void* session, void* command_context, const rsf_sr_frame* frame);
    /* Drop what can be dropped without closing, for a game that has gone to a menu. */
    rsf_backend_result (*release_resources)(void* session);
    void (*close)(void* session);
} rsf_sr_provider;

/* Creating the presentation chain, which is the part no two vendors do alike.
 *
 * All three take the chain over and hand something back, and what they hand back differs: FidelityFX
 * creates its own for the window, XeFG wraps one and returns a proxy, Streamline upgrades the
 * factory so the chain the caller then creates is already theirs. The shape below covers all three
 * by asking for what the window needs and returning what the caller must present through. */
typedef struct rsf_fg_swapchain_desc {
    uint32_t struct_size;
    uint32_t abi_version;
    /* The window, as an HWND. Untyped so this header needs no Windows headers. */
    void* hwnd;
    /* The D3D12 device and direct queue the vendor presents from. Frame generation is D3D12 on all
       three, so these are not optional the way the probe's are. */
    void* d3d12_device;
    void* d3d12_queue;
    /* What the game's own chain looks like, so the proxy matches it. */
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t buffer_count;
    /* The most frames that may ever be generated between two rendered ones. Reserved at creation
       because every vendor allocates for it, and raising it later is NEEDS_RESTART. */
    uint32_t max_generated_frames;
    rsf_ui_mode ui_mode;
    rsf_latency_mode latency;
    uint32_t allow_tearing;
    rsf_backend_log_fn log;
    void* log_user;
} rsf_fg_swapchain_desc;

typedef struct rsf_fg_swapchain_result {
    uint32_t struct_size;
    /* What the caller presents through from now on, as an `IDXGISwapChain*`. Owned by the backend
       and released by `destroy_swapchain`. */
    void* present_chain;
    /* What the vendor actually granted, which may be less than was asked for. Reported rather than
       assumed: a caller that believes it reserved three and got one will count frames that never
       existed. */
    uint32_t effective_max_generated_frames;
    rsf_ui_mode effective_ui_mode;
} rsf_fg_swapchain_result;

/* One rendered frame's inputs, handed over before the present that may generate around it.
 *
 * Unlike reconstruction, the vendor keeps these past the call: interpolation happens between this
 * present and the next. That is what `rsf_lifetime` is for and why the ring in the presentation
 * side exists at all. */
typedef struct rsf_fg_frame {
    uint32_t struct_size;
    const rsf_frame_record* record;
    /* The finished frame, and the same frame without the interface on it. The second is what the
       generator interpolates; interpolating the first drags the interface along with the scene. */
    rsf_backend_resource backbuffer;
    rsf_backend_resource hudless;
    /* The interface as a premultiplied layer, which the generator composites onto each frame it
       makes. Exactly what extraction produces, which is the reason extraction is worth doing
       beyond sharpness. */
    rsf_backend_resource ui;
    rsf_backend_resource depth;
    rsf_backend_resource motion;
    float motion_scale_x;
    float motion_scale_y;
    /* Zero means do not generate around this frame: a cut, a menu, a video, or a frame whose
       inputs were incomplete. Distinct from disabling generation, which costs a chain rebuild. */
    uint32_t interpolate;
    uint32_t generated_frames;
} rsf_fg_frame;

typedef struct rsf_fg_provider {
    uint32_t struct_size;
    rsf_backend_result (*probe)(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps);
    rsf_backend_result (*create_swapchain)(const rsf_fg_swapchain_desc* desc,
                                           rsf_fg_swapchain_result* result, void** session);
    /* Change what is generated without rebuilding, where the backend allows it. Returns
       NEEDS_RESTART where it does not, rather than pretending. */
    rsf_backend_result (*set_options)(void* session, uint32_t enabled, uint32_t generated_frames,
                                      rsf_ui_mode ui_mode);
    rsf_backend_result (*tag_frame)(void* session, void* command_list, const rsf_fg_frame* frame);
    /* Called after the present returns, for backends that finish their work there. */
    rsf_backend_result (*after_present)(void* session);
    rsf_backend_result (*resize)(void* session, uint32_t width, uint32_t height, uint32_t format);
    rsf_backend_result (*latency_marker)(void* session, rsf_latency_marker marker,
                                         uint64_t frame_id);
    rsf_backend_result (*latency_sleep)(void* session);
    /* How many frames the vendor says it generated, which is the only honest source for that
       number: counting presents counts frames the vendor may have dropped. */
    rsf_backend_result (*get_generated_count)(void* session, uint64_t* generated);
    void (*destroy_swapchain)(void* session);
} rsf_fg_provider;

/* Each vendor's module exports these two. Both exist whether or not the vendor's headers were
   available at build time; without them `probe` returns NOT_COMPILED, so a caller asks the same
   question of every vendor and gets an answer from each rather than a link error. */
const rsf_sr_provider* rsf_dlss_sr_provider(void);
const rsf_fg_provider* rsf_dlss_fg_provider(void);
const rsf_sr_provider* rsf_fsr_sr_provider(void);
const rsf_fg_provider* rsf_fsr_fg_provider(void);
const rsf_sr_provider* rsf_xess_sr_provider(void);
const rsf_fg_provider* rsf_xess_fg_provider(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_BACKEND_H */
