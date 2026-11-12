// SPDX-License-Identifier: GPL-3.0-only
// rsf_get_d3d11_sr_api, the generic Runtime export the Anomaly SR Stage 5 adapter borrows, queried from a client that
// imports ReScaleFrame.Runtime.dll the way a DLL-loaded adapter does (no OrchestratorCore, no RSF_RUNTIME_BUILD).
// Checks the size/version refusals (nothing written on failure), the filled table, the idle pipeline's answers, the
// argument refusals of every entry that can be called without a device, and the adapter's own table validation
// (sr_temporal.cpp bind_sr_api) on the real table and on tampered copies. No GPU and no vendor runtime is needed.
#include "sr_temporal.h"
#include <rescaleframe/d3d11_sr_api.h>

#include <windows.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>

namespace {
using namespace rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

void query() {
    CHECK(rsf_get_d3d11_sr_api(nullptr) == RSF_ERROR_INVALID_ARGUMENT);
    // Short capacity: refused before abi_version is read, nothing written.
    unsigned char buffer[sizeof(rsf_d3d11_sr_api) + 16];
    std::memset(buffer, 0xcd, sizeof(buffer));
    auto* table = reinterpret_cast<rsf_d3d11_sr_api*>(buffer);
    table->struct_size = sizeof(rsf_d3d11_sr_api) - 1;
    CHECK(rsf_get_d3d11_sr_api(table) == RSF_ERROR_INVALID_ARGUMENT);
    for (std::size_t i = sizeof(uint32_t); i < sizeof(buffer); ++i) CHECK(buffer[i] == 0xcd);
    // Wrong version at full capacity: ABI_MISMATCH, nothing written but the caller's own two fields.
    table->struct_size = sizeof(rsf_d3d11_sr_api);
    table->abi_version = RSF_D3D11_SR_API_VERSION + 1;
    CHECK(rsf_get_d3d11_sr_api(table) == RSF_ERROR_ABI_MISMATCH);
    for (std::size_t i = 2 * sizeof(uint32_t); i < sizeof(buffer); ++i) CHECK(buffer[i] == 0xcd);
    // Larger capacity: the known prefix is filled, struct_size is the Runtime's, bytes beyond are untouched.
    table->struct_size = sizeof(buffer);
    table->abi_version = RSF_D3D11_SR_API_VERSION;
    CHECK(rsf_get_d3d11_sr_api(table) == RSF_OK);
    CHECK(table->struct_size == sizeof(rsf_d3d11_sr_api) && table->abi_version == RSF_D3D11_SR_API_VERSION);
    CHECK(table->pipeline_abi_version == RSF_DLSS_PIPELINE_ABI_VERSION && RSF_DLSS_PIPELINE_ABI_VERSION == 5u);
    for (std::size_t i = sizeof(rsf_d3d11_sr_api); i < sizeof(buffer); ++i) CHECK(buffer[i] == 0xcd);
    CHECK(table->start && table->evaluate && table->reset && table->stop && table->status && table->select_backend &&
        table->set_quality && table->set_preset && table->resize_output && table->output_texture && table->request_dump);
    // The entry point is what a carrier-loaded adapter resolves by name.
    const HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    CHECK(runtime != nullptr);
    const auto resolved = reinterpret_cast<rsf_get_d3d11_sr_api_fn>(reinterpret_cast<void*>(GetProcAddress(runtime, RSF_D3D11_SR_API_ENTRY_POINT)));
    CHECK(resolved == &rsf_get_d3d11_sr_api || resolved != nullptr);
}

void idle_pipeline() {
    rsf_d3d11_sr_api api{sizeof(api), RSF_D3D11_SR_API_VERSION};
    CHECK(rsf_get_d3d11_sr_api(&api) == RSF_OK);
    // Status: refuses a short structure, reports a stopped pipeline.
    rsf_dlss_pipeline_status status{};
    status.struct_size = sizeof(status) - 4;
    CHECK(api.status(&status) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    status.struct_size = sizeof(status);
    CHECK(api.status(&status) == RSF_DLSS_PIPELINE_OK && status.running == 0 && status.frames_evaluated == 0);
    CHECK(api.stop() == RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING);
    CHECK(api.output_texture() == nullptr);
    CHECK(api.reset() == RSF_DLSS_PIPELINE_OK);
    CHECK(api.request_dump(nullptr) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    CHECK(api.request_dump("x") == RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING);
    CHECK(api.select_backend(RSF_SR_XESS, nullptr, nullptr) == RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING);
    CHECK(api.resize_output(1920, 1080, nullptr, nullptr) == RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING);
    CHECK(api.resize_output(0, 1080, nullptr, nullptr) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    CHECK(api.set_quality(5, nullptr, nullptr) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    CHECK(api.set_preset(0) == RSF_DLSS_PIPELINE_OK);
    // Start: a short setup or a missing device is INVALID_ARGUMENT, another pipeline ABI is ABI_MISMATCH. Nothing starts.
    rsf_dlss_pipeline_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.streamline_directory_utf8 = "Z:\\does-not-exist";
    setup.output_width = 64; setup.output_height = 64;
    setup.motion = motion_decode_params();
    int fake_device = 0;
    CHECK(api.start(nullptr, &setup) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    setup.struct_size = sizeof(setup) - sizeof(uint32_t);   // an ABI 4 caller: no standard_depth
    CHECK(api.start(&fake_device, &setup) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    setup.struct_size = sizeof(setup);
    setup.abi_version = 4;
    CHECK(api.start(&fake_device, &setup) == RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH);
    setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.motion.scale_x = 0;   // a zero decode scale is refused before anything loads
    CHECK(api.start(&fake_device, &setup) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    status.struct_size = sizeof(status);
    CHECK(api.status(&status) == RSF_DLSS_PIPELINE_OK && status.running == 0);
    // Evaluate: null context or pass, and a short pass, are refused before the structure is read.
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass) - 8;
    CHECK(api.evaluate(nullptr, &pass) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    CHECK(api.evaluate(&fake_device, nullptr) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    CHECK(api.evaluate(&fake_device, &pass) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    // A full pass without resources is refused by rsf_native_sr_evaluate's own checks, still without touching a device.
    pass.struct_size = sizeof(pass);
    pass.role = RSF_GAME_RENDER_SR;
    CHECK(api.evaluate(&fake_device, &pass) == RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT);
    CHECK(api.stop() == RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING);
}

void adapter_binding() {
    rsf_d3d11_sr_api real{sizeof(real), RSF_D3D11_SR_API_VERSION};
    CHECK(rsf_get_d3d11_sr_api(&real) == RSF_OK);
    rsf_d3d11_sr_api out{};
    const char* reason = nullptr;
    CHECK(bind_sr_api(nullptr, out, &reason) == ApiStatus::Missing && reason);
    CHECK(bind_sr_api(&rsf_get_d3d11_sr_api, out, &reason) == ApiStatus::Bound && out.start == real.start && out.evaluate == real.evaluate);
    // A Runtime of another version refuses the query.
    CHECK(bind_sr_api([](rsf_d3d11_sr_api*) -> rsf_result { return RSF_ERROR_ABI_MISMATCH; }, out, &reason) == ApiStatus::Refused &&
        std::string_view{reason}.find("version") != std::string_view::npos);
    // Tampered tables: each is refused and leaves `out` untouched.
    static rsf_d3d11_sr_api tampered;
    const auto check_refused = [&](void (*tamper)(rsf_d3d11_sr_api&)) {
        tampered = real;
        tamper(tampered);
        rsf_d3d11_sr_api untouched{};
        untouched.struct_size = 77;
        const bool refused = bind_sr_api([](rsf_d3d11_sr_api* t) -> rsf_result { *t = tampered; return RSF_OK; }, untouched, &reason) ==
            ApiStatus::Refused;
        CHECK(refused && untouched.struct_size == 77 && !untouched.start);
    };
    check_refused([](rsf_d3d11_sr_api& t) { t.struct_size = sizeof(t) - 8; });
    check_refused([](rsf_d3d11_sr_api& t) { t.abi_version = 99; });
    check_refused([](rsf_d3d11_sr_api& t) { t.pipeline_abi_version = 4; });
    check_refused([](rsf_d3d11_sr_api& t) { t.evaluate = nullptr; });
    check_refused([](rsf_d3d11_sr_api& t) { t.reset = nullptr; });
    check_refused([](rsf_d3d11_sr_api& t) { t.request_dump = nullptr; });
    // The driver binds the real table and, unstarted, reports no SR output and refuses a pass as not running.
    TemporalDriver driver;
    TemporalOptions options{};
    options.stage = 5;
    driver.configure(options, nullptr, nullptr);
    CHECK(driver.bind_table(real) && !driver.running());
    rsf_overlay_sr_report report{};
    CHECK(driver.overlay_report(report) && report.effective_backend == 0 && report.struct_size == sizeof(report) && report.reason[0]);
    PassInputs in{};
    CHECK(!driver.evaluate(nullptr, in, {}));
    CHECK(driver.counters().refused[static_cast<std::size_t>(Refusal::NotRunning)] == 1);
    uint32_t w{}, h{};
    CHECK(!driver.planned_render(1920, 1080, w, h));
    // Settings the adapter cannot apply keep the current value; SR=0 is honoured.
    driver.request({false, 2, 4, 0});
    CHECK(!driver.enabled() && driver.status().requested.backend == backend_dlss && driver.status().requested.quality == 1);
    TemporalDriver refused;
    refused.configure(options, nullptr, nullptr);
    tampered = real;
    tampered.stop = nullptr;
    CHECK(!refused.bind_table(tampered) && !refused.status().bound && !refused.status().reason.empty());
}
}

// A fake Runtime table that records what the driver asks of it.
struct Fake {
    rsf_dlss_pipeline_setup setup{};
    std::string streamline, fidelityfx, xess, project, engine_version;
    void* device{};
    uint32_t starts{}, stops{}, selects{}, qualities{}, presets{}, resizes{}, evaluates{};
    uint32_t backend{RSF_SR_DLSS}, quality{1}, preset{}, output_width{}, output_height{}, render_width{}, render_height{};
    bool running{}, refuse_fsr4{true};
    rsf_dlss_pipeline_result start_result{RSF_DLSS_PIPELINE_OK};
    rsf_dlss_pipeline_result evaluate_result{RSF_DLSS_PIPELINE_OK};
    rsf_game_render_pass last{};
    static Fake* self;
};
Fake* Fake::self;
void plan(Fake& f) {
    const double ratio[] = {1.0, 1.5, 1.7, 2.0, 3.0};
    f.render_width = uint32_t(f.output_width / ratio[f.quality] + 0.5);
    f.render_height = uint32_t(f.output_height / ratio[f.quality] + 0.5);
}
rsf_d3d11_sr_api fake_table() {
    rsf_d3d11_sr_api t{sizeof(t), RSF_D3D11_SR_API_VERSION, RSF_DLSS_PIPELINE_ABI_VERSION};
    t.start = [](void* device, const rsf_dlss_pipeline_setup* s) -> rsf_dlss_pipeline_result {
        auto& f = *Fake::self;
        ++f.starts;
        f.setup = *s;
        f.streamline = s->streamline_directory_utf8; f.fidelityfx = s->fsr3_directory_utf8; f.xess = s->xess_directory_utf8;
        f.project = s->project_id_utf8; f.engine_version = s->engine_version_utf8;
        if (f.start_result != RSF_DLSS_PIPELINE_OK) return f.start_result;
        f.device = device; f.running = true; f.backend = RSF_SR_DLSS; f.quality = s->quality;
        f.output_width = s->output_width; f.output_height = s->output_height;
        plan(f);
        return RSF_DLSS_PIPELINE_OK;
    };
    t.evaluate = [](void*, const rsf_game_render_pass* pass) -> rsf_dlss_pipeline_result {
        auto& f = *Fake::self;
        ++f.evaluates;
        f.last = *pass;
        return f.evaluate_result;
    };
    t.reset = []() -> rsf_dlss_pipeline_result { return RSF_DLSS_PIPELINE_OK; };
    t.stop = []() -> rsf_dlss_pipeline_result { auto& f = *Fake::self; ++f.stops; f.running = false; return RSF_DLSS_PIPELINE_OK; };
    t.status = [](rsf_dlss_pipeline_status* s) -> rsf_dlss_pipeline_result {
        auto& f = *Fake::self;
        *s = {};
        s->struct_size = sizeof(*s);
        s->running = f.running; s->dlss_supported = f.running;
        s->render_width = f.render_width; s->render_height = f.render_height;
        s->output_width = f.output_width; s->output_height = f.output_height;
        s->backend = f.backend; s->requested_backend = f.backend;
        return RSF_DLSS_PIPELINE_OK;
    };
    t.select_backend = [](uint32_t backend, uint32_t*, uint32_t*) -> rsf_dlss_pipeline_result {
        auto& f = *Fake::self;
        ++f.selects;
        if (backend == RSF_SR_FSR4 && f.refuse_fsr4) return RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED;
        f.backend = backend;
        return RSF_DLSS_PIPELINE_OK;
    };
    t.set_quality = [](uint32_t quality, uint32_t*, uint32_t*) -> rsf_dlss_pipeline_result {
        auto& f = *Fake::self; ++f.qualities; f.quality = quality; plan(f); return RSF_DLSS_PIPELINE_OK;
    };
    t.set_preset = [](uint32_t preset) -> rsf_dlss_pipeline_result { auto& f = *Fake::self; ++f.presets; f.preset = preset; return RSF_DLSS_PIPELINE_OK; };
    t.resize_output = [](uint32_t w, uint32_t h, uint32_t*, uint32_t*) -> rsf_dlss_pipeline_result {
        auto& f = *Fake::self; ++f.resizes; f.output_width = w; f.output_height = h; plan(f); return RSF_DLSS_PIPELINE_OK;
    };
    t.output_texture = []() -> void* { return nullptr; };
    t.request_dump = [](const char*) -> rsf_dlss_pipeline_result { return RSF_DLSS_PIPELINE_OK; };
    return t;
}
// A Stage 5 frame as the seam builds it: unjittered X-Ray projection, jitter applied at the planned size.
PassInputs frame(uint32_t rw, uint32_t rh, uint64_t index, bool sequence_reset) {
    static int resources[4];
    PassInputs in{};
    in.native_frame = index;
    in.render_width = rw; in.render_height = rh; in.output_width = 1920; in.output_height = 1080;
    const float h = 1.0f / std::tan(0.6f), w = h / (16.0f / 9.0f), q = 900.0f / (900.0f - 0.2f);
    const Matrix p{w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * 0.2f, 0};
    const Matrix v{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
    in.current = in.previous = {v, p, true};
    in.jitter.applied = true; in.jitter.render_width = rw; in.jitter.render_height = rh; in.jitter.current_x = 0.25f;
    in.jitter.reset = sequence_reset;
    in.color = &resources[0]; in.color_output = &resources[1]; in.depth = &resources[2]; in.motion = &resources[3];
    return in;
}

void driver_lifecycle() {
    Fake fake;
    Fake::self = &fake;
    const auto table = fake_table();
    TemporalDriver driver;
    TemporalOptions options{};
    options.stage = 5;
    options.streamline = "C:\\sl"; options.fidelityfx = "C:\\ffx"; options.xess = "C:\\xess";
    options.initial = {true, backend_dlss, 1, 0};
    driver.configure(options, nullptr, nullptr);
    CHECK(driver.bind_table(table));
    int device_a = 0, device_b = 0;
    auto* a = reinterpret_cast<ID3D11Device*>(&device_a);
    auto* b = reinterpret_cast<ID3D11Device*>(&device_b);
    // First boundary: one start with the adapter's identity, motion contract and depth convention.
    driver.boundary(a, 1920, 1080);
    CHECK(fake.starts == 1 && driver.running() && fake.device == &device_a);
    const auto& s = fake.setup;
    CHECK(s.struct_size == sizeof(s) && s.abi_version == RSF_DLSS_PIPELINE_ABI_VERSION && s.engine == RSF_DLSS_ENGINE_CUSTOM &&
        s.view_space_to_meters == 1.0f && s.standard_depth == 1 && fake.project == project_id && fake.engine_version == engine_version);
    CHECK(fake.streamline == "C:\\sl" && fake.fidelityfx == "C:\\ffx" && std::string_view{s.fsr4_directory_utf8} == "C:\\ffx" &&
        fake.xess == "C:\\xess" && std::string_view{s.fsr2_directory_utf8}.empty() && s.output_width == 1920 && s.quality == 1);
    CHECK(s.motion.validity_enabled == 1 && s.motion.validity_channel == 2 && s.motion.zero_means_unwritten == 0 && s.motion.scale_x == 1);
    CHECK(fake.selects == 0 && fake.presets == 0);   // DLSS and preset 0 are the pipeline's start state
    uint32_t rw{}, rh{};
    CHECK(driver.planned_render(1920, 1080, rw, rh) && rw == 1280 && rh == 720);
    CHECK(!driver.planned_render(2560, 1440, rw, rh));
    driver.boundary(a, 1920, 1080);
    CHECK(fake.starts == 1 && fake.selects == 0);    // nothing to apply
    // First pass: reset (first), consecutive frames: none, a jitter-sequence restart: sequence, a cut: cut.
    auto in = frame(1280, 720, 1, false);
    CHECK(driver.evaluate(nullptr, in, {}) && (fake.last.flags & RSF_GAME_RENDER_RESET) && fake.last.native_frame == 1);
    in = frame(1280, 720, 2, false);
    CHECK(driver.evaluate(nullptr, in, {}) && !(fake.last.flags & RSF_GAME_RENDER_RESET));
    in = frame(1280, 720, 3, true);
    ResetFacts facts{};
    facts.sequence = in.jitter.reset;
    CHECK(driver.evaluate(nullptr, in, facts) && (fake.last.flags & RSF_GAME_RENDER_RESET));
    in = frame(1280, 720, 4, false);
    facts = {};
    facts.cut = true;
    CHECK(driver.evaluate(nullptr, in, facts) && (fake.last.flags & RSF_GAME_RENDER_RESET));
    auto c = driver.counters();
    CHECK(c.evaluated[backend_dlss] == 4 && c.resets[size_t(ResetReason::First)] == 1 && c.resets[size_t(ResetReason::Sequence)] == 1 &&
        c.resets[size_t(ResetReason::Cut)] == 1);
    rsf_overlay_sr_report report{};
    CHECK(driver.overlay_report(report) && report.effective_backend == backend_dlss && report.render_width == 1280 &&
        report.effective_quality == 1 && (report.inputs & RSF_OVERLAY_SR_INPUT_EXPOSURE) == 0 && report.reason[0] == '\0');
    // Settings at the boundary: XeSS and Balanced, preset forwarded as chosen; a settings reset on the next pass.
    driver.request({true, backend_xess, 2, 4});
    CHECK(fake.selects == 0);                        // nothing until the boundary
    driver.boundary(a, 1920, 1080);                  // the first use of XeSS is announced, quality follows at once
    CHECK(fake.selects == 0 && driver.status().pending_backend == backend_xess && fake.qualities == 1);
    driver.boundary(a, 1920, 1080);                  // and selected at the next boundary
    CHECK(fake.selects == 1 && fake.backend == RSF_SR_XESS && fake.qualities == 1 && fake.quality == 2 && fake.presets == 1 && fake.preset == 4);
    CHECK(driver.planned_render(1920, 1080, rw, rh) && rw == 1129 && rh == 635);
    in = frame(1129, 635, 5, false);
    CHECK(driver.evaluate(nullptr, in, {}) && (fake.last.flags & RSF_GAME_RENDER_RESET));
    c = driver.counters();
    CHECK(c.evaluated[backend_xess] == 1 && c.resets[size_t(ResetReason::Settings)] == 1);
    // A backend the GPU refuses is kept out, reported, and not asked for again every frame.
    driver.request({true, backend_fsr4, 2, 4});
    driver.boundary(a, 1920, 1080);
    driver.boundary(a, 1920, 1080);
    driver.boundary(a, 1920, 1080);
    CHECK(fake.selects == 2 && fake.backend == RSF_SR_XESS && driver.counters().select_failures == 1);
    in = frame(1129, 635, 6, false);
    CHECK(driver.evaluate(nullptr, in, {}) && driver.overlay_report(report) && report.effective_backend == backend_xess &&
        std::string_view{report.reason}.find("FSR4") != std::string_view::npos);
    // SR off: no pass is asked for (the world window stays closed); the pipeline keeps running.
    driver.request({false, backend_xess, 2, 4});
    driver.boundary(a, 1920, 1080);
    CHECK(!driver.enabled() && driver.running() && fake.selects == 2);
    driver.request({true, backend_xess, 2, 4});
    driver.boundary(a, 1920, 1080);
    CHECK(driver.enabled());
    // Resolution change: the next frame's render size is planned for the new output at the boundary.
    driver.boundary(a, 2560, 1440);
    CHECK(fake.resizes == 1 && driver.planned_render(2560, 1440, rw, rh) && rw == 1506);
    // A refused pass is counted with its result, and does not consume the pending reset.
    fake.evaluate_result = RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED;
    in = frame(1506, 847, 7, false);
    CHECK(!driver.evaluate(nullptr, in, {}) && driver.counters().results[9] == 1 &&
        driver.counters().refused[size_t(Refusal::Pipeline)] == 1);
    CHECK(driver.overlay_report(report) && report.effective_backend == 0 && std::string_view{report.reason}.find("refused") != std::string_view::npos);
    fake.evaluate_result = RSF_DLSS_PIPELINE_OK;
    // Build refusals never reach the Runtime.
    const auto before = fake.evaluates;
    in = frame(1506, 847, 8, false);
    in.jitter.applied = false;
    CHECK(!driver.evaluate(nullptr, in, {}) && fake.evaluates == before &&
        driver.counters().pass_errors[size_t(PassError::Jitter)] == 1);
    // A new engine device: stop, then start on it, and the first pass after is a device reset.
    driver.boundary(b, 2560, 1440);
    CHECK(fake.stops == 1 && fake.starts == 2 && fake.device == &device_b && fake.backend == RSF_SR_XESS && fake.quality == 2);
    in = frame(1506, 847, 9, false);
    CHECK(driver.evaluate(nullptr, in, {}) && (fake.last.flags & RSF_GAME_RENDER_RESET));
    c = driver.counters();
    CHECK(c.resets[size_t(ResetReason::First)] == 2);   // a new pipeline session counts as first
    // Teardown.
    driver.stop();
    CHECK(fake.stops == 2 && !driver.running() && !driver.planned_render(2560, 1440, rw, rh));
    in = frame(1506, 847, 10, false);
    CHECK(!driver.evaluate(nullptr, in, {}) && driver.counters().refused[size_t(Refusal::NotRunning)] == 1);
    // The periodic log lines.
    std::vector<std::string> lines;
    driver.report([](void* user, const char* line) { static_cast<std::vector<std::string>*>(user)->push_back(line); }, &lines);
    CHECK(lines.size() == 2 && lines[0].find("evaluated dlss=4 fsr3=0 fsr4=0 xess=3") != std::string::npos &&
        lines[1].find("pipeline bound=1 running=0") != std::string::npos);
    // A Stage 4 driver never starts the pipeline.
    TemporalDriver inputs_only;
    options.stage = 4;
    inputs_only.configure(options, nullptr, nullptr);
    CHECK(inputs_only.bind_table(table));
    const auto starts = fake.starts;
    inputs_only.boundary(a, 1920, 1080);
    CHECK(fake.starts == starts && !inputs_only.running());
}

// A start the pipeline refuses (wave 7: Streamline could not load its plugins from a "\\?\" directory) is never a silent
// bilinear frame: the log names the directories and keys, the overlay reason says SR is off and why.
void start_refused_loudly() {
    Fake fake;
    Fake::self = &fake;
    fake.start_result = RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED;
    const auto table = fake_table();
    TemporalDriver driver;
    TemporalOptions options{};
    options.stage = 5;
    options.streamline = "D:\\Anomaly 1.5.3\\bin\\ReScaleFrame\\streamline"; options.fidelityfx = "C:\\ffx"; options.xess = "C:\\xess";
    options.initial = {true, backend_dlss, 1, 0};
    std::vector<std::string> lines;
    driver.configure(options, [](void* user, const char* line) { static_cast<std::vector<std::string>*>(user)->push_back(line); }, &lines);
    CHECK(driver.bind_table(table));
    int device = 0;
    driver.boundary(reinterpret_cast<ID3D11Device*>(&device), 1920, 1080);
    CHECK(fake.starts == 1 && !driver.running() && driver.counters().start_failures == 1);
    bool named = false, directories = false;
    for (const auto& line : lines) {
        named = named || line.find("did not start (-5, Streamline failed)") != std::string::npos;
        directories = directories || (line.find("SR is OFF") != std::string::npos &&
            line.find("D:\\Anomaly 1.5.3\\bin\\ReScaleFrame\\streamline") != std::string::npos && line.find("SRStreamlineDir") != std::string::npos);
    }
    CHECK(named && directories);
    rsf_overlay_sr_report report{};
    CHECK(driver.overlay_report(report) && report.effective_backend == 0);
    const std::string_view reason{report.reason};
    CHECK(reason.find("SR off") != std::string_view::npos && reason.find("Streamline") != std::string_view::npos &&
        reason.find("SRStreamlineDir") != std::string_view::npos && reason.size() < sizeof(report.reason) - 1);
    CHECK(start_refusal_reason(RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED).find("DLSS support") != std::string::npos);
    CHECK(start_refusal_reason(RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED).find("resource failed") != std::string::npos);
    for (const int32_t r : {int32_t(RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED), int32_t(RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED),
             int32_t(RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT), int32_t(RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED)})
        CHECK(start_refusal_reason(r).size() < sizeof(report.reason));
    // Not retried every frame for the same device and settings.
    driver.boundary(reinterpret_cast<ID3D11Device*>(&device), 1920, 1080);
    CHECK(fake.starts == 1);
}

// The first request for a backend the vendor has not loaded in this process (FSR4's first dispatch builds pipelines for tens of seconds on a
// cold driver cache) only announces it: the status names it and the Present draws that, then the blocking select runs at the next boundary.
// Later selections run at once.
void announce_before_select() {
    Fake fake;
    Fake::self = &fake;
    fake.refuse_fsr4 = false;
    const auto table = fake_table();
    TemporalDriver driver;
    TemporalOptions options{};
    options.stage = 5;
    options.streamline = "C:\\sl"; options.fidelityfx = "C:\\ffx"; options.xess = "C:\\xess";
    options.initial = {true, backend_dlss, 1, 0};
    std::vector<std::string> lines;
    driver.configure(options, [](void* user, const char* line) { static_cast<std::vector<std::string>*>(user)->push_back(line); }, &lines);
    CHECK(driver.bind_table(table));
    int device = 0;
    auto* a = reinterpret_cast<ID3D11Device*>(&device);
    driver.boundary(a, 1920, 1080);
    auto in = frame(1280, 720, 1, false);
    CHECK(driver.evaluate(nullptr, in, {}));
    // FSR4 requested: announced, not selected; the seam keeps evaluating with DLSS and the overlay names what is coming.
    driver.request({true, backend_fsr4, 1, 0});
    driver.boundary(a, 1920, 1080);
    CHECK(fake.selects == 0 && driver.status().applied.backend == backend_dlss && driver.status().pending_backend == backend_fsr4);
    CHECK(driver.counters().select_announced == 1);
    in = frame(1280, 720, 2, false);
    CHECK(driver.evaluate(nullptr, in, {}) && !(fake.last.flags & RSF_GAME_RENDER_RESET));
    rsf_overlay_sr_report report{};
    CHECK(driver.overlay_report(report) && report.effective_backend == backend_dlss);
    const std::string_view preparing{report.reason};
    CHECK(preparing.find("Preparing FSR4") != std::string_view::npos && preparing.find("pauses") != std::string_view::npos &&
        preparing.size() < sizeof(report.reason) - 1);
    // The next boundary runs the blocking select, once, with a settings reset on the next pass; the status is clear again.
    driver.boundary(a, 1920, 1080);
    CHECK(fake.selects == 1 && fake.backend == RSF_SR_FSR4 && driver.status().applied.backend == backend_fsr4 && driver.status().pending_backend == 0);
    in = frame(1280, 720, 3, false);
    CHECK(driver.evaluate(nullptr, in, {}) && (fake.last.flags & RSF_GAME_RENDER_RESET));
    CHECK(driver.overlay_report(report) && report.effective_backend == backend_fsr4 && report.reason[0] == '\0');
    // Prepared once in this process: switching away and back is not announced again, and DLSS never is.
    driver.request({true, backend_dlss, 1, 0});
    driver.boundary(a, 1920, 1080);
    CHECK(fake.selects == 2 && driver.status().pending_backend == 0);
    driver.request({true, backend_fsr4, 1, 0});
    driver.boundary(a, 1920, 1080);
    CHECK(fake.selects == 3 && driver.status().pending_backend == 0 && driver.counters().select_announced == 1);
    // A wish that changes after the announcement cancels it; one back to the applied backend selects nothing.
    driver.request({true, backend_xess, 1, 0});
    driver.boundary(a, 1920, 1080);
    CHECK(driver.status().pending_backend == backend_xess && fake.selects == 3);
    driver.request({true, backend_fsr4, 1, 0});
    driver.boundary(a, 1920, 1080);
    CHECK(driver.status().pending_backend == 0 && fake.selects == 3 && driver.status().applied.backend == backend_fsr4);
    // A refusal after the announcement is reported once and not retried every frame.
    fake.refuse_fsr4 = true;
    driver.request({true, backend_dlss, 1, 0});
    driver.boundary(a, 1920, 1080);
    TemporalDriver other;
    other.configure(options, nullptr, nullptr);
    CHECK(other.bind_table(table));
    fake.backend = RSF_SR_DLSS;
    other.boundary(a, 1920, 1080);
    other.request({true, backend_fsr4, 1, 0});
    other.boundary(a, 1920, 1080);
    other.boundary(a, 1920, 1080);
    const auto asked = fake.selects;
    other.boundary(a, 1920, 1080);
    other.boundary(a, 1920, 1080);
    CHECK(fake.selects == asked && other.counters().select_failures == 1 && other.status().pending_backend == 0);
    bool said_announce = false, said_took = false, said_refused = false;
    for (const auto& line : lines) {
        said_announce = said_announce || line.find("preparing FSR4. The first use makes the vendor build its pipelines") != std::string::npos;
        said_took = said_took || line.find("FSR4 selected, the boundary took") != std::string::npos;
    }
    CHECK(said_announce && said_took);
    (void)said_refused;
    // Teardown while announced forgets it.
    driver.stop();
    CHECK(!driver.running() && driver.status().pending_backend == 0);
    std::vector<std::string> report_lines;
    other.report([](void* user, const char* line) { static_cast<std::vector<std::string>*>(user)->push_back(line); }, &report_lines);
    CHECK(report_lines.size() == 2 && report_lines[1].find("announced=") != std::string::npos && report_lines[1].find("longest_select_ms=") != std::string::npos);
}

int main(int argc, char** argv) {
    struct { const char* name; void (*run)(); } cases[] = {{"query", query}, {"idle", idle_pipeline}, {"binding", adapter_binding},
        {"lifecycle", driver_lifecycle}, {"start-refused", start_refused_loudly}, {"announce", announce_before_select}};
    int ran = 0;
    for (const auto& item : cases) {
        if (argc > 1 && std::string_view{argv[1]} != item.name) continue;
        item.run();
        ++ran;
    }
    if (!ran) { std::printf("unknown case\n"); return 2; }
    std::printf("%d case(s), %d failure(s)\n", ran, failures);
    return failures ? 1 : 0;
}
