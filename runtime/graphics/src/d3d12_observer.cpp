// SPDX-License-Identifier: MIT
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <stdint.h>

extern "C" int32_t rsf_observer12_guard_query(void* object, const GUID* iid, void** out) noexcept try
{
    return static_cast<IUnknown*>(object)->QueryInterface(*iid, out);
} catch (...) { return E_FAIL; }

extern "C" int32_t rsf_observer12_guard_debug(const GUID* iid, void** out) noexcept try
{
    return D3D12GetDebugInterface(*iid, out);
} catch (...) { return E_FAIL; }

extern "C" int32_t rsf_observer12_guard_queue_device(void* queue, void** out) noexcept try
{
    return static_cast<ID3D12CommandQueue*>(queue)->GetDevice(__uuidof(ID3D12Device), out);
} catch (...) { return E_FAIL; }

extern "C" int32_t rsf_observer12_guard_removed_reason(void* device) noexcept try
{
    return static_cast<ID3D12Device*>(device)->GetDeviceRemovedReason();
} catch (...) { return S_OK; }

extern "C" const void* rsf_observer12_guard_breadcrumb_head(void* dred) noexcept try
{
    D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT output{};
    const auto result = static_cast<ID3D12DeviceRemovedExtendedData*>(dred)->GetAutoBreadcrumbsOutput(&output);
    return SUCCEEDED(result) ? output.pHeadAutoBreadcrumbNode : nullptr;
} catch (...) { return nullptr; }

extern "C" int32_t rsf_observer12_guard_page_fault(void* dred, uint64_t* address) noexcept try
{
    D3D12_DRED_PAGE_FAULT_OUTPUT output{};
    const auto result = static_cast<ID3D12DeviceRemovedExtendedData*>(dred)->GetPageFaultAllocationOutput(&output);
    if (SUCCEEDED(result)) *address = output.PageFaultVA;
    return result;
} catch (...) { return E_FAIL; }

extern "C" void rsf_observer12_guard_breadcrumbs(void* settings) noexcept try
{
    static_cast<ID3D12DeviceRemovedExtendedDataSettings*>(settings)->SetAutoBreadcrumbsEnablement(
        D3D12_DRED_ENABLEMENT_FORCED_ON);
} catch (...) {}

extern "C" void rsf_observer12_guard_page_faults(void* settings) noexcept try
{
    static_cast<ID3D12DeviceRemovedExtendedDataSettings*>(settings)->SetPageFaultEnablement(
        D3D12_DRED_ENABLEMENT_FORCED_ON);
} catch (...) {}

extern "C" int32_t rsf_observer12_guard_debug_layer(void* debug) noexcept try
{
    static_cast<ID3D12Debug*>(debug)->EnableDebugLayer();
    return 1;
} catch (...) { return 0; }

extern "C" uint32_t rsf_observer12_guard_queue_type(void* queue) noexcept try
{
    return static_cast<uint32_t>(static_cast<ID3D12CommandQueue*>(queue)->GetDesc().Type);
} catch (...) { return UINT32_MAX; }

extern "C" void rsf_observer12_guard_register_debug(void* info_queue, D3D12MessageFunc callback) noexcept try
{
    DWORD cookie{};
    (void)static_cast<ID3D12InfoQueue1*>(info_queue)->RegisterMessageCallback(
        callback, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &cookie);
} catch (...) {}

extern "C" int32_t rsf_observer12_guard_describe_window(void* chain, void** window) noexcept try
{
    DXGI_SWAP_CHAIN_DESC description{};
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&description))) return 0;
    *window = description.OutputWindow;
    return 1;
} catch (...) { return 0; }
