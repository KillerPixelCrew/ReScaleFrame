/* SPDX-License-Identifier: MIT */
/* Guarded SDK queries used by the typed Rust DXGI and D3D12 wrappers. */
#include <d3d11.h>
#include <d3d12.h>
#include <dxgi1_6.h>

#include "census_internal.h"

_Static_assert(sizeof(D3D12_RESOURCE_DESC) == 56 && _Alignof(D3D12_RESOURCE_DESC) == 8,
               "D3D12_RESOURCE_DESC layout read by the Rust census");
_Static_assert(sizeof(D3D11_TEXTURE2D_DESC) == 44 && _Alignof(D3D11_TEXTURE2D_DESC) == 4,
               "D3D11_TEXTURE2D_DESC layout read by the Rust census");

int32_t rsf_census_query_interface(void* object, const void* iid, void** result)
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

void rsf_census_release_interface(void* object)
{
    if (!rsf_census_vtable_slot(object, 2)) return;
    IUnknown* unknown = (IUnknown*)object;
    RSF_FOREIGN_TRY {
        unknown->lpVtbl->Release(unknown);
    } RSF_FAULT_CATCH {
    }
}

bool rsf_census_queue_desc(const void* queue, void* description)
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

bool rsf_census_swapchain_desc(const void* chain, void* description)
{
    if (!rsf_census_vtable_slot(chain, 12)) return false;
    IDXGISwapChain* object = (IDXGISwapChain*)chain;
    RSF_FOREIGN_TRY {
        return SUCCEEDED(object->lpVtbl->GetDesc(object, (DXGI_SWAP_CHAIN_DESC*)description));
    } RSF_FAULT_CATCH {
        return false;
    }
}

bool rsf_census_resource12_desc(const void* resource, void* description)
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

bool rsf_census_resource11_dimension(const void* resource, int32_t* dimension)
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

bool rsf_census_resource11_desc(const void* resource, void* description)
{
    ID3D11Texture2D* object = (ID3D11Texture2D*)resource;
    RSF_FOREIGN_TRY {
        object->lpVtbl->GetDesc(object, (D3D11_TEXTURE2D_DESC*)description);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}
