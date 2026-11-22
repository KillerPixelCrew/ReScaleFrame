// SPDX-License-Identifier: MIT
// Win32 DLL linkage and native exception barriers for the Rust adapter.
#include <rescaleframe/game_api.h>
#include <rescaleframe/log.h>
#include <windows.h>
#include <MinHook.h>
#include <d3d11.h>
#include <wrl/client.h>

extern "C" {
bool rsf_wingman_verify_executable() noexcept;
uintptr_t rsf_wingman_image_base() noexcept;
bool rsf_wingman_copy(void*, const void*, size_t) noexcept;
uint64_t rsf_wingman_tick_count() noexcept;
bool rsf_wingman_accepting() noexcept;
}

namespace {
bool create_hook(uintptr_t target, uintptr_t detour, uintptr_t* original) noexcept
{
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return false;
    // The trampoline remains reachable during native unwinding and after disable.
    HMODULE pinned = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(detour), &pinned)) return false;
    void* trampoline = nullptr;
    if (MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(detour), &trampoline) != MH_OK)
        return false;
    *original = reinterpret_cast<uintptr_t>(trampoline);
    return true;
}

void* acquire_texture_guarded(uintptr_t rdg)
{
#if defined(_MSC_VER)
    __try {
#endif
        void* rhi = nullptr;
        uintptr_t table = 0;
        using Native = void* (*)(void*);
        Native native_resource = nullptr;
        // UE4.27 puts GetNativeResource after GetSizeXYZ. This is slot 7.
        if (!rdg || !rsf_wingman_copy(&rhi, reinterpret_cast<void*>(rdg + 0x10), sizeof(rhi)) ||
            !rsf_wingman_copy(&table, rhi, sizeof(table)) || !table ||
            !rsf_wingman_copy(&native_resource, reinterpret_cast<void*>(table + 7 * sizeof(void*)),
                sizeof(native_resource)) || !native_resource) return nullptr;
        auto* native = static_cast<IUnknown*>(native_resource(rhi));
        ID3D11Texture2D* texture = nullptr;
        if (!native || FAILED(native->QueryInterface(__uuidof(ID3D11Texture2D),
                reinterpret_cast<void**>(&texture)))) return nullptr;
        return texture;
#if defined(_MSC_VER)
    } __except (EXCEPTION_EXECUTE_HANDLER) { return nullptr; }
#endif
}

void* acquire_texture(uintptr_t rdg) noexcept try { return acquire_texture_guarded(rdg); }
catch (...) { return nullptr; }

void release_texture(void* texture) noexcept { static_cast<ID3D11Texture2D*>(texture)->Release(); }

void report_execution(const rsf_game_host_services* host, uintptr_t list,
    const rsf_game_render_pass* pass, bool log) noexcept try
{
    using Microsoft::WRL::ComPtr;
    auto* color = static_cast<ID3D11Texture2D*>(pass->color_input);
    ComPtr<ID3D11Device> device;
    color->GetDevice(&device);
    if (!device) return;
    ComPtr<ID3D11DeviceContext> context;
    device->GetImmediateContext(&context);
    if (log) {
        D3D11_TEXTURE2D_DESC desc[4]{};
        color->GetDesc(&desc[0]);
        static_cast<ID3D11Texture2D*>(pass->depth)->GetDesc(&desc[1]);
        static_cast<ID3D11Texture2D*>(pass->motion)->GetDesc(&desc[2]);
        static_cast<ID3D11Texture2D*>(pass->color_output)->GetDesc(&desc[3]);
        rsf_logf(host->log, host->user,
            "PW GPU temporal list=%p history=%p colour=%ux%u/%u depth=%ux%u/%u motion=%ux%u/%u output=%ux%u/%u camera=%u frame=%llu",
            reinterpret_cast<void*>(list), reinterpret_cast<void*>(uintptr_t(pass->history_key)),
            desc[0].Width, desc[0].Height, unsigned(desc[0].Format),
            desc[1].Width, desc[1].Height, unsigned(desc[1].Format),
            desc[2].Width, desc[2].Height, unsigned(desc[2].Format),
            desc[3].Width, desc[3].Height, unsigned(desc[3].Format), pass->camera_valid,
            static_cast<unsigned long long>(pass->native_frame));
    }
    // These callbacks describe the pass after the native dispatch on the same RHI stream.
    if (host->render_pass && rsf_wingman_accepting()) {
        host->render_pass(host->user, context.Get(), pass, 1);
        host->render_pass(host->user, context.Get(), pass, 0);
    }
} catch (...) {}
}

struct WingmanPlatform {
    bool (*verify_executable)() noexcept;
    uintptr_t (*image_base)() noexcept;
    bool (*copy)(void*, const void*, size_t) noexcept;
    uint64_t (*tick_count)() noexcept;
    bool (*create_hook)(uintptr_t, uintptr_t, uintptr_t*) noexcept;
    bool (*enable_hook)(uintptr_t) noexcept;
    bool (*disable_hook)(uintptr_t) noexcept;
    void* (*acquire_texture)(uintptr_t) noexcept;
    void (*release_texture)(void*) noexcept;
    void (*report_execution)(const rsf_game_host_services*, uintptr_t, const rsf_game_render_pass*, bool) noexcept;
    void (*log)(rsf_game_log_fn, void*, const char*) noexcept;
};
static_assert(sizeof(WingmanPlatform) == 11 * sizeof(void*));

constexpr WingmanPlatform platform{
    rsf_wingman_verify_executable, rsf_wingman_image_base, rsf_wingman_copy, rsf_wingman_tick_count,
    create_hook,
    [](uintptr_t target) noexcept { return MH_EnableHook(reinterpret_cast<void*>(target)) == MH_OK; },
    [](uintptr_t target) noexcept {
        const auto result = MH_DisableHook(reinterpret_cast<void*>(target));
        return result == MH_OK || result == MH_ERROR_DISABLED;
    },
    acquire_texture, release_texture, report_execution,
    [](rsf_game_log_fn log, void* user, const char* message) noexcept {
        try { if (log) log(user, message); } catch (...) {}
    },
};

extern "C" rsf_result rsf_wingman_publish_api(
    uint32_t, rsf_game_plugin_api*, const WingmanPlatform*) noexcept;

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t version, rsf_game_plugin_api* api) noexcept
{
    return rsf_wingman_publish_api(version, api, &platform);
}
