// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/dlss_pipeline.h>

#include <rescaleframe/texture_dump.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>

namespace {

// Ace Combat 7 is a UE4.18 title, so this identity is simply true. It is not decoration: Streamline
// will not start NGX without an identity, and DLSS is an NGX feature, so with none supplied the
// plugin loads and then refuses with a message that reads exactly like unsupported hardware. An
// injected integration has no NVIDIA-issued application id of its own, since that belongs to the
// game's publisher, so it identifies by engine instead.
const char* const unreal_version = "4.18";
const char* const project_id = "a3ed1f08-3542-4698-b85c-e1a9908e861a";

// One pipeline per process, because the entry points carry no handle: there is one game, one
// device and one Streamline. State is a function-local static rather than a namespace-scope object
// so that nothing here runs before the DLL's first call into it.
struct Pipeline {
    // Owned by the thread that drives frames. start, on_frame and stop are not safe to call
    // concurrently with each other and are not meant to be: they all use the device.
    ID3D11Device* device = nullptr;
    ID3D11Texture2D* output = nullptr;
    rsf_motion_decode* decode = nullptr;
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
    rsf_dlss_pipeline_result reported = RSF_DLSS_PIPELINE_OK;
    uint32_t reported_width = 0;
    uint32_t reported_height = 0;
    bool streamline_loaded = false;
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
    uint64_t frames_evaluated = 0;
    uint64_t frames_refused = 0;
    rsf_dlss_pipeline_result last_result = RSF_DLSS_PIPELINE_OK;
    bool dump_pending = false;
    std::string dump_prefix;
};

Pipeline& pipeline()
{
    static Pipeline instance;
    return instance;
}

void say(Pipeline& self, const char* format, ...)
{
    if (!self.log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    self.log(self.log_user, message);
}

// Streamline and the decode pass each take their own sink. Both are forwarded to ours with a
// prefix rather than being given to the caller separately, so that one log carries the whole
// sequence in the order it happened. The message goes through as an argument, never as the format,
// because a vendor's line may contain a percent sign.
void from_dlss(void* user, const char* message)
{
    (void)user;
    say(pipeline(), "dlss: %s", message ? message : "");
}

void from_decode(void* user, const char* message)
{
    (void)user;
    say(pipeline(), "motion decode: %s", message ? message : "");
}

void from_dump(void* user, const char* message)
{
    (void)user;
    say(pipeline(), "dump: %s", message ? message : "");
}

// Whether this failure is worth a line, which it is the first time and again whenever the failure
// or the render size it concerns changes. Called only from the thread that drives frames, which is
// the only one that touches the fields it reads.
bool worth_saying(Pipeline& self, rsf_dlss_pipeline_result result, uint32_t width, uint32_t height)
{
    if (self.reported == result && self.reported_width == width &&
        self.reported_height == height) {
        return false;
    }
    self.reported = result;
    self.reported_width = width;
    self.reported_height = height;
    return true;
}

// Undo whatever is currently held, in the reverse of the order it was acquired. Safe to call at any
// point in a partially completed start, which is what the failure paths there rely on.
void tear_down(Pipeline& self)
{
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
    if (self.streamline_loaded) {
        say(self, "shutting streamline down");
        // Frees the DLSS feature's own resources first. Streamline's shutdown does this too, but
        // the order matters when the caller's device is about to go and this makes it explicit.
        rsf_dlss_release_resources();
        rsf_dlss_shutdown();
        self.streamline_loaded = false;
    }
    if (self.device) {
        self.device->Release();
        self.device = nullptr;
    }

    // Dropped after the last line above, and dropped at all because the DLSS backend keeps
    // `from_dlss` as its own sink for the life of the process and that forwards to here. Leaving
    // these set would hand a later vendor line a `log_user` the caller stopped owning when it
    // stopped the pipeline.
    self.log = nullptr;
    self.log_user = nullptr;
    self.reported = RSF_DLSS_PIPELINE_OK;
    self.reported_width = 0;
    self.reported_height = 0;

    std::lock_guard<std::mutex> lock(self.guard);
    self.running = false;
    self.supported = false;
    self.dump_pending = false;
}

// Record the outcome of a frame and hand it back. Every frame that did not reach a successful
// evaluate counts as refused, whether this code refused it or DLSS did, so that the two counters
// sum to the well formed frames offered while running and a caller cannot report activity that did
// not happen. A call that carried no frame at all, or the wrong ABI, is counted as neither.
rsf_dlss_pipeline_result finish(Pipeline& self, rsf_dlss_pipeline_result result)
{
    if (result == RSF_DLSS_PIPELINE_OK) {
        // A failure that follows a good frame is news again, so the suppression above only ever
        // covers an unbroken run of the same one.
        self.reported = RSF_DLSS_PIPELINE_OK;
        self.reported_width = 0;
        self.reported_height = 0;
    }
    std::lock_guard<std::mutex> lock(self.guard);
    if (result == RSF_DLSS_PIPELINE_OK) {
        ++self.frames_evaluated;
    } else {
        ++self.frames_refused;
    }
    self.last_result = result;
    return result;
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
    dlss.engine = RSF_DLSS_ENGINE_UNREAL;
    dlss.engine_version_utf8 = unreal_version;
    dlss.project_id_utf8 = project_id;
    dlss.require_signature = setup->require_signature;
    dlss.log = from_dlss;

    const rsf_dlss_result loaded = rsf_dlss_load(&dlss);
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
    D3D11_TEXTURE2D_DESC description{};
    description.Width = setup->output_width;
    description.Height = setup->output_height;
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
    if (FAILED(device->CreateTexture2D(&description, nullptr, &self.output)) || !self.output) {
        say(self, "the output target could not be created");
        tear_down(self);
        return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
    }

    // Cleared once, because a fresh texture holds whatever was in that memory and there is no
    // promise that a reconstruction writes every pixel of it. It does not: a frame whose render
    // size differs from the one the feature was built for left a corner of this target untouched,
    // and the previous tenant of that memory showed through as blocks. Black there is honest,
    // where blocks read as an artifact of the reconstruction rather than as an absence of one.
    {
        ID3D11DeviceContext* context = nullptr;
        ID3D11RenderTargetView* target = nullptr;
        device->GetImmediateContext(&context);
        if (context && SUCCEEDED(device->CreateRenderTargetView(self.output, nullptr, &target)) &&
            target) {
            const FLOAT black[4] = {0.0f, 0.0f, 0.0f, 1.0f};
            context->ClearRenderTargetView(target, black);
            target->Release();
        }
        if (context) {
            context->Release();
        }
    }

    {
        std::lock_guard<std::mutex> lock(self.guard);
        self.supported = true;
        self.output_width = setup->output_width;
        self.output_height = setup->output_height;
        self.quality = setup->quality;
        self.planned_render_width = plan.render_width;
        self.planned_render_height = plan.render_height;
        // A build of the SDK that reports no range is taken to accept the optimal size only,
        // rather than being read as "any size".
        self.render_width_min = plan.render_width_min ? plan.render_width_min : plan.render_width;
        self.render_height_min =
            plan.render_height_min ? plan.render_height_min : plan.render_height;
        self.render_width_max = plan.render_width_max ? plan.render_width_max : plan.render_width;
        self.render_height_max =
            plan.render_height_max ? plan.render_height_max : plan.render_height;
        self.frames_evaluated = 0;
        self.frames_refused = 0;
        self.last_result = RSF_DLSS_PIPELINE_OK;
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
    if (!frame->scene_color || !frame->depth || !frame->game_motion || !frame->camera ||
        frame->render_width == 0 || frame->render_height == 0) {
        return finish(self, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    }
    // Checked before the copy below, which reads a whole structure out of the caller's memory.
    if (frame->camera->struct_size < sizeof(rsf_camera_frame)) {
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
        first = self.frames_evaluated == 0 && self.frames_refused == 0;
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
        if (worth_saying(self, RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT, frame->render_width,
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
        // Zero means R16G16_FLOAT, which is what backends expect and what the default sentinel was
        // chosen to be representable in.
        decode.output_format = 0;
        decode.log = from_decode;
        const rsf_motion_decode_result built =
            rsf_motion_decode_create(self.device, &decode, &self.decode);
        if (built != RSF_MOTION_DECODE_OK || !self.decode) {
            if (worth_saying(self, RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED, frame->render_width,
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
        if (worth_saying(self, RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED, frame->render_width,
                         frame->render_height)) {
            say(self, "the motion decode did not run (result %d)", int(decoded));
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED);
    }

    // Only the fields this pipeline is the one to know. The matrices, the jitter and the camera
    // basis come from whatever read the game's view buffer and are used exactly as given: a value
    // invented here would produce a plausible image of a camera that was never rendered, which
    // reads as softness rather than as a bug.
    rsf_camera_frame camera = *frame->camera;
    camera.struct_size = uint32_t(sizeof(rsf_camera_frame));
    camera.motion_decoded = 1u;
    // After the decode a stored zero is an ordinary value that real motion can take, so the marker
    // for "nothing wrote this pixel" is whatever the decode wrote instead, and only exists if the
    // encoding reserved a clear value in the first place.
    camera.has_motion_sentinel = self.motion.zero_means_unwritten ? 1u : 0u;
    camera.motion_sentinel = self.motion.invalid_value;
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
    if (rebuilt) {
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
        if (worth_saying(self, RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED, frame->render_width,
                         frame->render_height)) {
            say(self, "the frame was refused before DLSS saw it (result %d)", int(assembled));
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED);
    }

    const rsf_dlss_result evaluated = rsf_dlss_evaluate(context_pointer, &dlss_frame);
    if (evaluated != RSF_DLSS_OK) {
        if (worth_saying(self, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED, frame->render_width,
                         frame->render_height)) {
            say(self, "DLSS did not evaluate this frame (result %d)", int(evaluated));
        }
        return finish(self, RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED);
    }
    if (first) {
        // Deliberately not "DLSS is working". Streamline accepted the inputs and wrote the output
        // target; whether the image in it is correct is a separate question that only looking at
        // one answers, which is what the dump below is for.
        say(self, "DLSS evaluated a frame. This says the inputs were accepted, not that the image "
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
    if (!prefix.empty()) {
        // Both sides of the comparison, from the same frame. An upscaled image on its own says
        // nothing: the question is whether it is this scene, sharper, and that needs the input it
        // was made from rather than a different frame's.
        const std::string input_prefix = prefix + "_input";
        const std::string output_prefix = prefix + "_output";
        const struct {
            const char* what;
            const std::string& path;
            void* texture;
        } targets[] = {
            {"the scene colour it was given", input_prefix, frame->scene_color},
            {"the upscaled result", output_prefix, self.output},
        };

        for (const auto& target : targets) {
            say(self, "writing %s to %s", target.what, target.path.c_str());
            rsf_texture_dump_options options{};
            options.struct_size = uint32_t(sizeof(options));
            options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
            options.output_prefix_utf8 = target.path.c_str();
            options.view = RSF_DUMP_VIEW_RAW;
            options.scale = 1.0f;
            options.log = from_dump;
            rsf_texture_dump_report report{};
            report.struct_size = uint32_t(sizeof(report));
            const rsf_dump_texture_result written =
                rsf_dump_texture(self.device, context_pointer, target.texture, &options, &report);
            if (written != RSF_TEXTURE_OK) {
                say(self, "writing %s failed (result %d)", target.what, int(written));
            }
        }
        // A failed dump is diagnostic only and does not change the frame's outcome.
    }

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
    status->frames_evaluated = self.frames_evaluated;
    status->frames_refused = self.frames_refused;
    status->last_result = self.last_result;
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
