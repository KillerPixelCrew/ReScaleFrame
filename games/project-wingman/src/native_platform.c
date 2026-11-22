/* SPDX-License-Identifier: MIT */
/* Win32 DLL linkage and native exception barriers for the Rust adapter.
   MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the same
   exceptions the former catch-all stopped. The engine and COM reads in acquire_texture still
   contain every fault, as before. */
#include <rescaleframe/game_api.h>
#include <rescaleframe/log.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#define COBJMACROS
#include <windows.h>
#include <initguid.h>
#include <MinHook.h>
#include <d3d11.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL
#define RSF_FOREIGN_EXCEPTION \
    (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH)

bool rsf_wingman_verify_executable(void);
uintptr_t rsf_wingman_image_base(void);
bool rsf_wingman_copy(void*, const void*, size_t);
uint64_t rsf_wingman_tick_count(void);
bool rsf_wingman_accepting(void);

static bool create_hook(uintptr_t target, uintptr_t detour, uintptr_t* original)
{
    const MH_STATUS initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return false;
    /* The trampoline remains reachable during native unwinding and after disable. */
    HMODULE pinned = NULL;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            (LPCWSTR)detour, &pinned)) return false;
    void* trampoline = NULL;
    if (MH_CreateHook((void*)target, (void*)detour, &trampoline) != MH_OK) return false;
    *original = (uintptr_t)trampoline;
    return true;
}

static bool enable_hook(uintptr_t target)
{
    return MH_EnableHook((void*)target) == MH_OK;
}

static bool disable_hook(uintptr_t target)
{
    const MH_STATUS result = MH_DisableHook((void*)target);
    return result == MH_OK || result == MH_ERROR_DISABLED;
}

typedef void* (*native_resource_fn)(void*);

/* The engine object is foreign: a bad read or throw yields no texture. Every fault is contained. */
static void* acquire_texture(uintptr_t rdg)
{
    __try {
        void* rhi = NULL;
        uintptr_t table = 0;
        native_resource_fn native_resource = NULL;
        IUnknown* native;
        ID3D11Texture2D* texture = NULL;
        /* UE4.27 puts GetNativeResource after GetSizeXYZ. This is slot 7. */
        if (!rdg || !rsf_wingman_copy(&rhi, (void*)(rdg + 0x10), sizeof(rhi)) ||
            !rsf_wingman_copy(&table, rhi, sizeof(table)) || !table ||
            !rsf_wingman_copy(&native_resource, (void*)(table + 7 * sizeof(void*)),
                sizeof(native_resource)) || !native_resource) return NULL;
        native = (IUnknown*)native_resource(rhi);
        if (!native || FAILED(IUnknown_QueryInterface(native, &IID_ID3D11Texture2D, (void**)&texture)))
            return NULL;
        return texture;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return NULL;
    }
}

static void release_texture(void* texture)
{
    ID3D11Texture2D_Release((ID3D11Texture2D*)texture);
}

static void report_execution(const rsf_game_host_services* host, uintptr_t list,
    const rsf_game_render_pass* pass, bool log)
{
    ID3D11Device* device = NULL;
    ID3D11DeviceContext* context = NULL;
    __try {
        __try {
            ID3D11Texture2D* color = (ID3D11Texture2D*)pass->color_input;
            ID3D11Texture2D_GetDevice(color, &device);
            if (device) {
                ID3D11Device_GetImmediateContext(device, &context);
                if (log) {
                    D3D11_TEXTURE2D_DESC desc[4] = {{0}};
                    ID3D11Texture2D_GetDesc(color, &desc[0]);
                    ID3D11Texture2D_GetDesc((ID3D11Texture2D*)pass->depth, &desc[1]);
                    ID3D11Texture2D_GetDesc((ID3D11Texture2D*)pass->motion, &desc[2]);
                    ID3D11Texture2D_GetDesc((ID3D11Texture2D*)pass->color_output, &desc[3]);
                    rsf_logf(host->log, host->user,
                        "PW GPU temporal list=%p history=%p colour=%ux%u/%u depth=%ux%u/%u motion=%ux%u/%u output=%ux%u/%u camera=%u frame=%llu",
                        (void*)list, (void*)(uintptr_t)pass->history_key,
                        desc[0].Width, desc[0].Height, (unsigned)desc[0].Format,
                        desc[1].Width, desc[1].Height, (unsigned)desc[1].Format,
                        desc[2].Width, desc[2].Height, (unsigned)desc[2].Format,
                        desc[3].Width, desc[3].Height, (unsigned)desc[3].Format, pass->camera_valid,
                        (unsigned long long)pass->native_frame);
                }
                /* These callbacks describe the pass after the native dispatch on the same RHI stream. */
                if (host->render_pass && rsf_wingman_accepting()) {
                    host->render_pass(host->user, context, pass, 1);
                    host->render_pass(host->user, context, pass, 0);
                }
            }
        } __finally {
            if (context) ID3D11DeviceContext_Release(context);
            if (device) ID3D11Device_Release(device);
        }
    } __except (RSF_FOREIGN_EXCEPTION) {}
}

static void guarded_log(rsf_game_log_fn log, void* user, const char* message)
{
    __try {
        if (log) log(user, message);
    } __except (RSF_FOREIGN_EXCEPTION) {}
}

typedef struct WingmanPlatform {
    bool (*verify_executable)(void);
    uintptr_t (*image_base)(void);
    bool (*copy)(void*, const void*, size_t);
    uint64_t (*tick_count)(void);
    bool (*create_hook)(uintptr_t, uintptr_t, uintptr_t*);
    bool (*enable_hook)(uintptr_t);
    bool (*disable_hook)(uintptr_t);
    void* (*acquire_texture)(uintptr_t);
    void (*release_texture)(void*);
    void (*report_execution)(const rsf_game_host_services*, uintptr_t, const rsf_game_render_pass*, bool);
    void (*log)(rsf_game_log_fn, void*, const char*);
} WingmanPlatform;
_Static_assert(sizeof(WingmanPlatform) == 11 * sizeof(void*), "Wingman platform table layout");

static const WingmanPlatform platform = {
    rsf_wingman_verify_executable, rsf_wingman_image_base, rsf_wingman_copy, rsf_wingman_tick_count,
    create_hook, enable_hook, disable_hook,
    acquire_texture, release_texture, report_execution, guarded_log,
};

rsf_result rsf_wingman_publish_api(uint32_t, rsf_game_plugin_api*, const WingmanPlatform*);

__declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t version, rsf_game_plugin_api* api)
{
    return rsf_wingman_publish_api(version, api, &platform);
}
