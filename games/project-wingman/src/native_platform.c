/* SPDX-License-Identifier: MIT */
/* Win32 DLL export and native exception barriers for the Rust adapter (rescaleframe/foreign_guard.h).
   Calls into the host contain foreign C++ exceptions only; the engine and COM reads in
   acquire_texture contain every fault, as before. */
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
#include <d3d11.h>

#include <rescaleframe/foreign_guard.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

bool rsf_wingman_accepting(void);
bool rsf_wingman_copy(void*, const void*, size_t);

typedef void* (*native_resource_fn)(void*);

/* The engine object is foreign: a bad read or throw yields no texture. Every fault is contained. */
void* rsf_wingman_acquire_texture(uintptr_t rdg)
{
    RSF_FOREIGN_TRY {
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
    } RSF_FAULT_CATCH {
        return NULL;
    }
}

void rsf_wingman_release_texture(void* texture)
{
    ID3D11Texture2D_Release((ID3D11Texture2D*)texture);
}

void rsf_wingman_report_execution(const rsf_game_host_services* host, uintptr_t list,
    const rsf_game_render_pass* pass, bool log)
{
    ID3D11Device* device = NULL;
    ID3D11DeviceContext* context = NULL;
    RSF_FOREIGN_TRY {
        RSF_FOREIGN_TRY {
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
        } RSF_FOREIGN_FINALLY {
            if (context) ID3D11DeviceContext_Release(context);
            if (device) ID3D11Device_Release(device);
        }
    } RSF_FOREIGN_CATCH {}
}

rsf_result rsf_wingman_publish_api(uint32_t, rsf_game_plugin_api*);

__declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t version, rsf_game_plugin_api* api)
{
    return rsf_wingman_publish_api(version, api);
}
