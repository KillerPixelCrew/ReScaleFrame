// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/dlss_pipeline.h>

#include <rescaleframe/texture_dump.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/motion_resolve.h>
#include <rescaleframe/colour_fidelity.h>
#include <rescaleframe/colour_transport.h>
#include <rescaleframe/log.h>
#include "sr_legacy_adapter.h"

#include <windows.h>

#include <d3d11.h>
#include <wrl/client.h>
#include <memory>

#include <cstdio>
#include <cstring>
#include <atomic>
#include <mutex>
#include <string>
#include <vector>

namespace {

/* How many evaluates may fail in a row before the pipeline stops asking. Generous enough that a
   resource rebuild or a mission load is never mistaken for a wall, small enough that a wall costs
   a fraction of a second rather than the session. */
constexpr uint32_t kEvaluateFailureLimit = 120;

// One pipeline per process, because the entry points carry no handle: there is one game, one
// device and one Streamline. State is a function-local static rather than a namespace-scope object
// so that nothing here runs before the DLL's first call into it.
struct Pipeline {
    // Owned by the thread that drives frames. start, on_frame and stop are not safe to call
    // concurrently with each other and are not meant to be: they all use the device.
    ID3D11Device* device = nullptr;
    ID3D11Texture2D* output = nullptr;
    rsf_motion_decode* decode = nullptr;
    rsf_motion_resolve* dense_motion = nullptr;
    bool hints_reported = false;
    bool clouds_reported = false;
    rsf_colour_fidelity* colour_fidelity = nullptr;
    rsf_colour_transport* transport = nullptr;
    bool transport_create_failed = false;
    bool transport_reported = false;
    bool colour_fidelity_create_failed = false;
    uint32_t dense_width = 0, dense_height = 0;
    rsf_motion_decode_params motion{};
    uint32_t decode_width = 0;
    uint32_t decode_height = 0;
    // The render size the decode pass last failed to build for. Building it is an HLSL compile and
    // two device allocations; the reasons it fails are a missing shader compiler or a device that
    // would not allocate, and neither is fixed by the next frame. Without this the failure path
    // compiles a shader inside the render thread every frame for as long as the game runs.
    uint32_t decode_failed_width = 0;
    uint32_t decode_failed_height = 0;
    // The last failure that reached the log and the render size it named. A frame that fails
    // usually fails the same way on the next one, and a game whose projection carries no jitter
    // fails every frame it will ever render, so without this the expected case costs a file open,
    // write and close per frame on the render thread: the sink appends and closes per line by
    // design and cannot keep up at frame rate.
    struct Reported {
        rsf_dlss_pipeline_result result = RSF_DLSS_PIPELINE_OK;
        uint32_t width = 0;
        uint32_t height = 0;
    };
    Reported reported;
    bool streamline_loaded = false;
    rsf_sr_legacy_adapter* alternate = nullptr;
    uint32_t backend = RSF_SR_DLSS;
    uint32_t requested_backend = RSF_SR_DLSS;
    uint32_t last_evaluated_backend = 0;
    uint32_t last_evaluated_width = 0, last_evaluated_height = 0;
    int32_t last_switch_result = 0;
    std::string sdk_directories[4];
    float units_to_meters = 1;


    /* Consecutive evaluate failures, and whether evaluating has been given up on.

       A failing evaluate is not free. NGX tries to create its feature on each one, and when that
       cannot succeed the attempts cost time and memory that are never returned: the game slows down
       frame by frame until it stops responding and dies. That is a worse outcome than not
       upscaling, and it hides the actual error behind a hang.

       So a run of failures with nothing in between stops it. A single success resets the count,
       because an evaluate that fails while resources are being rebuilt is ordinary. */
    uint32_t consecutive_evaluate_failures = 0;
    bool evaluate_given_up = false;
    bool reset_pending = false;
    rsf_dlss_pipeline_log_fn log = nullptr;
    void* log_user = nullptr;

    // Read from other threads, so written only under `guard`. Nothing below is a resource, and no
    // D3D or Streamline call is ever made while the lock is held: this project has deadlocked
    // twice by re-entering a lock from a callback made inside such a call.
    std::mutex guard;
    bool running = false;
    bool supported = false;
    uint32_t output_width = 0;
    uint32_t output_height = 0;
    uint32_t planned_render_width = 0;
    uint32_t planned_render_height = 0;
    uint32_t render_width_min = 0;
    uint32_t render_height_min = 0;
    uint32_t render_width_max = 0;
    uint32_t render_height_max = 0;
    rsf_dlss_quality quality = RSF_DLSS_QUALITY_NATIVE;
    // Frames that reached a successful evaluate and frames that did not, for the scene and for the
    // layer feature.
    struct Counters {
        uint64_t evaluated = 0;
        uint64_t refused = 0;
        rsf_dlss_pipeline_result last_result = RSF_DLSS_PIPELINE_OK;
    };
    Counters scene;
    bool dump_pending = false;
    std::string dump_prefix;

    // The layer feature: viewport 1, one to one, with its own history.
    ID3D11Texture2D* layer_output = nullptr;
    ID3D11Texture2D* layer_motion = nullptr;
    uint32_t layer_width = 0;
    uint32_t layer_height = 0;
    bool layer_rebuilt = false;
    uint32_t layer_consecutive_failures = 0;
    bool layer_given_up = false;
    Reported layer_reported;
    Counters layer_counts;
};

void release_layer(Pipeline& self)
{
    if (self.layer_output) {
        self.layer_output->Release();
        self.layer_output = nullptr;
    }
    if (self.layer_motion) {
        self.layer_motion->Release();
        self.layer_motion = nullptr;
    }
    self.layer_width = 0;
    self.layer_height = 0;
}

std::atomic<bool> colour_correction{false};
std::atomic<bool> colour_transport{true};
Pipeline& pipeline()
{
    static Pipeline instance;
    return instance;
}

// The pipeline's own sink, through the shared formatter. A line without arguments goes through as
// an argument rather than as the format.
template <class... Arguments>
void say(Pipeline& self, const char* format, Arguments... arguments)
{
    rsf::say(self.log, self.log_user, format, arguments...);
}
inline void say(Pipeline& self, const char* message)
{
    rsf::say(self.log, self.log_user, "%s", message);
}

// Streamline, the decode pass and the dump each take their own sink. All are forwarded to ours with
// a prefix rather than being given to the caller separately, so that one log carries the whole
// sequence in the order it happened. The message goes through as an argument, never as the format,
// because a vendor's line may contain a percent sign.
constexpr char dlss_log_prefix[] = "dlss";
constexpr char decode_log_prefix[] = "motion decode";
constexpr char dump_log_prefix[] = "dump";
template <const char* Prefix>
void forward(void* user, const char* message)
{
    (void)user;
    Pipeline& self = pipeline();
    say(self, "%s: %s", Prefix, message ? message : "");
}

// Whether this failure is worth a line, which it is the first time and again whenever the failure
// or the render size it concerns changes. Called only from the thread that drives frames, which is
// the only one that touches the fields it reads.
bool worth_saying(Pipeline::Reported& reported, rsf_dlss_pipeline_result result, uint32_t width = 0,
                  uint32_t height = 0)
{
    if (reported.result == result && reported.width == width && reported.height == height) {
        return false;
    }
    reported = {result, width, height};
    return true;
}

// A float colour target at output size with every bind, cleared to black once. A fresh texture
// holds whatever was in that memory and there is no promise that a reconstruction writes every
// pixel of it. It does not: a frame whose render size differs from the one the feature was built
// for left a corner of the target untouched, and the previous tenant of that memory showed through
// as blocks. Black there is honest, where blocks read as an artifact of the reconstruction rather
// than as an absence of one. Returns an owned reference, or null.
ID3D11Texture2D* create_cleared_target(ID3D11Device* device, uint32_t width, uint32_t height)
{
    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    // The format Ace Combat 7's own full resolution colour targets are in, established from a
    // replayed capture in docs/research/ac7-frame-capture.md. Matching it means the result can go
    // back where the input came from without a conversion, and it is wide enough for pre-tonemap
    // values.
    description.Format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    // All three binds: DLSS writes it, whatever reinserts it reads it, and a compute pass may yet
    // need to touch it. None of them can be added later without recreating the texture.
    description.BindFlags =
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> target;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &target)) || !target) {
        return nullptr;
    }
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> view;
    device->GetImmediateContext(&context);
    if (context && SUCCEEDED(device->CreateRenderTargetView(target.Get(), nullptr, &view)) && view) {
        const FLOAT black[4] = {0.0f, 0.0f, 0.0f, 1.0f};
        context->ClearRenderTargetView(view.Get(), black);
    }
    return target.Detach();
}

