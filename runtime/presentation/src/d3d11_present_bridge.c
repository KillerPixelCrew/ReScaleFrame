/* SPDX-License-Identifier: MIT */
/* The native calls the Rust presentation owner cannot make itself: foreign C++ exceptions from
   DXGI and the Streamline proxies are contained with a structured-exception filter on the MSVC
   C++ exception code 0xE06D7363; other faults propagate. The two factory detours stay here so no foreign unwind crosses a Rust frame on the
   way back to the application. Everything else is d3d11_present.rs. */
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <dxgi1_6.h>
#include <stdbool.h>
#include <rescaleframe/foreign_guard.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

/* Rust factory owner (d3d11_present_factory.rs). */
uint32_t rsf_d3d11p_factory_hooks_ready(void);
void* rsf_d3d11p_factory_legacy_original(void);
void* rsf_d3d11p_factory_hwnd_original(void);
bool rsf_d3d11p_factory_intercept(void* factory, void* device, const DXGI_SWAP_CHAIN_DESC* desc, void** out);
bool rsf_d3d11p_factory_intercept_hwnd(void* factory, void* device, HWND hwnd, const DXGI_SWAP_CHAIN_DESC1* desc,
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen, void* output, void** out, int32_t* result);
int32_t rsf_d3d11p_factory_create_hwnd(void* factory, void* device, void* hwnd, const DXGI_SWAP_CHAIN_DESC1* desc,
    void** out);
int32_t rsf_d3d11p_factory_wrap_native(void* device, void* native, void** out);

typedef HRESULT(STDMETHODCALLTYPE* rsf_d3d11p_create_fn)(IDXGIFactory*, IUnknown*, DXGI_SWAP_CHAIN_DESC*, IDXGISwapChain**);
typedef HRESULT(STDMETHODCALLTYPE* rsf_d3d11p_create_hwnd_fn)(IDXGIFactory2*, IUnknown*, HWND,
    const DXGI_SWAP_CHAIN_DESC1*, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
typedef int32_t (*rsf_d3d11p_create_original_fn)(void*, void*, DXGI_SWAP_CHAIN_DESC*, void**);

static int32_t leaf_create_original(void* factory, void* queue, DXGI_SWAP_CHAIN_DESC* desc, void** out)
{
    RSF_FOREIGN_TRY {
        const rsf_d3d11p_create_fn original = (rsf_d3d11p_create_fn)rsf_d3d11p_factory_legacy_original();
        return original ? original((IDXGIFactory*)factory, (IUnknown*)queue, desc, (IDXGISwapChain**)out) : E_FAIL;
    } RSF_FOREIGN_CATCH { return E_FAIL; }
}

HRESULT STDMETHODCALLTYPE rsf_d3d11p_create_hook(IDXGIFactory* factory, IUnknown* device, DXGI_SWAP_CHAIN_DESC* desc,
    IDXGISwapChain** out)
{
    if (rsf_d3d11p_factory_hooks_ready() && rsf_d3d11p_factory_intercept(factory, device, desc, (void**)out)) return S_OK;
    const rsf_d3d11p_create_fn original = (rsf_d3d11p_create_fn)rsf_d3d11p_factory_legacy_original();
    return original ? original(factory, device, desc, out) : E_FAIL;
}

HRESULT STDMETHODCALLTYPE rsf_d3d11p_hwnd_hook(IDXGIFactory2* factory, IUnknown* device, HWND hwnd,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen, IDXGIOutput* output,
    IDXGISwapChain1** out)
{
    int32_t result = E_FAIL;
    if (rsf_d3d11p_factory_hooks_ready() &&
        rsf_d3d11p_factory_intercept_hwnd(factory, device, hwnd, desc, fullscreen, output, (void**)out, &result))
        return result;
    const rsf_d3d11p_create_hwnd_fn original = (rsf_d3d11p_create_hwnd_fn)rsf_d3d11p_factory_hwnd_original();
    return original ? original(factory, device, hwnd, desc, fullscreen, output, out) : E_FAIL;
}

/* IID_IDXGIFactory2, spelled here so the C leaf needs no dxguid import. */
static const GUID factory2_iid = {0x50c83a1c, 0xe072, 0x4c48, {0x87, 0xb0, 0x36, 0x30, 0xfa, 0x36, 0xa6, 0xd0}};

/* Transfers a retained Factory2 reference, or null. */
void* rsf_d3d11p_bootstrap_factory(void)
{
    RSF_FOREIGN_TRY {
        IDXGIFactory2* factory = NULL;
        if (FAILED(CreateDXGIFactory1(&factory2_iid, (void**)&factory))) return NULL;
        return factory;
    } RSF_FOREIGN_CATCH { return NULL; }
}

rsf_d3d11p_create_original_fn rsf_d3d11p_original_create(void)
{
    return rsf_d3d11p_factory_legacy_original() ? leaf_create_original : NULL;
}

void* rsf_d3d11p_unwrap_guarded(void* host, void* proxy)
{
    RSF_FOREIGN_TRY {
        return rsf_streamline_host_native((rsf_streamline_host*)host, proxy);
    } RSF_FOREIGN_CATCH { return NULL; }
}

int32_t rsf_d3d11_present_create_for_hwnd(void* factory, void* queue, void* hwnd,
    const void* description, void** out)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11p_factory_create_hwnd(factory, queue, hwnd, (const DXGI_SWAP_CHAIN_DESC1*)description, out);
    } RSF_FOREIGN_CATCH { if (out) *out = NULL; return E_FAIL; }
}

int32_t rsf_d3d11_present_wrap_native_chain(void* raw_device, void* raw_chain, void** out)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11p_factory_wrap_native(raw_device, raw_chain, out);
    } RSF_FOREIGN_CATCH { return E_FAIL; }
}
