// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/plugin_session.h>
#include <windows.h>
#include <mutex>
#include <new>
#include <memory>

// Own the loaded module until plugin producers and queued work have quiesced and stopped.
// readers pins status callbacks against transitions; guard is released before invoking hooks.
struct rsf_plugin_session {
    HMODULE module = nullptr;
    rsf_game_plugin_api api{};
    rsf_game_host_services services{};
    std::mutex guard;
    bool prepared = false, started = false, quiesced = false, transitioning = false;
    uint32_t readers = 0;
    ~rsf_plugin_session() { if (module) FreeLibrary(module); }
};
namespace {
// Refuse ambient DLL lookup: accept drive-rooted and UNC paths only.
bool absolute_path(const wchar_t* path)
{
    if (!path || !path[0]) return false;
    return (path[1] == L':' && (path[2] == L'\\' || path[2] == L'/')) ||
           (path[0] == L'\\' && path[1] == L'\\');
}
// prepare can fail after installing work. Treat unconfirmed retirement as BUSY so the caller
// retains this owner and its callback storage instead of unloading a partially active plugin.
rsf_result retire_failed_prepare(rsf_plugin_session& self) noexcept
{
    try {
        rsf_game_control_args args{sizeof(args), RSF_GAME_ABI_VERSION};
        const auto quiet = self.api.hooks.quiesce(&args);
        self.quiesced = quiet == RSF_OK;
        return self.quiesced ? self.api.hooks.stop(&args) : RSF_ERROR_BUSY;
    } catch (...) { return RSF_ERROR_BUSY; }
}
// Exception-safe reader pin for status; plugin exceptions cannot strand the transition guard.
struct Reader {
    rsf_plugin_session& self;
    explicit Reader(rsf_plugin_session& s) : self(s) {}
    ~Reader() { std::lock_guard<std::mutex> lock(self.guard); --self.readers; }
};
}
extern "C" RSF_RUNTIME_API rsf_result rsf_plugin_session_prepare(const rsf_plugin_session_options* options,
    rsf_plugin_session** out) try
{
    if (!out) return RSF_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    if (!options || options->struct_size < sizeof(*options) || !absolute_path(options->plugin_path) ||
        !options->probe || options->probe->struct_size < sizeof(rsf_game_probe) ||
        options->services.struct_size < sizeof(rsf_game_host_services) || !options->services.session_id)
        return RSF_ERROR_INVALID_ARGUMENT;
    if (options->abi_version != RSF_PLUGIN_SESSION_ABI_VERSION ||
        options->services.abi_version != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    std::unique_ptr<rsf_plugin_session> self(new(std::nothrow) rsf_plugin_session);
    if (!self) return RSF_ERROR_NOT_READY;
    self->services = options->services;
    self->module = LoadLibraryExW(options->plugin_path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!self->module) return RSF_ERROR_NOT_READY;
    auto entry = reinterpret_cast<rsf_get_game_plugin_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(self->module, RSF_GAME_ENTRY_POINT)));
    self->api.struct_size = sizeof(self->api);
    rsf_result result = entry ? entry(RSF_GAME_ABI_VERSION, &self->api) : RSF_ERROR_ABI_MISMATCH;
    if (result == RSF_OK && (self->api.struct_size < sizeof(self->api) || self->api.abi_version != RSF_GAME_ABI_VERSION))
        result = RSF_ERROR_ABI_MISMATCH;
    if (result == RSF_OK && (!self->api.detect || self->api.detect(options->probe) != RSF_GAME_RECOGNIZED))
        result = RSF_ERROR_NATIVE_REFUSED;
    const auto& hooks = self->api.hooks;
    if (result == RSF_OK && (!hooks.prepare || !hooks.start || !hooks.quiesce || !hooks.stop || !hooks.status))
        result = RSF_ERROR_ABI_MISMATCH;
    if (result != RSF_OK) return result;
    rsf_game_prepare_args args{sizeof(args), RSF_GAME_ABI_VERSION, &self->services};
    try { result = hooks.prepare(&args); } catch (...) { result = RSF_ERROR_NOT_READY; }
    if (result != RSF_OK) {
        // A failed prepare may still own hooks or queued commands. Return an inactive owner
        // when it cannot retire, keeping the module and copied host services alive for retry.
        if (retire_failed_prepare(*self) != RSF_OK) {
            *out = self.release(); return RSF_ERROR_BUSY;
        }
        return result;
    }
    self->prepared = true; *out = self.release(); return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API rsf_result rsf_plugin_session_start(rsf_plugin_session* self) try
{
    if (!self) return RSF_ERROR_INVALID_ARGUMENT;
    {
        std::lock_guard<std::mutex> lock(self->guard);
        if (self->transitioning || self->readers) return RSF_ERROR_BUSY;
        if (!self->prepared || self->quiesced) return RSF_ERROR_NOT_READY;
        if (self->started) return RSF_OK;
        self->transitioning = true;
    }
    rsf_game_start_args args{sizeof(args), RSF_GAME_ABI_VERSION};
    rsf_result result = RSF_ERROR_NOT_READY;
    try { result = self->api.hooks.start(&args); } catch (...) {}
    {
        std::lock_guard<std::mutex> lock(self->guard);
        self->transitioning = false; if (result == RSF_OK) self->started = true;
    }
    return result;
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API rsf_result rsf_plugin_session_quiesce(rsf_plugin_session* self) try
{
    if (!self) return RSF_ERROR_INVALID_ARGUMENT;
    {
        std::lock_guard<std::mutex> lock(self->guard);
        if (self->transitioning || self->readers) return RSF_ERROR_BUSY;
        if (self->quiesced) return RSF_OK;
        self->transitioning = true;
    }
    rsf_game_control_args args{sizeof(args), RSF_GAME_ABI_VERSION};
    rsf_result result = RSF_ERROR_NOT_READY;
    try { result = self->api.hooks.quiesce(&args); } catch (...) {}
    {
        std::lock_guard<std::mutex> lock(self->guard); self->transitioning = false;
        if (result == RSF_OK) { self->quiesced = true; self->started = false; }
    }
    return result;
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API rsf_result rsf_plugin_session_status(rsf_plugin_session* self, rsf_game_renderer_status* status) try
{
    if (!self || !status || status->struct_size < sizeof(*status)) return RSF_ERROR_INVALID_ARGUMENT;
    if (status->abi_version != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    {
        std::lock_guard<std::mutex> lock(self->guard);
        if (self->transitioning) return RSF_ERROR_BUSY;
        ++self->readers;
    }
    Reader reader(*self); return self->api.hooks.status(status);
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API rsf_result rsf_plugin_session_stop(rsf_plugin_session* self) try
{
    if (!self) return RSF_OK;
    {
        std::lock_guard<std::mutex> lock(self->guard);
        if (self->transitioning || self->readers || !self->quiesced || self->started) return RSF_ERROR_BUSY;
        self->transitioning = true;
    }
    rsf_game_control_args args{sizeof(args), RSF_GAME_ABI_VERSION};
    rsf_result result = RSF_ERROR_NOT_READY;
    try { result = self->api.hooks.stop(&args); } catch (...) {}
    if (result != RSF_OK) {
        std::lock_guard<std::mutex> lock(self->guard); self->transitioning = false; return result;
    }
    delete self; return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }
