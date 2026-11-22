// SPDX-License-Identifier: MIT
#include "sr_temporal.h"
#include <d3d11.h>

namespace rsf::anomaly::sr {
static_assert(backend_dlss == RSF_SR_DLSS && backend_fsr3 == RSF_SR_FSR3 &&
    backend_fsr4 == RSF_SR_FSR4 && backend_xess == RSF_SR_XESS);
static_assert(motion_validity_channel == 2, "the core motion target stores validity in Z");
static_assert(RSF_DLSS_ENGINE_CUSTOM == 0);
namespace {
static_assert(sizeof(rsf_d3d11_sr_api) == 112 && offsetof(rsf_d3d11_sr_api, start) == 16 &&
    offsetof(rsf_d3d11_sr_api, set_sharpening) == 104);
static_assert(sizeof(SrSettings) == 16 && offsetof(SrSettings, backend) == 4);
static_assert(sizeof(TemporalStatusData) == 288 &&
    offsetof(TemporalStatusData, reason_utf8) == 188);
static_assert(sizeof(TemporalCounters) == 432);

}

extern "C" {
TemporalState* rsf_anomaly_sr_create() noexcept;
void rsf_anomaly_sr_destroy(TemporalState*) noexcept;
ApiStatus rsf_anomaly_sr_query(rsf_get_d3d11_sr_api_fn, rsf_d3d11_sr_api*, const char**) noexcept;
void rsf_anomaly_sr_configure(TemporalState*, const rsf_anomaly_sr_driver_config*, rsf_game_log_fn, void*) noexcept;
bool rsf_anomaly_sr_bind_runtime(TemporalState*, HMODULE) noexcept;
uint64_t rsf_anomaly_sr_session_id() noexcept;
bool rsf_anomaly_sr_bind_table(TemporalState*, const rsf_d3d11_sr_api*, uint64_t) noexcept;
void rsf_anomaly_sr_request(TemporalState*, const SrSettings*) noexcept;
bool rsf_anomaly_sr_enabled(const TemporalState*) noexcept;
bool rsf_anomaly_sr_running(const TemporalState*) noexcept;
void* rsf_anomaly_sr_output_texture(const TemporalState*) noexcept;
bool rsf_anomaly_sr_plan(const TemporalState*, uint32_t, uint32_t, uint32_t*, uint32_t*) noexcept;
void rsf_anomaly_sr_boundary(TemporalState*, void*, uint32_t, uint32_t) noexcept;
bool rsf_anomaly_sr_evaluate(TemporalState*, void*, rsf_anomaly_sr_pass_inputs*, const rsf_anomaly_sr_reset_facts*,
    rsf_camera_frame*, uint32_t*) noexcept;
bool rsf_anomaly_sr_request_dump(const TemporalState*, const char*) noexcept;
void rsf_anomaly_sr_fallback(TemporalState*, uint8_t, bool) noexcept;
void rsf_anomaly_sr_count_prepared(TemporalState*, bool) noexcept;
void rsf_anomaly_sr_stop(TemporalState*) noexcept;
void rsf_anomaly_sr_status(const TemporalState*, TemporalStatusData*) noexcept;
void rsf_anomaly_sr_counters(const TemporalState*, TemporalCounters*) noexcept;
void rsf_anomaly_sr_overlay_report(const TemporalState*, rsf_overlay_sr_report*) noexcept;
void rsf_anomaly_sr_report(TemporalState*, bool, bool, rsf_game_log_fn, void*) noexcept;
void rsf_anomaly_sr_start_refusal(int32_t, char*, std::size_t) noexcept;
}

TemporalDriver::TemporalDriver() noexcept : state_(rsf_anomaly_sr_create()) {}
TemporalDriver::~TemporalDriver() { rsf_anomaly_sr_destroy(state_); }

void TemporalDriver::LogSink::call(void* user, const char* message) noexcept {
    const auto& sink = *static_cast<const LogSink*>(user);
    if (sink.emit) try { sink.emit(sink.user, message); } catch (...) {}
}

void TemporalDriver::configure(const TemporalOptions& options, rsf_game_log_fn log, void* user) noexcept {
    const rsf_anomaly_sr_driver_config config{options.stage, options.exposure_auto, options.colour_transport,
        options.first_person_zero, options.require_signature, options.initial,
        options.streamline.c_str(), options.fidelityfx.c_str(), options.xess.c_str()};
    configure(config, log, user);
}

void TemporalDriver::configure(const rsf_anomaly_sr_driver_config& config, rsf_game_log_fn log, void* user) noexcept {
    log_ = {log, user};
    rsf_anomaly_sr_configure(state_, &config, log ? &LogSink::call : nullptr, log ? &log_ : nullptr);
}

ApiStatus bind_sr_api(rsf_get_d3d11_sr_api_fn query, rsf_d3d11_sr_api& out, const char** reason) noexcept {
    return rsf_anomaly_sr_query(query, &out, reason);
}

bool TemporalDriver::bind(HMODULE runtime) noexcept {
    return rsf_anomaly_sr_bind_runtime(state_, runtime);
}
bool TemporalDriver::bind_table(const rsf_d3d11_sr_api& table) noexcept {
    return rsf_anomaly_sr_bind_table(state_, &table, rsf_anomaly_sr_session_id());
}
void TemporalDriver::request(const SrSettings& settings) noexcept { rsf_anomaly_sr_request(state_, &settings); }
bool TemporalDriver::enabled() const noexcept { return rsf_anomaly_sr_enabled(state_); }
bool TemporalDriver::running() const noexcept { return rsf_anomaly_sr_running(state_); }
uint32_t TemporalDriver::output_format() const noexcept {
    auto* texture = static_cast<ID3D11Texture2D*>(rsf_anomaly_sr_output_texture(state_));
    if (!texture) return 0;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    return desc.Format;
}
bool TemporalDriver::planned_render(uint32_t ow, uint32_t oh, uint32_t& rw, uint32_t& rh) const noexcept {
    return rsf_anomaly_sr_plan(state_, ow, oh, &rw, &rh);
}
void TemporalDriver::boundary(ID3D11Device* device, uint32_t ow, uint32_t oh) noexcept {
    rsf_anomaly_sr_boundary(state_, device, ow, oh);
}
bool TemporalDriver::evaluate(ID3D11DeviceContext* context, PassInputs& inputs, const ResetFacts& facts) noexcept {
    auto args = pass_inputs(inputs);
    const bool evaluated = evaluate(context, args, facts);
    inputs.session = args.session;
    inputs.reset = args.reset;
    return evaluated;
}
bool TemporalDriver::evaluate(ID3D11DeviceContext* context, rsf_anomaly_sr_pass_inputs& inputs, const ResetFacts& facts,
    rsf_camera_frame* captured_camera, uint32_t* captured_backend) noexcept {
    const auto reset = reset_facts(facts);
    return rsf_anomaly_sr_evaluate(state_, context, &inputs, &reset, captured_camera, captured_backend);
}
bool TemporalDriver::request_dump(const std::string& prefix) noexcept {
    return rsf_anomaly_sr_request_dump(state_, prefix.c_str());
}
void TemporalDriver::fallback(uint8_t reason, bool drawn) noexcept { rsf_anomaly_sr_fallback(state_, reason, drawn); }
void TemporalDriver::count_prepared(bool missing) noexcept { rsf_anomaly_sr_count_prepared(state_, missing); }
void TemporalDriver::stop() noexcept { rsf_anomaly_sr_stop(state_); }
TemporalStatus TemporalDriver::status() const noexcept {
    TemporalStatus status{};
    rsf_anomaly_sr_status(state_, &status);
    status.reason = status.reason_utf8;
    return status;
}
TemporalCounters TemporalDriver::counters() const noexcept {
    TemporalCounters counters{};
    rsf_anomaly_sr_counters(state_, &counters);
    return counters;
}
bool TemporalDriver::overlay_report(rsf_overlay_sr_report& report) const noexcept {
    rsf_anomaly_sr_overlay_report(state_, &report);
    return true;
}
void TemporalDriver::report(bool exposure_auto, bool first_person_zero, rsf_game_log_fn emit, void* user) noexcept {
    if (!emit) return;
    LogSink sink{emit, user};
    rsf_anomaly_sr_report(state_, exposure_auto, first_person_zero, &LogSink::call, &sink);
}

std::string start_refusal_reason(int32_t result) {
    char reason[96]{};
    rsf_anomaly_sr_start_refusal(result, reason, sizeof(reason));
    return reason;
}
}