// The render sizes a frame may carry, and the one planned for.
struct RenderRange {
    uint32_t width = 0, height = 0;
    uint32_t min_width = 0, min_height = 0;
    uint32_t max_width = 0, max_height = 0;
};
// A build of the SDK that reports no range is taken to accept the optimal size only, rather than
// being read as "any size".
RenderRange range_of(const rsf_dlss_plan& plan)
{
    RenderRange range;
    range.width = plan.render_width;
    range.height = plan.render_height;
    range.min_width = plan.render_width_min ? plan.render_width_min : plan.render_width;
    range.min_height = plan.render_height_min ? plan.render_height_min : plan.render_height;
    range.max_width = plan.render_width_max ? plan.render_width_max : plan.render_width;
    range.max_height = plan.render_height_max ? plan.render_height_max : plan.render_height;
    return range;
}
// Caller holds `guard`.
void commit_range(Pipeline& self, const RenderRange& range)
{
    self.planned_render_width = range.width;
    self.planned_render_height = range.height;
    self.render_width_min = range.min_width;
    self.render_height_min = range.min_height;
    self.render_width_max = range.max_width;
    self.render_height_max = range.max_height;
}

rsf_backend_result create_alternate(Pipeline& self, uint32_t width, uint32_t height,
                                    rsf_sr_legacy_adapter** out)
{
    return rsf_sr_legacy_create(self.device, width, height, self.sdk_directories[0].c_str(),
        self.sdk_directories[1].c_str(), self.sdk_directories[2].c_str(), self.sdk_directories[3].c_str(),
        self.units_to_meters, self.log, self.log_user, out);
}

/* Input conditioning this pipeline does for a backend before it evaluates. The alternate backends
   condition their own inputs in the legacy adapter, which resolves dense motion with the unwritten
   sentinel and converts units, so only DLSS declares anything here. Colour transport and colour
   correction are remedies for DLSS's own behaviour: its auto-exposing presets band in dark linear
   HDR, and its output is pulled toward the current jittered frame. */
struct BackendNeeds {
    bool dense_motion;
    bool colour_transport;
    bool colour_correction;
};
constexpr BackendNeeds needs_of(uint32_t backend)
{
    return backend == RSF_SR_DLSS ? BackendNeeds{true, true, true} : BackendNeeds{false, false, false};
}

// Undo whatever is currently held, in the reverse of the order it was acquired. Safe to call at any
// point in a partially completed start, which is what the failure paths there rely on.
void tear_down(Pipeline& self)
{
    rsf_native_sr_release_resources();
    rsf_colour_fidelity_destroy(self.colour_fidelity); self.colour_fidelity = nullptr;
    rsf_colour_transport_destroy(self.transport); self.transport = nullptr;
    self.transport_create_failed = false; self.transport_reported = false;
    self.colour_fidelity_create_failed = false;
    rsf_motion_resolve_destroy(self.dense_motion); self.dense_motion = nullptr;
    self.hints_reported = false;
    self.clouds_reported = false;
    self.dense_width = self.dense_height = 0;
    if (self.decode) {
        rsf_motion_decode_destroy(self.decode);
        self.decode = nullptr;
    }
    self.decode_width = 0;
    self.decode_height = 0;
    self.decode_failed_width = 0;
    self.decode_failed_height = 0;
    if (self.output) {
        self.output->Release();
        self.output = nullptr;
    }
    release_layer(self);
    rsf_sr_legacy_destroy(self.alternate);
    self.alternate = nullptr;
    if (self.streamline_loaded) {
        say(self, "shutting streamline down");
        // Frees the DLSS feature's own resources first. Streamline's shutdown does this too, but
        // the order matters when the caller's device is about to go and this makes it explicit.
        rsf_dlss_release_resources();
        rsf_dlss_release_viewport(1u);
        rsf_dlss_shutdown();
        self.streamline_loaded = false;
    }
    if (self.device) {
        self.device->Release();
        self.device = nullptr;
    }

    // Dropped after the last line above, and dropped at all because the DLSS backend keeps
    // `forward<dlss_log_prefix>` as its own sink for the life of the process and that forwards to
    // here. Leaving these set would hand a later vendor line a `log_user` the caller stopped owning
    // when it stopped the pipeline.
    self.log = nullptr;
    self.log_user = nullptr;
    self.reported = {};
    self.last_evaluated_backend = self.last_evaluated_width = self.last_evaluated_height = 0;

    self.layer_consecutive_failures = 0;
    self.layer_given_up = false;
    self.layer_reported = {};
    std::lock_guard<std::mutex> lock(self.guard);
    self.running = false;
    self.backend = self.requested_backend = RSF_SR_DLSS;
    self.last_switch_result = 0;
    self.supported = false;
    self.dump_pending = false;
    self.layer_counts = {};
}

// Record the outcome of a frame and hand it back. Every frame that did not reach a successful
// evaluate counts as refused, whether this code refused it or DLSS did, so that the two counters
// sum to the well formed frames offered while running and a caller cannot report activity that did
// not happen. A call that carried no frame at all, or the wrong ABI, is counted as neither.
rsf_dlss_pipeline_result count(Pipeline& self, Pipeline::Counters& counters, rsf_dlss_pipeline_result result)
{
    std::lock_guard<std::mutex> lock(self.guard);
    if (result == RSF_DLSS_PIPELINE_OK) {
        ++counters.evaluated;
    } else {
        ++counters.refused;
    }
    counters.last_result = result;
    return result;
}

rsf_dlss_pipeline_result finish_layer(Pipeline& self, rsf_dlss_pipeline_result result)
{
    return count(self, self.layer_counts, result);
}

