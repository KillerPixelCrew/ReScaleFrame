// SPDX-License-Identifier: GPL-3.0-only
#include "mono_runtime.h"
#include <windows.h>
#include <string>
#include <thread>

// Resolve embedding functions from the player's loaded Mono runtime. No second runtime/domain is
// created, and no Unity object is touched on these workers. Bootstrap defers adapter work to its loop.
namespace {
struct Domain;
struct Assembly;
struct Image;
struct Class;
struct Method;
struct Object;
struct Thread;
// Process-local embedding table and borrowed domain/method identities. Lifecycle calls serialize
// start/stop; Unity owns the runtime, scripting domain and loaded managed assemblies.
struct Mono {
    void (*domains)(void (*)(Domain*, void*), void*) = nullptr;
    const char* (*domain_name)(Domain*) = nullptr;
    Thread* (*attach)(Domain*) = nullptr;
    void (*detach)(Thread*) = nullptr;
    int (*set_domain)(Domain*, int) = nullptr;
    Assembly* (*open)(Domain*, const char*) = nullptr;
    Image* (*image)(Assembly*) = nullptr;
    Class* (*find_class)(Image*, const char*, const char*) = nullptr;
    Method* (*method)(Class*, const char*, int) = nullptr;
    Object* (*invoke)(Method*, void*, void**, Object**) = nullptr;
    void* (*unbox)(Object*) = nullptr;
    Domain* domain = nullptr;
    Domain* (*root)() = nullptr;
    Method* stop = nullptr;
    void (*log)(const char*) = nullptr;
} mono;

template<typename T> bool resolve(HMODULE module, const char* name, T& target)
{
    target = reinterpret_cast<T>(reinterpret_cast<void*>(GetProcAddress(module, name)));
    return target != nullptr;
}
// Mono's assembly loader expects UTF-8; include the terminator for the borrowed c_str call.
bool utf8(const wchar_t* text, std::string& output)
{
    const int size = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text, -1, nullptr, 0, nullptr, nullptr);
    if (!size) return false;
    output.resize(static_cast<size_t>(size));
    return WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text, -1, output.data(), size, nullptr, nullptr) != 0;
}
void domain_candidate(Domain* domain, void*)
{
    const char* name = mono.domain_name(domain);
    if (name && mono.log) mono.log(name);
    // This Unity 6000.3 player runs its scripts in the named Unity Root Domain. Older
    // players may use a child domain. Select only domains explicitly identified by Unity.
    if (name && (std::string(name) == "Unity Child Domain" ||
        (!mono.domain && std::string(name) == "Unity Root Domain"))) mono.domain = domain;
}
}

// Attach to root for discovery, then select the explicit Unity domain before managed invocation.
// Detach on every exit; managed Start copies api and returns zero only after bootstrap patching.
static bool start_attached(const wchar_t* helper, rsf_unity_native_api* api, const char** reason) noexcept try
{
    if (!helper || !api || !reason || mono.stop) return false;
    *reason = "Unity Mono scripting domain is not available.";
    mono.log = api->log;
    HMODULE module = GetModuleHandleW(L"mono-2.0-bdwgc.dll");
    if (!module || !GetModuleHandleW(L"UnityPlayer.dll")) return false;
    if (!resolve(module, "mono_get_root_domain", mono.root) || !resolve(module, "mono_domain_foreach", mono.domains) ||
        !resolve(module, "mono_domain_get_friendly_name", mono.domain_name) ||
        !resolve(module, "mono_thread_attach", mono.attach) || !resolve(module, "mono_thread_detach", mono.detach) ||
        !resolve(module, "mono_domain_set", mono.set_domain) || !resolve(module, "mono_domain_assembly_open", mono.open) ||
        !resolve(module, "mono_assembly_get_image", mono.image) || !resolve(module, "mono_class_from_name", mono.find_class) ||
        !resolve(module, "mono_class_get_method_from_name", mono.method) || !resolve(module, "mono_runtime_invoke", mono.invoke) ||
        !resolve(module, "mono_object_unbox", mono.unbox)) {
        *reason = "Required Mono embedding exports are missing."; return false;
    }
    Domain* root = mono.root();
    if (!root) return false;
    Thread* thread = mono.attach(root);
    if (!thread) return false;
    struct Detach { Thread* value; ~Detach() { mono.detach(value); } } detach{thread};
    mono.domain = nullptr; mono.domains(domain_candidate, nullptr);
    if (!mono.domain) return false;
    if (!mono.set_domain(mono.domain, 0)) return false;
    std::string path;
    if (!utf8(helper, path)) return false;
    const auto separator = path.find_last_of("/\\");
    const std::string harmony = path.substr(0, separator + 1) + "0Harmony.dll";
    if (!mono.open(mono.domain, harmony.c_str())) { *reason = "Harmony runtime could not load into Unity's script domain."; return false; }
    Assembly* assembly = mono.open(mono.domain, path.c_str());
    Class* klass = assembly ? mono.find_class(mono.image(assembly), "ReScaleFrame.Unity", "Bootstrap") : nullptr;
    Method* start = klass ? mono.method(klass, "Start", 1) : nullptr;
    Method* stop = klass ? mono.method(klass, "Stop", 0) : nullptr;
    if (!start || !stop) { *reason = "Managed Unity bootstrap entry points are missing."; return false; }
    void* address = api;
    void* arguments[] = {&address};
    Object* error = nullptr;
    Object* result = mono.invoke(start, nullptr, arguments, &error);
    if (error || !result || *static_cast<int32_t*>(mono.unbox(result)) != 0) {
        *reason = "Managed bootstrap refused; see the managed diagnostics."; return false;
    }
    mono.stop = stop;
    *reason = "Mono helper loaded; main-thread adapter activation and GPU validation pending.";
    return true;
}
catch (...) { if (reason) *reason = "Mono bootstrap allocation or conversion failed."; return false; }

// Retain the cached Stop method on attachment/invocation/refusal failure so cleanup can be retried.
static bool stop_attached() noexcept
{
    if (!mono.stop) return true;
    Thread* thread = mono.attach(mono.domain);
    if (!thread) return false;
    Object* error = nullptr;
    Object* result = mono.invoke(mono.stop, nullptr, nullptr, &error);
    const bool ok = !error && result && *static_cast<int32_t*>(mono.unbox(result)) == 0;
    mono.detach(thread);
    if (ok) mono.stop = nullptr;
    return ok;
}
bool rsf_unity_mono_start(const wchar_t* helper, rsf_unity_native_api* api, const char** reason) noexcept try
{
    bool result = false;
    // This is a fresh native thread, so its Mono attachment is ours to detach. Never detach a
    // caller's existing Unity/Mono thread or overwrite its current scripting domain.
    std::thread worker([&] { result = start_attached(helper, api, reason); });
    worker.join(); return result;
}
catch (...) { if (reason) *reason = "Mono bootstrap worker could not start."; return false; }
bool rsf_unity_mono_stop() noexcept try
{
    if (!mono.stop) return true;
    bool result = false;
    std::thread worker([&] { result = stop_attached(); });
    worker.join(); return result;
}
catch (...) { return false; }
