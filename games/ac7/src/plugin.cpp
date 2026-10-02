#include <rescaleframe/game_api.h>
#include <rescaleframe/version.h>
#include <rescaleframe/ac7_native_renderer.h>

#include <cstddef>
#include <mutex>

namespace {
constexpr char known_sha256[] =
    "c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f";
std::mutex lifecycle_guard;
rsf_ac7_native_renderer* renderer = nullptr;
bool transitioning = false, active = false;
const char* reason = "Native renderer has not been prepared.";

template<class T> rsf_result validate(const T* value)
{
    if (!value || value->struct_size < sizeof(T)) return RSF_ERROR_INVALID_ARGUMENT;
    return value->abi_version == RSF_GAME_ABI_VERSION ? RSF_OK : RSF_ERROR_ABI_MISMATCH;
}
rsf_result prepare(const rsf_game_prepare_args* args) noexcept
{
    const auto valid = validate(args); if (valid != RSF_OK) return valid;
    const auto host_valid = validate(args->host); if (host_valid != RSF_OK) return host_valid;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    try {
        { std::lock_guard<std::mutex> lock(lifecycle_guard);
          if (renderer || transitioning) return RSF_ERROR_BUSY;
          transitioning = true; }
        rsf_ac7_native_renderer_options options{};
        options.struct_size = sizeof(options); options.pending_capacity = 256;
        options.session_id = args->host->session_id; options.on_stage = args->host->render_pass;
        options.log = args->host->log; options.user = args->host->user;
        options.render_config = args->host->render_config;
        options.cpu_event = args->host->cpu_event;
        options.on_state = args->host->scope_state;
        rsf_ac7_native_renderer* created = nullptr;
        const bool prepared = rsf_ac7_native_renderer_prepare(&options, &created) != 0;
        { std::lock_guard<std::mutex> lock(lifecycle_guard);
          transitioning = false; renderer = created;
          reason = prepared ? "Native pass controller prepared; graphics activation pending."
                            : "Native renderer refused: researched engine bytes are unavailable or differ."; }
        return prepared ? RSF_OK : RSF_ERROR_NATIVE_REFUSED;
    } catch (...) {
        std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false;
        return RSF_ERROR_NOT_READY;
    }
}
rsf_result start(const rsf_game_start_args* args) noexcept
{
    const auto valid = validate(args); if (valid != RSF_OK) return valid;
    rsf_ac7_native_renderer* current = nullptr;
    try {
        {
            std::lock_guard<std::mutex> lock(lifecycle_guard);
            if (transitioning) return RSF_ERROR_BUSY;
            if (!renderer) return RSF_ERROR_NOT_READY;
            transitioning = true; current = renderer;
        }
        const bool started = rsf_ac7_native_renderer_start(current) != 0;
        {
            std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false;
            active = started;
            reason = started ? "Native view producer and pre-tonemap SR graph active; game validation pending."
                             : "Native renderer activation refused.";
        }
        return started ? RSF_OK : RSF_ERROR_NOT_READY;
    } catch (...) {
        std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false; return RSF_ERROR_NOT_READY;
    }
}
rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    const auto valid = validate(args); if (valid != RSF_OK) return valid;
    rsf_ac7_native_renderer* current = nullptr;
    try {
        {
            std::lock_guard<std::mutex> lock(lifecycle_guard);
            if (transitioning) return RSF_ERROR_BUSY;
            transitioning = true; current = renderer;
        }
        if (current) rsf_ac7_native_renderer_quiesce(current);
        {
            std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false; active = false;
            reason = "Native producers quiesced; queued RHI work may still need to drain.";
        }
        return RSF_OK;
    } catch (...) {
        std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false; return RSF_ERROR_NOT_READY;
    }
}
rsf_result stop(const rsf_game_control_args* args) noexcept
{
    const auto valid = validate(args); if (valid != RSF_OK) return valid;
    rsf_ac7_native_renderer* current = nullptr;
    try {
        {
            std::lock_guard<std::mutex> lock(lifecycle_guard);
            if (transitioning || active) return RSF_ERROR_BUSY;
            transitioning = true; current = renderer;
        }
        const bool stopped = !current || rsf_ac7_native_renderer_stop(current) != 0;
        {
            std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false;
            if (stopped) { renderer = nullptr; reason = "Native renderer stopped."; }
            else reason = "Native renderer draining; module and host services remain owned.";
        }
        return stopped ? RSF_OK : RSF_ERROR_BUSY;
    } catch (...) {
        std::lock_guard<std::mutex> lock(lifecycle_guard); transitioning = false; return RSF_ERROR_NOT_READY;
    }
}
rsf_result status(rsf_game_renderer_status* output) noexcept
{
    const auto valid = validate(output); if (valid != RSF_OK) return valid;
    try {
        std::lock_guard<std::mutex> lock(lifecycle_guard);
        if (transitioning) return RSF_ERROR_BUSY;
        output->prepared = renderer ? 1u : 0u;
        output->active = active && rsf_ac7_native_renderer_is_active(renderer) ? 1u : 0u;
        output->rendering_ready = 0;
        output->reason = active && !output->active ? "Native renderer deactivated after command ownership refusal." : reason;
        return RSF_OK;
    } catch (...) { return RSF_ERROR_NOT_READY; }
}

char ascii_lower(char value) noexcept
{
    return value >= 'A' && value <= 'Z' ? static_cast<char>(value + ('a' - 'A')) : value;
}

bool equal_ascii(const char* value, const char* expected) noexcept
{
    if (!value || !expected) {
        return false;
    }
    for (std::size_t i = 0;; ++i) {
        if (ascii_lower(value[i]) != ascii_lower(expected[i])) {
            return false;
        }
        if (expected[i] == '\0') {
            return true;
        }
    }
}

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    if (!probe || probe->struct_size < sizeof(rsf_game_probe) || probe->pe_machine != 0x8664) {
        return RSF_GAME_UNKNOWN;
    }
    return equal_ascii(probe->executable_name_utf8, "Ace7Game.exe") &&
                   equal_ascii(probe->sha256_hex, known_sha256)
               ? RSF_GAME_RECOGNIZED
               : RSF_GAME_UNKNOWN;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    if (!api || api->struct_size < sizeof(rsf_game_plugin_api)) {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if (requested_abi != RSF_GAME_ABI_VERSION) {
        return RSF_ERROR_ABI_MISMATCH;
    }
    *api = {
        sizeof(rsf_game_plugin_api), RSF_GAME_ABI_VERSION,
        {"ac7", "Ace Combat 7: Skies Unknown", RSF_VERSION_STRING, 0,
         "Native SR graph and view producer built and synthetic-tested. Game validation is pending."},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