rsf_dlss_pipeline_result finish(Pipeline& self, rsf_dlss_pipeline_result result)
{
    if (result == RSF_DLSS_PIPELINE_OK) {
        // A failure that follows a good frame is news again, so the suppression above only ever
        // covers an unbroken run of the same one.
        self.reported = {};
    }
    return count(self, self.scene, result);
}

} // namespace

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_start(void* device_pointer,
                                                            const rsf_dlss_pipeline_setup* setup)
{
    if (!device_pointer || !setup || setup->struct_size < sizeof(rsf_dlss_pipeline_setup) ||
        !setup->streamline_directory_utf8 || !*setup->streamline_directory_utf8 ||
        setup->output_width == 0 || setup->output_height == 0 ||
        setup->motion.struct_size < sizeof(rsf_motion_decode_params)) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_DLSS_PIPELINE_ABI_VERSION) {
        return RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH;
    }

    Pipeline& self = pipeline();
    if (self.streamline_loaded || self.device) {
        return RSF_DLSS_PIPELINE_ERROR_ALREADY_RUNNING;
    }

    // A zero decode scale multiplies every stored vector to nothing, which produces a motion field
    // of exact zeros: a perfectly stable image rather than a visible failure. There is no default
    // worth guessing for a game's encoding, so this is refused instead.
    if (setup->motion.scale_x == 0.0f || setup->motion.scale_y == 0.0f) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }

    self.log = setup->log;
    self.log_user = setup->log_user;
    self.motion = setup->motion;
    const char* sdk_paths[] = {setup->fsr2_directory_utf8, setup->fsr3_directory_utf8,
                              setup->fsr4_directory_utf8, setup->xess_directory_utf8};
    for (uint32_t i = 0; i < 4; ++i) self.sdk_directories[i] = sdk_paths[i] ? sdk_paths[i] : "";
    self.units_to_meters = setup->view_space_to_meters;

    self.motion.struct_size = uint32_t(sizeof(rsf_motion_decode_params));
    // Per axis, not as a pair. A zero on one axis alone decodes that axis to no motion at all,
    // which is the same silent failure the non-zero test above exists to catch, and there is no
    // reading of an output scale of zero that anyone means: a flip is -1, not 0.
    if (self.motion.output_scale_x == 0.0f) {
        self.motion.output_scale_x = 1.0f;
    }
    if (self.motion.output_scale_y == 0.0f) {
        self.motion.output_scale_y = 1.0f;
    }

    std::string plugins = setup->streamline_directory_utf8;
    while (!plugins.empty() && (plugins.back() == '\\' || plugins.back() == '/')) {
        plugins.pop_back();
    }
    const std::string interposer = plugins + "\\sl.interposer.dll";

    say(self, "loading streamline from %s", plugins.c_str());
    rsf_dlss_setup dlss{};
    dlss.struct_size = uint32_t(sizeof(dlss));
    dlss.abi_version = RSF_DLSS_ABI_VERSION;
    dlss.interposer_path_utf8 = interposer.c_str();
    dlss.plugin_directory_utf8 = plugins.c_str();
    dlss.log_directory_utf8 = setup->streamline_log_directory_utf8;
    dlss.application_id = 0;
    dlss.engine = setup->engine;
    dlss.engine_version_utf8 = setup->engine_version_utf8;
    dlss.project_id_utf8 = setup->project_id_utf8;
    dlss.require_signature = setup->require_signature;
    dlss.log = forward<dlss_log_prefix>;

    auto* shared = rsf_d3d11_present_host();
    const rsf_dlss_result loaded = shared ? rsf_dlss_share_host(shared, setup->log, setup->log_user) : rsf_dlss_load(&dlss);
    if (loaded != RSF_DLSS_OK) {
        say(self, "streamline did not load (result %d)", int(loaded));
        tear_down(self);
        return RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    }
    self.streamline_loaded = true;

    say(self, "handing the game's device to streamline");
    auto* device = static_cast<ID3D11Device*>(device_pointer);
    self.device = device;
    self.device->AddRef();
    const rsf_dlss_result device_set = rsf_dlss_set_device(device_pointer);
    if (device_set != RSF_DLSS_OK) {
        say(self, "streamline refused the device (result %d)", int(device_set));
        tear_down(self);
        return RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    }

    say(self, "asking whether DLSS runs on this adapter");
    rsf_dlss_support support{};
    support.struct_size = uint32_t(sizeof(support));
    const rsf_dlss_result queried = rsf_dlss_query_support(&support);
    if (queried != RSF_DLSS_OK || !support.supported) {
        // Which requirement failed is the difference between "update the driver" and "this machine
        // cannot do it", and neither is visible from the returned code alone.
        say(self, "DLSS is not available here (result %d), driver %u.%u against a required "
                  "%u.%u%s%s%s",
            int(queried), support.detected_driver_major, support.detected_driver_minor,
            support.required_driver_major, support.required_driver_minor,
            support.driver_out_of_date ? ", driver out of date" : "",
            support.os_out_of_date ? ", OS out of date" : "",
            support.no_supported_adapter ? ", no supported adapter" : "");
        tear_down(self);
        return queried == RSF_DLSS_ERROR_NOT_SUPPORTED
                   ? RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED
                   : RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    }

    // The render size comes from DLSS rather than from a ratio computed here. DLSS is entitled to
    // decide what a quality level means, and a size chosen on this side that does not match is a
    // rejected evaluate at best.
    say(self, "asking DLSS what quality level %u renders at for %ux%u", setup->quality,
        setup->output_width, setup->output_height);
    rsf_dlss_plan plan{};
    plan.struct_size = uint32_t(sizeof(plan));
    plan.output_width = setup->output_width;
    plan.output_height = setup->output_height;
    plan.quality = setup->quality;
    const rsf_dlss_result planned = rsf_dlss_plan_render_size(&plan);
    if (planned != RSF_DLSS_OK || plan.render_width == 0 || plan.render_height == 0) {
        say(self, "DLSS gave no render size (result %d)", int(planned));
        tear_down(self);
        return RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    }

    say(self, "creating the %ux%u output target", setup->output_width, setup->output_height);
    self.output = create_cleared_target(device, setup->output_width, setup->output_height);
    if (!self.output) {
        say(self, "the output target could not be created");
        tear_down(self);
        return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
    }

    {
        std::lock_guard<std::mutex> lock(self.guard);
        self.supported = true;
        self.output_width = setup->output_width;
        self.output_height = setup->output_height;
        self.quality = setup->quality;
        commit_range(self, range_of(plan));
        self.scene = {};
        // Starting again is the way back from having given up, which the message says.
        self.consecutive_evaluate_failures = 0;
        self.evaluate_given_up = false;
        self.running = true;
    }

    say(self, "pipeline ready: DLSS renders %ux%u for %ux%u", plan.render_width, plan.render_height,
        setup->output_width, setup->output_height);
    return RSF_DLSS_PIPELINE_OK;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_on_frame(void* context_pointer,
                                                               const rsf_dlss_pipeline_frame* frame)
{
    if (!context_pointer || !frame || frame->struct_size < sizeof(rsf_dlss_pipeline_frame)) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }
    if (frame->abi_version != RSF_DLSS_PIPELINE_ABI_VERSION) {
        return RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH;
    }

    Pipeline& self = pipeline();
    if (!self.device || !self.output || !self.streamline_loaded) {
        return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    }
    /* Given up on, and said so once already. Returning here rather than at the evaluate skips the
       decode and the frame assembly as well, so a run that cannot upscale costs the game nothing
       instead of costing it more every frame. */
    if (self.evaluate_given_up) {
        return finish(self, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED);
    }
    if (!frame->scene_color || !frame->depth || !frame->game_motion || !frame->camera ||
        frame->render_width == 0 || frame->render_height == 0) {
        return finish(self, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    }
    // Checked before the copy below, which reads a whole structure out of the caller's memory.
    if (frame->camera->struct_size < sizeof(rsf_pipeline_camera_frame)) {
        return finish(self, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    }
    // Checked here rather than left to assembly, which would report a camera from a mismatched
    // build as a refused frame. That code reads as "no jitter", and chasing an anti-aliasing gate
    // that is not the problem is the expensive way to find a rebuild was needed. Uncounted, like
    // the frame ABI above: this is not a frame that was offered and turned down.
    if (frame->camera->abi_version != RSF_FRAME_ASSEMBLY_ABI_VERSION) {
        return RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH;
    }

    // One line per frame would drown the sink, which appends and closes per line by design, so the
    // sequence is announced once. Failures and size changes still speak every time.
    bool first = false;
    uint32_t width_min = 0;
    uint32_t height_min = 0;
    uint32_t width_max = 0;
    uint32_t height_max = 0;
    uint32_t output_width = 0;
    uint32_t output_height = 0;
    rsf_dlss_quality quality = RSF_DLSS_QUALITY_NATIVE;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        first = self.scene.evaluated == 0 && self.scene.refused == 0;
        width_min = self.render_width_min;
        height_min = self.render_height_min;
        width_max = self.render_width_max;
        height_max = self.render_height_max;
        output_width = self.output_width;
        output_height = self.output_height;
        quality = self.quality;
    }

    if (frame->render_width < width_min || frame->render_width > width_max ||
        frame->render_height < height_min || frame->render_height > height_max) {
        if (worth_saying(self.reported, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT, frame->render_width,
                         frame->render_height)) {
            say(self,
                "frame renders at %ux%u, outside the %ux%u to %ux%u DLSS accepts at this quality",
                frame->render_width, frame->render_height, width_min, height_min, width_max,
                height_max);
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    }

    // A decode pass is built for one size, and so is the DLSS feature behind it. A render scale
    // change therefore costs both, which is why it is worth noticing rather than absorbing.
    if (self.decode &&
        (self.decode_width != frame->render_width || self.decode_height != frame->render_height)) {
        say(self, "render size moved from %ux%u to %ux%u, rebuilding", self.decode_width,
            self.decode_height, frame->render_width, frame->render_height);
        rsf_motion_decode_destroy(self.decode);
        self.decode = nullptr;
        self.decode_width = 0;
        self.decode_height = 0;
        rsf_dlss_release_resources();
    }
    // Set when this frame is the first one a freshly built feature sees, either because nothing had
    // been built yet or because the size change above threw the old one away. DLSS has no history
    // it can use across that, and reusing what it has produces a smear that decays over several
    // frames rather than an error. The caller cannot know this: it does not see the rebuild.
    const bool rebuilt = self.decode == nullptr;
    if (!self.decode) {
        // Not retried per frame, for the reason recorded on the fields themselves.
        if (self.decode_failed_width == frame->render_width &&
            self.decode_failed_height == frame->render_height) {
            return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        }
        say(self, "building the motion decode pass for %ux%u", frame->render_width,
            frame->render_height);
        rsf_motion_decode_setup decode{};
        decode.struct_size = uint32_t(sizeof(decode));
        decode.abi_version = RSF_MOTION_DECODE_ABI_VERSION;
        decode.width = frame->render_width;
        decode.height = frame->render_height;
        // Decoded NDC motion spans [-2,2]. R16G16_FLOAT steps by about 5e-4 NDC near one, which is
        // a few tenths of a pixel at common render widths, coarser than the UNORM16 source. The
        // dense resolve consumes it on D3D11; only its R16G16_FLOAT output is shared with D3D12.
        decode.output_format = DXGI_FORMAT_R32G32_FLOAT;
        decode.log = forward<decode_log_prefix>;
        const rsf_motion_decode_result built =
            rsf_motion_decode_create(self.device, &decode, &self.decode);
        if (built != RSF_MOTION_DECODE_OK || !self.decode) {
            if (worth_saying(self.reported, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED, frame->render_width,
                             frame->render_height)) {
                say(self, "the motion decode pass could not be built (result %d)", int(built));
            }
            self.decode = nullptr;
            self.decode_failed_width = frame->render_width;
            self.decode_failed_height = frame->render_height;
            return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        }
        self.decode_width = frame->render_width;
        self.decode_height = frame->render_height;
        self.decode_failed_width = 0;
        self.decode_failed_height = 0;
    }

    if (first) {
        say(self, "first frame: decoding motion, assembling, evaluating");
    }

    // Mandatory, not an optimisation. Every backend offers a scale factor for motion and nothing to
    // subtract a bias with, and Unreal's storage is biased, so the game's own target reads as a
    // large constant motion across a still image.
    const rsf_motion_decode_result decoded =
        rsf_motion_decode_run(self.decode, context_pointer, frame->game_motion, &self.motion);
    if (decoded != RSF_MOTION_DECODE_OK) {
        if (worth_saying(self.reported, RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED, frame->render_width,
                         frame->render_height)) {
            say(self, "the motion decode did not run (result %d)", int(decoded));
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED);
    }

    // Only the fields this pipeline is the one to know. The matrices, the jitter and the camera
    // basis come from whatever read the game's view buffer and are used exactly as given: a value
    // invented here would produce a plausible image of a camera that was never rendered, which
    // reads as softness rather than as a bug.
    rsf_pipeline_camera_frame camera = *frame->camera;
    camera.struct_size = uint32_t(sizeof(rsf_pipeline_camera_frame));
    camera.motion_decoded = 1u;
    // After the decode a stored zero is an ordinary value that real motion can take, so the marker
    // for "nothing wrote this pixel" is whatever the decode wrote instead, and only exists if the
    // encoding reserved a clear value in the first place.
    camera.has_motion_sentinel = self.motion.zero_means_unwritten ? 1u : 0u;
    camera.motion_sentinel = self.motion.invalid_value;
    // A caller that gives a scale gives canonical current-minus-previous UV (the native path's
    // producer measures it). Left at zero, the decode's output is used as it stands.
    const bool motion_in_uv = camera.motion_scale[0] != 0.0f || camera.motion_scale[1] != 0.0f;
    // Decoded Unreal motion already spans the [-1,1] range Streamline wants, so 1 and 1 leave it
    // alone. The axis directions and the sign of the difference are unverified: they can only be
    // settled against a rendered result, and a flip belongs in the decode's output scale rather
    // than here when one turns out to be needed.
    if (camera.motion_scale[0] == 0.0f && camera.motion_scale[1] == 0.0f) {
        camera.motion_scale[0] = 1.0f;
        camera.motion_scale[1] = 1.0f;
    }
    // Or'd in, never cleared: the caller's own reasons for a reset are its own, and this adds the
    // one only this code knows about.
    if (rebuilt || self.reset_pending) {
        camera.reset = 1u;
    }

    rsf_frame_resources resources{};
    resources.struct_size = uint32_t(sizeof(resources));
    // Slot 0 of the bound set, as the caller found it. Nothing in a descriptor distinguishes the
    // scene colour temporal AA reads from the other full resolution float targets in the frame, so
    // this is an assumption and stays one until a rendered result confirms it.
    resources.color_in = frame->scene_color;
    resources.color_out = self.output;
    resources.depth = frame->depth;
    resources.motion = rsf_motion_decode_texture(self.decode);
    resources.exposure = frame->exposure;
    resources.render_width = frame->render_width;
    resources.render_height = frame->render_height;
    resources.output_width = output_width;
    resources.output_height = output_height;
    resources.quality = quality;

    rsf_dlss_frame dlss_frame{};
    dlss_frame.struct_size = uint32_t(sizeof(dlss_frame));
    const rsf_frame_assembly_result assembled =
        rsf_assemble_dlss_frame(&camera, &resources, &dlss_frame);
    if (assembled != RSF_FRAME_ASSEMBLY_OK) {
        // Worth naming once, and only once: a frame refused for want of jitter is the expected
        // state of Ace Combat 7 until the anti-aliasing gate is patched, so it is not a
        // malfunction and it repeats for as long as the game runs.
        if (worth_saying(self.reported, RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED, frame->render_width,
                         frame->render_height)) {
            say(self, "the frame was refused before DLSS saw it (result %d)", int(assembled));
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED);
    }
    dlss_frame.color_before_transparency = frame->color_before_transparency;
    dlss_frame.transparency_layer = frame->transparency_layer;
    dlss_frame.reactive_mask = frame->reactive_mask;
    dlss_frame.transparency_hint = frame->transparency_mask;
    dlss_frame.bias_current_color = frame->bias_mask;
    dlss_frame.motion_depth_layer = frame->motion_depth_layer;
    if (frame->reactive_mask && !self.hints_reported) {
        self.hints_reported = true;
        say(self, "translucency hints: opaque-only colour%s, reactive, coverage and bias masks at %ux%u",
            frame->transparency_layer ? ", separate layer" : "", frame->render_width, frame->render_height);
    }
    if (frame->motion_depth_layer && !self.clouds_reported) {
        self.clouds_reported = true;
        say(self, "cloud motion: unwritten pixels reproject at the nearer of cloud and scene depth");
    }

    const BackendNeeds needs = needs_of(self.backend);
    if (needs.dense_motion && !camera.camera_motion_included && camera.has_motion_sentinel) {
        bool rebuilt_resolve = false;
        if (!rsf::fit_motion_resolve(self.device, frame->render_width, frame->render_height, self.dense_motion,
                self.dense_width, self.dense_height, &rebuilt_resolve))
            return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        if (rebuilt_resolve)
            say(self, "DLSS dense motion: explicit unwritten sentinel, preserving valid zero vectors at %ux%u",
                frame->render_width, frame->render_height);
        rsf_motion_resolve_params resolve{}; resolve.struct_size = sizeof(resolve);
        std::memcpy(resolve.clip_to_previous, dlss_frame.clip_to_prev_clip, sizeof(resolve.clip_to_previous));
        // Canonical sparse inputs are current-minus-previous UV. Resolve emits previous-minus-current pixels.
        resolve.decoded_to_pixels[0] = -float(frame->render_width) * dlss_frame.motion_scale_x;
        resolve.decoded_to_pixels[1] = -float(frame->render_height) * dlss_frame.motion_scale_y;
        resolve.sentinel = camera.motion_sentinel; resolve.has_sentinel = 1;
        resolve.depth_layer = frame->motion_depth_layer;
        if (!rsf_motion_resolve_run(self.dense_motion, context_pointer, dlss_frame.motion, dlss_frame.depth, &resolve))
            return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        dlss_frame.motion = rsf_motion_resolve_motion(self.dense_motion);
        dlss_frame.depth = rsf_motion_resolve_depth(self.dense_motion);
        dlss_frame.camera_motion_included = 1;
        dlss_frame.motion_scale_x = 1.0f / float(frame->render_width);
        dlss_frame.motion_scale_y = 1.0f / float(frame->render_height);
    }
    const bool correct_colour = needs.colour_correction && colour_correction.load(std::memory_order_acquire);
    if (correct_colour && !self.colour_fidelity) {
        if (!self.colour_fidelity_create_failed &&
            !rsf_colour_fidelity_create(self.device, &self.colour_fidelity)) {
            self.colour_fidelity_create_failed = true;
            say(self, "DLSS colour correction could not create its GPU pass; retaining the native graph");
        }
        if (!self.colour_fidelity) return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        say(self, "DLSS colour correction: current linear scene colour, depth-guarded bounded RGB residual, before native tonemapping");
    }
    // DLSS 310's Performance and Ultra Performance defaults (presets M and L) auto-expose and band in
    // AC7's dark linear HDR; a preset choice cannot fix that, since a driver override replaces it.
    // DLSS instead receives an invertible display-range encoding and runs with HDR input off, on
    // every preset, and its output is decoded back to linear before the engine grades it.
    const bool transported = needs.colour_transport && colour_transport.load(std::memory_order_acquire);
    if (transported && !self.transport && !self.transport_create_failed &&
        !rsf_colour_transport_create(self.device, &self.transport)) {
        self.transport_create_failed = true;
        say(self, "DLSS colour transport could not create its GPU passes; DLSS receives linear HDR");
    }
    if (transported && self.transport) {
        void* encoded = nullptr;
        if (!rsf_colour_transport_encode(self.transport, context_pointer, frame->scene_color, frame->exposure,
                frame->render_width, frame->render_height, &encoded))
            return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        dlss_frame.color_in = encoded; dlss_frame.color_encoded = 1; dlss_frame.exposure = nullptr;
        if (!self.transport_reported) {
            self.transport_reported = true;
            say(self, "DLSS colour transport: exposed display-range encoding in, HDR input off, decoded to linear with highlight recovery");
        }
    }
    rsf_dlss_result evaluated = RSF_DLSS_OK;
    if (self.backend == RSF_SR_DLSS) {
        evaluated = rsf_native_fg_evaluate(context_pointer, &dlss_frame);
    } else {
        // The legacy adapter takes canonical UV. The decode's own output, with no scale from the
        // caller, is current-minus-previous NDC with y up, as the adapter has always read it.
        rsf_dlss_frame alternate_frame = dlss_frame;
        if (!motion_in_uv) {
            alternate_frame.motion_scale_x *= 0.5f;
            alternate_frame.motion_scale_y *= -0.5f;
        }
        evaluated = rsf_sr_legacy_evaluate(self.alternate, context_pointer, &alternate_frame,
                                           self.motion.zero_means_unwritten);
    }
    // Every backend offers its frame to FG capture, which keeps one copy per frame: DLSS through a
    // Streamline host has already filled the slot in its evaluate. The alternate backends hand over
    // the adapter's dense inputs.
    if (evaluated == RSF_DLSS_OK) {
        auto generation_frame = dlss_frame;
        if (self.backend != RSF_SR_DLSS &&
            rsf_sr_legacy_fg_inputs(self.alternate, &generation_frame.depth, &generation_frame.motion))
            generation_frame.camera_motion_included = 1;
        rsf_native_fg_capture(context_pointer, &generation_frame);
    }
    if (evaluated != RSF_DLSS_OK) {
        ++self.consecutive_evaluate_failures;
        if (self.consecutive_evaluate_failures >= kEvaluateFailureLimit) {
            self.evaluate_given_up = true;
            say(self,
                "SR failed to evaluate %u frames in a row (last result %d), so it will not be "
                "asked again. Each attempt costs time and memory that is not returned, and a game "
                "slowing to a stop hides the error rather than showing it. Restart the backend to "
                "try again. If the log above says NGX create feature failed, a capture layer such "
                "as RenderDoc is the usual reason: it wraps the device and NGX refuses it",
                self.consecutive_evaluate_failures, int(evaluated));
        } else if (worth_saying(self.reported, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED, frame->render_width,
                                frame->render_height)) {
            say(self, "SR did not evaluate this frame (result %d)", int(evaluated));
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED);
    }
    if (dlss_frame.color_encoded &&
        !rsf_colour_transport_decode(self.transport, context_pointer, self.output, frame->exposure,
            output_width, output_height, frame->scene_color, frame->render_width, frame->render_height,
            camera.jitter_pixels)) {
        self.reset_pending = true;
        return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
    }
    if (first) {
        // Deliberately not "DLSS is working". Streamline accepted the inputs and wrote the output
        // target; whether the image in it is correct is a separate question that only looking at
        // one answers, which is what the dump below is for.
        say(self, "SR evaluated a frame. This says the inputs were accepted, not that the image "
                  "is right.");
    }

    // Taken here rather than in request_dump. The requesting thread is a key poller, and using an
    // immediate context from two threads at once reads whatever the staging copy happened to hold
    // and has taken this process down once already.
    std::string prefix;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        if (self.dump_pending) {
            self.dump_pending = false;
            prefix = self.dump_prefix;
        }
    }
    if (correct_colour) {
        if (!prefix.empty()) {
            const std::string raw_prefix = prefix + "_output_uncorrected";
            rsf_texture_dump_options raw{}; raw.struct_size = sizeof(raw);
            raw.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION; raw.output_prefix_utf8 = raw_prefix.c_str();
            raw.log = forward<dump_log_prefix>;
            rsf_dump_texture_bytes(self.device, context_pointer, self.output, &raw);
        }
        if (!rsf_colour_fidelity_run(self.colour_fidelity, context_pointer, frame->scene_color,
                self.output, dlss_frame.depth, dlss_frame.depth_inverted, camera.jitter_pixels)) {
            self.reset_pending = true;
            if (worth_saying(self.reported, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED, frame->render_width,
                    frame->render_height))
                say(self, "DLSS colour correction refused this frame; retaining the native graph");
            return finish(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED);
        }
    }
    if (!prefix.empty()) {
        // Both sides of the comparison, from the same frame. An upscaled image on its own says
        // nothing: the question is whether it is this scene, sharper, and that needs the input it
        // was made from rather than a different frame's.
        /* The velocity the backend was actually handed, rather than one that merely looks like
           velocity. The observer retains several `R16G16_UNORM` targets and the key dump writes the
           first of them, which is not necessarily the one the pass binds: a flight capture read
           entirely unwritten while the tap was recognising motion in every frame. These two come
           from this frame's own inputs, so what they show is what the backend saw. */
        const struct {
            const char* what;
            const char* suffix;
            void* texture;
        } targets[] = {
            {"the scene colour it was given", "_input", frame->scene_color},
            {"the upscaled result", "_output", self.output},
            {"the game velocity it was given", "_motion", frame->game_motion},
            {"the decoded motion it submitted", "_motion_decoded",
             self.decode ? rsf_motion_decode_texture(self.decode) : nullptr},
            {"the complete motion it submitted", "_motion_submitted", dlss_frame.motion},
            {"the depth it submitted", "_depth", frame->depth},
            {"the engine exposure guide", "_exposure", frame->exposure},
        };

        for (const auto& target : targets) {
            if (!target.texture) {
                continue;
            }
            const std::string path = prefix + target.suffix;
            say(self, "writing %s to %s", target.what, path.c_str());
            rsf_texture_dump_options options{};
            options.struct_size = uint32_t(sizeof(options));
            options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
            options.output_prefix_utf8 = path.c_str();
            options.view = RSF_DUMP_VIEW_RAW;
            options.scale = 1.0f;
            options.log = forward<dump_log_prefix>;
            rsf_texture_dump_report report{};
            report.struct_size = uint32_t(sizeof(report));
            const rsf_dump_texture_result raw =
                rsf_dump_texture_bytes(self.device, context_pointer, target.texture, &options);
            if (raw != RSF_TEXTURE_OK) say(self, "raw readback of %s failed (result %d)", target.what, int(raw));
            const rsf_dump_texture_result written = target.texture == frame->depth ? raw :
                rsf_dump_texture(self.device, context_pointer, target.texture, &options, &report);
            if (written != RSF_TEXTURE_OK) {
                say(self, "writing %s failed (result %d)", target.what, int(written));
            }
        }
        // Constants are the exact assembled values for these images, not a later view-buffer read.
        const std::string camera_path = prefix + "_frame.json";
        FILE* camera_file = std::fopen(camera_path.c_str(), "wb");
        if (camera_file) {
            std::fprintf(camera_file,
                "{\"render\":[%u,%u],\"output\":[%u,%u],\"backend\":%u,"
                "\"jitter\":[%.9g,%.9g],\"motion_scale\":[%.9g,%.9g],"
                "\"camera_motion_included\":%u,\"reset\":%u,\"clip_to_previous\":[",
                frame->render_width, frame->render_height, output_width, output_height, self.backend,
                double(dlss_frame.jitter_x), double(dlss_frame.jitter_y),
                double(dlss_frame.motion_scale_x), double(dlss_frame.motion_scale_y),
                dlss_frame.camera_motion_included, dlss_frame.reset);
            for (uint32_t i = 0; i < 16; ++i)
                std::fprintf(camera_file, "%s%.9g", i ? "," : "", double(dlss_frame.clip_to_prev_clip[i]));
            std::fprintf(camera_file, "]}\n");
            std::fclose(camera_file);
        }
        // A failed dump is diagnostic only and does not change the frame's outcome.
    }

    // One good frame means the run of failures was a rebuild rather than a wall.
    if (self.last_evaluated_backend != self.backend || self.last_evaluated_width != frame->render_width ||
        self.last_evaluated_height != frame->render_height) {
        say(self,"SR submitted successfully: backend %u, input %ux%u, output %ux%u; reconstruction output ready in GPU command order",
            self.backend,frame->render_width,frame->render_height,output_width,output_height);
        self.last_evaluated_backend = self.backend;
        self.last_evaluated_width = frame->render_width; self.last_evaluated_height = frame->render_height;
    }
    self.reset_pending = false;
    self.consecutive_evaluate_failures = 0;
    return finish(self, RSF_DLSS_PIPELINE_OK);
}

extern "C" void* rsf_dlss_pipeline_output_texture(void)
{
    return pipeline().output;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_request_dump(const char* prefix_utf8)
{
    if (!prefix_utf8 || !*prefix_utf8) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }
    Pipeline& self = pipeline();
    // Nothing is logged here on purpose. This runs on another thread, and the log sink is owned by
    // the one driving frames.
    std::lock_guard<std::mutex> lock(self.guard);
    if (!self.running) {
        return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    }
    self.dump_prefix = prefix_utf8;
    self.dump_pending = true;
    return RSF_DLSS_PIPELINE_OK;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_get_status(rsf_dlss_pipeline_status* status)
{
    if (!status || status->struct_size < sizeof(rsf_dlss_pipeline_status)) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }
    Pipeline& self = pipeline();
    std::lock_guard<std::mutex> lock(self.guard);
    status->running = self.running ? 1u : 0u;
    status->dlss_supported = self.supported ? 1u : 0u;
    status->render_width = self.planned_render_width;
    status->render_height = self.planned_render_height;
    status->output_width = self.output_width;
    status->output_height = self.output_height;
    status->frames_evaluated = self.scene.evaluated;
    status->frames_refused = self.scene.refused;
    status->last_result = self.scene.last_result;
    status->layer_frames_evaluated = self.layer_counts.evaluated;
    status->layer_frames_refused = self.layer_counts.refused;
    status->layer_width = self.layer_width;
    status->layer_height = self.layer_height;
    status->layer_last_result = self.layer_counts.last_result;
    status->backend = self.backend;
    status->requested_backend = self.requested_backend;
    status->last_switch_result = self.last_switch_result;
    return RSF_DLSS_PIPELINE_OK;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_prepare_layer(uint32_t width,
                                                                    uint32_t height,
                                                                    void** output)
{
    Pipeline& self = pipeline();
    if (!output || width == 0 || height == 0) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }
    *output = nullptr;
    if (!self.device || !self.streamline_loaded) {
        return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    }
    if (self.layer_output && self.layer_width == width && self.layer_height == height) {
        *output = self.layer_output;
        return RSF_DLSS_PIPELINE_OK;
    }
    if (self.layer_output) {
        say(self, "layer size moved from %ux%u to %ux%u, rebuilding the layer feature",
            self.layer_width, self.layer_height, width, height);
        rsf_dlss_release_viewport(1u);
    }
    release_layer(self);

    say(self, "creating the %ux%u layer output and its zero motion", width, height);
    self.layer_output = create_cleared_target(self.device, width, height);
    if (!self.layer_output) {
        say(self, "the layer output could not be created");
        release_layer(self);
        return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
    }
    // Zero motion for the whole layer. Its materials carry no velocity of their own, and the
    // camera's share is derived from the depth by Streamline as for the scene's static geometry.
    D3D11_TEXTURE2D_DESC motion{};
    motion.Width = width;
    motion.Height = height;
    motion.MipLevels = 1;
    motion.ArraySize = 1;
    motion.Format = DXGI_FORMAT_R16G16_FLOAT;
    motion.SampleDesc.Count = 1;
    motion.Usage = D3D11_USAGE_DEFAULT;
    motion.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    std::vector<uint32_t> zeros(size_t(width) * size_t(height), 0u);
    D3D11_SUBRESOURCE_DATA initial{};
    initial.pSysMem = zeros.data();
    initial.SysMemPitch = width * 4u;
    if (FAILED(self.device->CreateTexture2D(&motion, &initial, &self.layer_motion)) ||
        !self.layer_motion) {
        say(self, "the layer's zero motion could not be created");
        release_layer(self);
        return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
    }
    self.layer_width = width;
    self.layer_height = height;
    self.layer_rebuilt = true;
    self.layer_consecutive_failures = 0;
    self.layer_given_up = false;
    *output = self.layer_output;
    return RSF_DLSS_PIPELINE_OK;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_on_layer(void* context_pointer,
                                                               const rsf_dlss_pipeline_layer* layer)
{
    if (!context_pointer || !layer || layer->struct_size < sizeof(rsf_dlss_pipeline_layer)) {
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    }
    if (layer->abi_version != RSF_DLSS_PIPELINE_ABI_VERSION) {
        return RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH;
    }
    Pipeline& self = pipeline();
    if (self.backend != RSF_SR_DLSS || !self.device || !self.streamline_loaded || !self.layer_output ||
        !self.layer_motion) {
        return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    }
    if (self.layer_given_up) {
        return finish_layer(self, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED);
    }
    if (!layer->color || !layer->depth || !layer->camera || layer->width != self.layer_width ||
        layer->height != self.layer_height ||
        layer->camera->struct_size < sizeof(rsf_pipeline_camera_frame)) {
        return finish_layer(self, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    }
    if (layer->camera->abi_version != RSF_FRAME_ASSEMBLY_ABI_VERSION) {
        return RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH;
    }

    rsf_pipeline_camera_frame camera = *layer->camera;
    camera.struct_size = uint32_t(sizeof(rsf_pipeline_camera_frame));
    // Zeros are already the decoded convention: no sentinel, nothing to scale.
    camera.motion_decoded = 1u;
    camera.has_motion_sentinel = 0u;
    camera.motion_scale[0] = 1.0f;
    camera.motion_scale[1] = 1.0f;
    if (self.layer_rebuilt) {
        camera.reset = 1u;
        self.layer_rebuilt = false;
    }

    rsf_frame_resources resources{};
    resources.struct_size = uint32_t(sizeof(resources));
    resources.color_in = layer->color;
    resources.color_out = self.layer_output;
    resources.depth = layer->depth;
    resources.motion = self.layer_motion;
    resources.exposure = nullptr;
    resources.render_width = layer->width;
    resources.render_height = layer->height;
    resources.output_width = layer->width;
    resources.output_height = layer->height;
    resources.quality = RSF_DLSS_QUALITY_NATIVE;

    rsf_dlss_frame dlss_frame{};
    dlss_frame.struct_size = uint32_t(sizeof(dlss_frame));
    const rsf_frame_assembly_result assembled =
        rsf_assemble_dlss_frame(&camera, &resources, &dlss_frame);
    if (assembled != RSF_FRAME_ASSEMBLY_OK) {
        if (worth_saying(self.layer_reported, RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED)) {
            say(self, "the layer frame was refused before DLSS saw it (result %d)",
                int(assembled));
        }
        return finish_layer(self, RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED);
    }
    dlss_frame.viewport = 1u;
    dlss_frame.alpha = 1u;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        dlss_frame.frame_index = uint32_t(self.scene.evaluated + self.scene.refused);
    }

    const rsf_dlss_result evaluated = rsf_dlss_evaluate(context_pointer, &dlss_frame);
    if (evaluated != RSF_DLSS_OK) {
        ++self.layer_consecutive_failures;
        if (self.layer_consecutive_failures >= kEvaluateFailureLimit) {
            self.layer_given_up = true;
            say(self, "DLSS failed to integrate the layer %u frames in a row (last result %d), "
                      "so it will not be asked again until the layer is prepared anew",
                self.layer_consecutive_failures, int(evaluated));
        } else if (worth_saying(self.layer_reported, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED)) {
            say(self, "DLSS did not integrate the layer this frame (result %d)", int(evaluated));
        }
        return finish_layer(self, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED);
    }
    if (self.layer_counts.evaluated == 0) {
        say(self, "DLSS integrated the layer at %ux%u, one to one, alpha carried", layer->width,
            layer->height);
    }
    self.layer_consecutive_failures = 0;
    self.layer_reported = {};
    return finish_layer(self, RSF_DLSS_PIPELINE_OK);
}

// No caller in the repository; kept as a public entry point. prepare_layer returns the same texture.
extern "C" void* rsf_dlss_pipeline_layer_output(void)
{
    return pipeline().layer_output;
}

extern "C" void rsf_dlss_pipeline_set_colour_correction(uint32_t enabled)
{
    colour_correction.store(enabled != 0, std::memory_order_release);
}

extern "C" void rsf_dlss_pipeline_set_colour_transport(uint32_t enabled)
{
    colour_transport.store(enabled != 0, std::memory_order_release);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_set_quality(rsf_dlss_quality quality,
                                                                  uint32_t* render_width,
                                                                  uint32_t* render_height)
{
    Pipeline& self = pipeline();
    if (self.backend != RSF_SR_DLSS) {
        uint32_t width = 0, height = 0;
        const auto result = rsf_sr_legacy_select(self.alternate, self.backend, quality, &width, &height);
        if (result != 0) return RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED;
        {
            std::lock_guard<std::mutex> lock(self.guard);
            self.quality = quality;
            self.planned_render_width = width; self.planned_render_height = height;
        }
        self.reset_pending = true;
        if (render_width) *render_width = width;
        if (render_height) *render_height = height;
        return RSF_DLSS_PIPELINE_OK;
    }
    uint32_t output_width = 0;
    uint32_t output_height = 0;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        if (!self.running) {
            return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
        }
        output_width = self.output_width;
        output_height = self.output_height;
    }
    rsf_dlss_plan plan{};
    plan.struct_size = uint32_t(sizeof(plan));
    plan.output_width = output_width;
    plan.output_height = output_height;
    plan.quality = quality;
    const rsf_dlss_result planned = rsf_dlss_plan_render_size(&plan);
    if (planned != RSF_DLSS_OK || plan.render_width == 0 || plan.render_height == 0) {
        say(self, "quality %u refused: DLSS gave no render size (result %d)", unsigned(quality),
            int(planned));
        return RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    }
    {
        std::lock_guard<std::mutex> lock(self.guard);
        self.quality = quality;
        self.reset_pending = true;
        self.evaluate_given_up = false;
        self.consecutive_evaluate_failures = 0;
        commit_range(self, range_of(plan));
    }
    say(self, "quality %u: DLSS renders %ux%u for %ux%u", unsigned(quality), plan.render_width,
        plan.render_height, output_width, output_height);
    if (render_width) {
        *render_width = plan.render_width;
    }
    if (render_height) {
        *render_height = plan.render_height;
    }
    return RSF_DLSS_PIPELINE_OK;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_resize_output(uint32_t width, uint32_t height,
    uint32_t* render_width, uint32_t* render_height) try
{
    auto& self = pipeline();
    if (!width || !height || width > D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION || height > D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION)
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    if (!self.device || !self.output) return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    if (width == self.output_width && height == self.output_height) {
        if (render_width) *render_width = self.planned_render_width;
        if (render_height) *render_height = self.planned_render_height;
        return RSF_DLSS_PIPELINE_OK;
    }
    // The alternate backends accept any render size up to the output.
    RenderRange range{0, 0, 1, 1, width, height};
    using AlternateOwner = std::unique_ptr<rsf_sr_legacy_adapter, decltype(&rsf_sr_legacy_destroy)>;
    AlternateOwner next_alternate(nullptr, rsf_sr_legacy_destroy);
    if (self.backend == RSF_SR_DLSS) {
        rsf_dlss_plan plan{}; plan.struct_size = sizeof(plan);
        plan.output_width = width; plan.output_height = height; plan.quality = self.quality;
        const auto result = rsf_dlss_plan_render_size(&plan);
        if (result != RSF_DLSS_OK || !plan.render_width || !plan.render_height)
            return RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
        range = range_of(plan);
    } else {
        rsf_sr_legacy_adapter* created = nullptr;
        auto result = create_alternate(self, width, height, &created);
        next_alternate.reset(created);
        if (result == RSF_BACKEND_OK)
            result = rsf_sr_legacy_select(created, self.backend, self.quality, &range.width, &range.height);
        if (result != RSF_BACKEND_OK || !range.width || !range.height)
            return RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED;
    }
    D3D11_TEXTURE2D_DESC descriptor{}; self.output->GetDesc(&descriptor);
    descriptor.Width = width; descriptor.Height = height;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> replacement;
    if (FAILED(self.device->CreateTexture2D(&descriptor, nullptr, &replacement)))
        return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
    if (self.backend == RSF_SR_DLSS && rsf_dlss_release_resources() != RSF_DLSS_OK)
        return RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    auto* previous_output = self.output;
    auto* previous_alternate = self.alternate;
    self.output = replacement.Detach(); self.alternate = next_alternate.release();
    rsf_motion_decode_destroy(self.decode); self.decode = nullptr;
    self.decode_width = self.decode_height = self.decode_failed_width = self.decode_failed_height = 0;
    self.reset_pending = true; self.evaluate_given_up = false; self.consecutive_evaluate_failures = 0;
    {
        std::lock_guard<std::mutex> lock(self.guard);
        self.output_width = width; self.output_height = height;
        commit_range(self, range);
    }
    previous_output->Release(); rsf_sr_legacy_destroy(previous_alternate);
    if (render_width) *render_width = range.width;
    if (render_height) *render_height = range.height;
    say(self, "output resized to %ux%u; backend %u plans %ux%u", width, height, self.backend, range.width, range.height);
    return RSF_DLSS_PIPELINE_OK;
}
catch (...) { return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED; }

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_select_backend(uint32_t backend,
    uint32_t* render_width, uint32_t* render_height)
{
    auto& self = pipeline();
    if (!self.device || !self.output) return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    if (backend < RSF_SR_DLSS || backend > RSF_SR_XESS) return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    { std::lock_guard<std::mutex> lock(self.guard); self.requested_backend = backend; }
    RenderRange range{0, 0, 1, 1, self.output_width, self.output_height};
    rsf_backend_result result = RSF_BACKEND_OK;
    if (backend == RSF_SR_DLSS) {
        rsf_dlss_plan plan{}; plan.struct_size = sizeof(plan);
        plan.output_width = self.output_width; plan.output_height = self.output_height; plan.quality = self.quality;
        result = rsf_dlss_plan_render_size(&plan);
        // Taken as reported, without range_of's zero defaulting.
        range = {plan.render_width, plan.render_height, plan.render_width_min, plan.render_height_min,
                 plan.render_width_max, plan.render_height_max};
        if (result == 0 && self.alternate) result = rsf_sr_legacy_select(self.alternate, RSF_SR_NONE, self.quality, nullptr, nullptr);
        if (result == 0) rsf_dlss_release_resources();
    } else {
        if (!self.alternate) result = create_alternate(self, self.output_width, self.output_height, &self.alternate);
        if (result == 0) result = rsf_sr_legacy_select(self.alternate, backend, self.quality, &range.width, &range.height);
    }
    {
        std::lock_guard<std::mutex> lock(self.guard);
        self.last_switch_result = result;
        if (result == 0) {
            self.backend = backend;
            commit_range(self, range);
        }
    }
    if (result != 0) {
        say(self, "backend %u refused (%d); keeping %u", backend, result, self.backend);
        return RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED;
    }
    self.reset_pending = true; self.evaluate_given_up = false; self.consecutive_evaluate_failures = 0;
    if (render_width) *render_width = range.width;
    if (render_height) *render_height = range.height;
    say(self, "selected backend %u: %ux%u", backend, range.width, range.height);
    return RSF_DLSS_PIPELINE_OK;
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_stop(void)
{
    Pipeline& self = pipeline();
    if (!self.device && !self.streamline_loaded && !self.output) {
        return RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING;
    }
    say(self, "stopping: releasing pipeline resources");
    tear_down(self);
    return RSF_DLSS_PIPELINE_OK;
}
