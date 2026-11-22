/* SPDX-License-Identifier: MIT */
/* Guarded SDK queries used by the typed Rust DXGI and D3D12 wrappers. */
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_6.h>

#include "census_internal.h"
#include "d3d_platform.h"

_Static_assert(sizeof(D3D12_RESOURCE_DESC) == 56 && _Alignof(D3D12_RESOURCE_DESC) == 8,
               "D3D12_RESOURCE_DESC layout read by the Rust census");
_Static_assert(sizeof(D3D11_TEXTURE2D_DESC) == 44 && _Alignof(D3D11_TEXTURE2D_DESC) == 4,
               "D3D11_TEXTURE2D_DESC layout read by the Rust census");

static int32_t query_interface(void* object, const void* iid, void** result)
{
    *result = NULL;
    if (!rsf_census_vtable_slot(object, 0)) return E_NOINTERFACE;
    IUnknown* unknown = (IUnknown*)object;
    RSF_FOREIGN_TRY {
        return unknown->lpVtbl->QueryInterface(unknown, (REFIID)iid, result);
    } RSF_FAULT_CATCH {
        return E_NOINTERFACE;
    }
}

static void release_interface(void* object)
{
    if (!rsf_census_vtable_slot(object, 2)) return;
    IUnknown* unknown = (IUnknown*)object;
    RSF_FOREIGN_TRY {
        unknown->lpVtbl->Release(unknown);
    } RSF_FAULT_CATCH {
    }
}

static bool queue_desc(const void* queue, void* description)
{
    if (!rsf_census_vtable_slot(queue, 18)) return false;
    ID3D12CommandQueue* object = (ID3D12CommandQueue*)queue;
    RSF_FOREIGN_TRY {
        object->lpVtbl->GetDesc(object, (D3D12_COMMAND_QUEUE_DESC*)description);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

static bool swapchain_desc(const void* chain, void* description)
{
    if (!rsf_census_vtable_slot(chain, 12)) return false;
    IDXGISwapChain* object = (IDXGISwapChain*)chain;
    RSF_FOREIGN_TRY {
        return SUCCEEDED(object->lpVtbl->GetDesc(object, (DXGI_SWAP_CHAIN_DESC*)description));
    } RSF_FAULT_CATCH {
        return false;
    }
}

static bool resource12_desc(const void* resource, void* description)
{
    if (!rsf_census_vtable_slot(resource, 10)) return false;
    ID3D12Resource* object = (ID3D12Resource*)resource;
    RSF_FOREIGN_TRY {
        object->lpVtbl->GetDesc(object, (D3D12_RESOURCE_DESC*)description);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

static bool resource11_dimension(const void* resource, int32_t* dimension)
{
    if (!rsf_census_vtable_slot(resource, 10)) return false;
    ID3D11Resource* object = (ID3D11Resource*)resource;
    RSF_FOREIGN_TRY {
        D3D11_RESOURCE_DIMENSION value = D3D11_RESOURCE_DIMENSION_UNKNOWN;
        object->lpVtbl->GetType(object, &value);
        *dimension = (int32_t)value;
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

static bool resource11_desc(const void* resource, void* description)
{
    ID3D11Texture2D* object = (ID3D11Texture2D*)resource;
    RSF_FOREIGN_TRY {
        object->lpVtbl->GetDesc(object, (D3D11_TEXTURE2D_DESC*)description);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

void rsf_census_describe_d3d_resource(rsf_census_json* json, const char* key, const void* resource,
                                      bool d3d12)
{
    static const rsf_census_resource_reads reads = {
        resource12_desc,
        resource11_dimension,
        resource11_desc,
    };
    rsf_census_json_resource(json, key, resource, d3d12, &reads);
}

void rsf_census_register_wrappers_d3d(void** table)
{
    static const uint16_t entries[] = {
#define RSF_CENSUS_D3D_ID(name) (uint16_t)RSF_FN_##name,
        RSF_CENSUS_D3D_ENTRIES(RSF_CENSUS_D3D_ID)
#undef RSF_CENSUS_D3D_ID
    };
    _Static_assert(sizeof(entries) / sizeof(entries[0]) == RSF_D3D_COUNT,
                   "one function id per D3D wrapper slot");
    static const rsf_census_d3d_platform platform = {
        rsf_census_safe_read,
        rsf_census_safe_read_cstr,
        rsf_census_vtable_slot,
        query_interface,
        release_interface,
        queue_desc,
        swapchain_desc,
        rsf_census_present_enter,
        rsf_census_present_leave,
    };
    void* wrappers[RSF_D3D_COUNT] = {0};
    rsf_census_d3d_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < RSF_D3D_COUNT; ++i) {
        if (wrappers[i]) table[entries[i]] = wrappers[i];
    }
}
