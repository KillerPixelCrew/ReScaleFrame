// SPDX-License-Identifier: MIT
// Explicit device regression, never registered in CTest. Scratch SDK root only.
// Hidden-window callbacks are not proof of physical scanout or gameplay quality.
// Exit 77 means prerequisites/hidden presentation prevented a generation receipt;
// exit 1 means a contract failure; exit 0 requires observed FSR generated callbacks.
#include <rescaleframe/d3d11_host.h>
#include "d3d11_host_device_fixture.h"
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_present_api.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/overlay_api.h>
#include <d3d11.h>
#include <d3d11sdklayers.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <dxgidebug.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <cstdio>
#include <filesystem>
#include <vector>
#include <cstring>

using Microsoft::WRL::ComPtr;
namespace {
// A forwarding outer wrapper with native canonical QI and real pre/post Present
// work. The engine retains this pointer; no test rewrites it to the inner owner.
struct OuterChain final : IDXGISwapChain {
    ComPtr<IDXGISwapChain> inner;
    ULONG refs = 1;
    uint32_t pre = 0, post = 0, dispatched = 0;
    void* host_user;
    OuterChain(IDXGISwapChain* value,void* user) : inner(value),host_user(user) {}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid,void** out) override { return inner->QueryInterface(iid,out); }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override { const auto remaining = --refs; if (!remaining) delete this; return remaining; }
    HRESULT STDMETHODCALLTYPE SetPrivateData(REFGUID key,UINT size,const void* data) override { return inner->SetPrivateData(key,size,data); }
    HRESULT STDMETHODCALLTYPE SetPrivateDataInterface(REFGUID key,const IUnknown* value) override { return inner->SetPrivateDataInterface(key,value); }
    HRESULT STDMETHODCALLTYPE GetPrivateData(REFGUID key,UINT* size,void* data) override { return inner->GetPrivateData(key,size,data); }
    HRESULT STDMETHODCALLTYPE GetParent(REFIID iid,void** out) override { return inner->GetParent(iid,out); }
    HRESULT STDMETHODCALLTYPE GetDevice(REFIID iid,void** out) override { return inner->GetDevice(iid,out); }
    HRESULT STDMETHODCALLTYPE Present(UINT sync,UINT flags) override {
        ++pre;
        // Model a continuation reached after external pre-work, without reentering
        // the facade's virtual slot. Never fall through after a service error.
        rsf_d3d11_present_dispatch request{sizeof(request),RSF_D3D11_HOST_ABI_VERSION,inner.Get(),sync,flags,0,E_FAIL};
        const auto status = rsf_d3d11_host_dispatch_present(host_user,&request);
        const HRESULT result = status != RSF_OK ? E_FAIL : request.handled ? request.native_result : inner->Present(sync,flags);
        if (status == RSF_OK && request.handled) ++dispatched;
        ++post; return result;
    }
    HRESULT STDMETHODCALLTYPE GetBuffer(UINT index,REFIID iid,void** out) override { return inner->GetBuffer(index,iid,out); }
    HRESULT STDMETHODCALLTYPE SetFullscreenState(BOOL enabled,IDXGIOutput* output) override { return inner->SetFullscreenState(enabled,output); }
    HRESULT STDMETHODCALLTYPE GetFullscreenState(BOOL* enabled,IDXGIOutput** output) override { return inner->GetFullscreenState(enabled,output); }
    HRESULT STDMETHODCALLTYPE GetDesc(DXGI_SWAP_CHAIN_DESC* out) override { return inner->GetDesc(out); }
    HRESULT STDMETHODCALLTYPE ResizeBuffers(UINT n,UINT w,UINT h,DXGI_FORMAT format,UINT flags) override { return inner->ResizeBuffers(n,w,h,format,flags); }
    HRESULT STDMETHODCALLTYPE ResizeTarget(const DXGI_MODE_DESC* desc) override { return inner->ResizeTarget(desc); }
    HRESULT STDMETHODCALLTYPE GetContainingOutput(IDXGIOutput** out) override { return inner->GetContainingOutput(out); }
    HRESULT STDMETHODCALLTYPE GetFrameStatistics(DXGI_FRAME_STATISTICS* out) override { return inner->GetFrameStatistics(out); }
    HRESULT STDMETHODCALLTYPE GetLastPresentCount(UINT* out) override { return inner->GetLastPresentCount(out); }
};
HWND window{};
struct TemporaryIni { std::wstring path; ~TemporaryIni() { DeleteFileW(path.c_str()); } };
constexpr UINT width = 960, height = 540;
constexpr GUID diagnostic_all{0xe48ae283,0xda80,0x490b,{0x87,0xe6,0x43,0xe9,0xa9,0xcf,0xda,0x08}};
void log(void*, const char* message) { std::printf("%s\n", message); }
int accept(void*, void* candidate) { return candidate == window; }
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, DXGI_FORMAT format, UINT bindings) {
    ComPtr<ID3D11Texture2D> result;
    result.Attach(static_cast<ID3D11Texture2D*>(
        rsf_d3d11_host_fixture_texture(device, width, height, uint32_t(format), bindings)));
    return result;
}
int fail(const char* label) { std::printf("FAIL: %s\n", label); return 1; }
void dxgi_errors(IDXGIInfoQueue* queue) {
    if (!queue) return;
    const auto count=queue->GetNumStoredMessagesAllowedByRetrievalFilters(diagnostic_all);
    std::printf("DXGI stored messages=%llu\n",static_cast<unsigned long long>(count));
    unsigned printed=0;
    for (UINT64 i=0;i<count && printed<16;++i) {
        SIZE_T size{};
        if (FAILED(queue->GetMessage(diagnostic_all,i,nullptr,&size)) || !size) continue;
        std::vector<unsigned char> storage(size);
        auto* message=reinterpret_cast<DXGI_INFO_QUEUE_MESSAGE*>(storage.data());
        if (SUCCEEDED(queue->GetMessage(diagnostic_all,i,message,&size)) &&
            message->Severity<=DXGI_INFO_QUEUE_MESSAGE_SEVERITY_WARNING) {
            std::printf("DXGI message severity=%u id=%d: %s\n",unsigned(message->Severity),message->ID,message->pDescription);
            ++printed;
        }
    }
}

}
int main(int argc, char** argv) {
    const bool scoped = argc == 3 && std::strcmp(argv[2],"--scoped") == 0;
    if (argc != 2 && !scoped) { std::puts("Usage: rsf_d3d11_host_rendered_fg_device <scratch runtime root> [--scoped]"); return 77; }
    const std::filesystem::path root(argv[1]);
    if (!std::filesystem::exists(root / "streamline/sl.interposer.dll") ||
        !std::filesystem::is_directory(root / "fidelityfx")) return 77;
    // Optional system diagnostic queue, before the exact production creation path.
    // Do not change factory creation flags or substitute a different API for it.
    ComPtr<IDXGIInfoQueue> dxgi_diagnostics;
    const auto debug_module=LoadLibraryExW(L"dxgidebug.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (debug_module) {
        using Query=HRESULT(WINAPI*)(UINT,REFIID,void**);
        const auto query=reinterpret_cast<Query>(reinterpret_cast<void*>(GetProcAddress(debug_module,"DXGIGetDebugInterface1")));
        if (query) (void)query(0,IID_PPV_ARGS(&dxgi_diagnostics));
    }
    std::printf("DXGI info queue available=%u\n",dxgi_diagnostics ? 1u : 0u);
    if (dxgi_diagnostics) dxgi_diagnostics->ClearStoredMessages(diagnostic_all);
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return 77;
    ComPtr<IDXGIAdapter1> adapter;
    for (UINT i = 0; ; ++i) {
        if (FAILED(factory->EnumAdapters1(i, &adapter))) return 77;
        DXGI_ADAPTER_DESC1 desc{}; adapter->GetDesc1(&desc);
        if (desc.VendorId == 0x10de && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) {
            std::printf("Adapter: %ls\n", desc.Description); break;
        }
        adapter.Reset();
    }
    const auto module = GetModuleHandleW(nullptr);
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = module; wc.lpszClassName = L"RSFRenderedFGDevice";
    if (!RegisterClassW(&wc)) return fail("register hidden fixture window class");
    window = CreateWindowW(wc.lpszClassName, L"RSF rendered FG device check", WS_OVERLAPPED,
        0, 0, int(width), int(height), nullptr, nullptr, module, nullptr);
    if (!window) return fail("create hidden fixture window"); // Deliberately never shown or made foreground.
    if (scoped) {
        // Before installing RSF interception, isolate the usage variable on the
        // same native adapter/queue. Separate HWNDs avoid deferred chain lifetime
        // becoming an alternative explanation for the second creation result.
        ComPtr<ID3D12Device> native_device;
        ComPtr<ID3D12CommandQueue> native_queue;
        D3D12_COMMAND_QUEUE_DESC queue_description{};
        queue_description.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;
        if (FAILED(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&native_device))) ||
            FAILED(native_device->CreateCommandQueue(&queue_description,IID_PPV_ARGS(&native_queue))))
            return fail("native paired-usage device/queue");
        const auto probe_usage=[&](UINT usage) {
            const auto probe=CreateWindowW(wc.lpszClassName,L"RSF private usage probe",WS_OVERLAPPED,
                0,0,int(width),int(height),nullptr,nullptr,module,nullptr);
            if (!probe) return HRESULT(E_FAIL);
            DXGI_SWAP_CHAIN_DESC descriptor{};
            descriptor.BufferDesc.Width=width; descriptor.BufferDesc.Height=height;
            descriptor.BufferDesc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;
            descriptor.SampleDesc.Count=1; descriptor.BufferUsage=usage;
            descriptor.BufferCount=3; descriptor.OutputWindow=probe;
            descriptor.Windowed=TRUE; descriptor.SwapEffect=DXGI_SWAP_EFFECT_FLIP_DISCARD;
            ComPtr<IDXGISwapChain> physical;
            const auto result=factory->CreateSwapChain(native_queue.Get(),&descriptor,&physical);
            std::printf("Native D3D12 paired creation usage=0x%x HRESULT=0x%08lx\n",usage,static_cast<unsigned long>(result));
            physical.Reset(); DestroyWindow(probe); return result;
        };
        const auto unordered=probe_usage(DXGI_USAGE_RENDER_TARGET_OUTPUT | DXGI_USAGE_SHADER_INPUT | DXGI_USAGE_UNORDERED_ACCESS);
        const auto control=probe_usage(DXGI_USAGE_RENDER_TARGET_OUTPUT);
        dxgi_errors(dxgi_diagnostics.Get());
        if (FAILED(control)) return fail("native paired usage render-target control");
        // Some runtimes may support the larger usage set. Preserve that result;
        // attribution of an invalid call requires the actual paired receipt.
        if (FAILED(unordered) && unordered!=DXGI_ERROR_INVALID_CALL) return fail("unexpected native unordered-usage refusal");
        if (dxgi_diagnostics) dxgi_diagnostics->ClearStoredMessages(diagnostic_all);
    }
    wchar_t temporary[MAX_PATH]{}, ini[MAX_PATH]{};
    if (!GetTempPathW(MAX_PATH, temporary) || !GetTempFileNameW(temporary, L"rsf", 0, ini)) return fail("create scratch INI path");
    const TemporaryIni remove_ini{ini};
    if (!WritePrivateProfileSectionW(L"Device", L"SR=0\0SRBackend=1\0SRQuality=0\0FG=0\0FGBackend=3\0FGGeneratedFrames=1\0Latency=0\0FrameLimitUs=0\0StatusOverlay=0\0", ini)) return fail("write scratch settings");
    const auto directory = root.wstring();
    rsf_d3d11_host_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = ini; setup.configuration_section = L"Device"; setup.runtime_directory = directory.c_str();
    setup.identity_utf8 = "shared-sr-device"; setup.executable_sha256 = "synthetic";
    setup.engine_version_utf8 = "1"; setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 1; setup.quality_choices = 0x1f; setup.depth_inverted = 1;
    setup.depth_far_plane = RSF_D3D11_HOST_DEPTH_INFINITE;
    setup.motion.struct_size = sizeof(setup.motion); setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1; setup.motion.invalid_value = -1000;
    setup.presentation_bridge = scoped ? 3u : 1u; setup.accept_window = accept;
    setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_RENDERED_SOURCE;
    rsf_game_host_services host{}; host.struct_size = sizeof(host); host.abi_version = RSF_GAME_ABI_VERSION;
    host.session_id = 91; host.log = log;
    if (rsf_d3d11_host_create(&setup, &host) != RSF_OK || rsf_d3d11_host_set_active(host.user, 1) != RSF_OK) return fail("create/activate production D3D11 host");
    rsf_d3d11_present_api presentation{sizeof(presentation), RSF_D3D11_PRESENT_API_VERSION};
    if (rsf_get_d3d11_present_api(&presentation) != RSF_OK) return fail("query production presentation API");
    DXGI_SWAP_CHAIN_DESC chain_desc{}; chain_desc.BufferDesc.Width = width; chain_desc.BufferDesc.Height = height;
    chain_desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; chain_desc.SampleDesc.Count = 1;
    chain_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; chain_desc.BufferCount = 2;
    if (scoped) chain_desc.BufferUsage |= DXGI_USAGE_SHADER_INPUT | DXGI_USAGE_UNORDERED_ACCESS;
    chain_desc.OutputWindow = window; chain_desc.Windowed = TRUE;
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context; ComPtr<IDXGISwapChain> chain;
    const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_1;
    rsf_d3d11_creation_scope scope{sizeof(scope),RSF_D3D11_HOST_ABI_VERSION,window,0};
    if (scoped) {
        if (rsf_d3d11_host_creation_begin(host.user,&scope) != RSF_OK || !scope.token) return fail("begin cancelled scope");
        rsf_d3d11_creation_scope nested{sizeof(nested),RSF_D3D11_HOST_ABI_VERSION,window,0};
        if (rsf_d3d11_host_creation_begin(host.user,&nested) == RSF_OK || nested.token) return fail("nested scope refused without changing output");
        auto wrong_token = scope; ++wrong_token.token;
        if (rsf_d3d11_host_creation_end(host.user,&wrong_token,nullptr) == RSF_OK)
            return fail("wrong-token End cannot close the real owner scope");
        if (rsf_d3d11_host_creation_end(host.user,&scope,nullptr) == RSF_OK || presentation.is_owner(nullptr)) return fail("empty scope cancels without admitting an owner");
        scope.token = 0;
        if (rsf_d3d11_host_creation_begin(host.user,&scope) != RSF_OK) return fail("begin malformed-result scope");
        const rsf_d3d11_native_chain malformed{sizeof(malformed)-1,RSF_D3D11_HOST_ABI_VERSION,nullptr,nullptr,nullptr};
        if (rsf_d3d11_host_creation_end(host.user,&scope,&malformed) != RSF_ERROR_INVALID_ARGUMENT)
            return fail("malformed result closes a valid scope before pointer access");
        scope.token = 0;
        if (rsf_d3d11_host_creation_begin(host.user,&scope) != RSF_OK) return fail("begin native scope after cancellation");
    }
    auto created = D3D11CreateDeviceAndSwapChain(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
        D3D11_CREATE_DEVICE_DEBUG, &level, 1, D3D11_SDK_VERSION, &chain_desc, &chain, &device, nullptr, &context);
    if (created == DXGI_ERROR_SDK_COMPONENT_MISSING)
        created = D3D11CreateDeviceAndSwapChain(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
            0, &level, 1, D3D11_SDK_VERSION, &chain_desc, &chain, &device, nullptr, &context);
    if (FAILED(created)) return fail("create hardware D3D11 device/swapchain");
    OuterChain* outer = nullptr;
    HRESULT application_uav=S_OK;
    if (scoped) {
        // Check emitted code, not just C++ call spelling: a hook installer that
        // follows the first entry jump must not land on the private continuation.
        auto** table = *reinterpret_cast<void***>(chain.Get());
        for (const unsigned slot : {2u,8u,10u,13u,22u,38u,39u}) {
            unsigned char entry[2]{}; SIZE_T copied = 0;
            if (!ReadProcessMemory(GetCurrentProcess(),table[slot],entry,sizeof(entry),&copied) || copied != sizeof(entry) ||
                entry[0] == 0xe9 || entry[0] == 0xeb || (entry[0] == 0xff && entry[1] == 0x25)) {
                std::printf("FAIL: public slot%u exposes an entry jump\n",slot);
                return fail("all external methods retain a call/return boundary");
            }
        }
        // Constructor-time queries must not create a real HWND chain or need Present.
        ComPtr<ID3D11Texture2D> buffer;
        const GUID annotation{0xb6420ea1,0x5912,0x4d17,{0xb1,0x65,0x0b,0x5a,0x28,0x68,0x73,0x64}};
        const unsigned char metadata[48]{};
        if (FAILED(chain->GetBuffer(0,IID_PPV_ARGS(&buffer))) ||
            FAILED(buffer->SetPrivateData(annotation,sizeof(metadata),metadata))) return fail("creation-time virtual buffer contract");
        D3D11_TEXTURE2D_DESC allocation{}; buffer->GetDesc(&allocation);
        ComPtr<ID3D11UnorderedAccessView> unordered;
        application_uav=device->CreateUnorderedAccessView(buffer.Get(),nullptr,&unordered);
        std::printf("Application usage=0x%x GetBuffer bind=0x%x UAV HRESULT=0x%08lx\n",
            chain_desc.BufferUsage,allocation.BindFlags,static_cast<unsigned long>(application_uav));
        DXGI_SWAP_CHAIN_DESC reported{};
        if (FAILED(chain->GetDesc(&reported)) || reported.BufferUsage!=chain_desc.BufferUsage)
            return fail("virtual descriptor preserves requested application usage");
        // Retain the failed contract result, but still exercise physical creation
        // so the same run discriminates application binds from native DXGI usage.
        if (presentation.is_owner(chain.Get())) return fail("inner owner cannot be admitted before End");
        const auto held_references = chain->AddRef();
        if (rsf_d3d11_host_fixture_check_unadmitted(host.user, chain.Get(), held_references) != 0) return 1;
        auto* inner = chain.Detach();
        outer = new OuterChain(inner,host.user); inner->Release(); chain.Attach(outer);
        void* original_output = chain.Get();
        const rsf_d3d11_native_chain outputs{sizeof(outputs),RSF_D3D11_HOST_ABI_VERSION,device.Get(),context.Get(),&original_output};
        if (rsf_d3d11_host_creation_end(host.user,&scope,&outputs) != RSF_OK || original_output != chain.Get() ||
            !presentation.is_owner(chain.Get()) || !presentation.is_owner(outer->inner.Get())) return fail("canonical outer admission preserves output");
        if (rsf_d3d11_host_fixture_check_outer(host.user, outer, &outer->refs) != 0) return 1;
    }
    // Native startup presents before the first scene. The bridge adopts its chain there.
    const auto first_present = chain->Present(0, 0);
    std::printf("Initial production Present HRESULT=0x%08lx\n",static_cast<unsigned long>(first_present));
    dxgi_errors(dxgi_diagnostics.Get());
    if (scoped && FAILED(first_present)) return fail("scoped physical creation/presentation");
    if (scoped && FAILED(application_uav)) return fail("requested application buffer UAV contract");
    if (outer && (outer->pre != 1 || outer->post != 1 || outer->dispatched != 1)) return fail("outer Present work and exact dispatch remain active");
    if (FAILED(first_present) || !presentation.is_owner(chain.Get()) || !presentation.host()) {
        std::printf("Shared presentation not established: 0x%08lx owner=%u host=%p\n",
            static_cast<unsigned long>(first_present), presentation.is_owner(chain.Get()),
            static_cast<void*>(presentation.host())); return fail("establish bridge-owned startup Present");
    }
    // Match the adapter's explicit native presentation binding before publishing
    // source CPU/FRAME facts. Scope admission alone does not bind this host.
    const rsf_d3d11_presentation_binding binding{sizeof(binding),RSF_D3D11_HOST_ABI_VERSION,chain.Get()};
    if (rsf_d3d11_host_bind_presentation(host.user,&binding) != RSF_OK)
        return fail("bind verified application presentation before source facts");
    if (rsf_d3d11_host_fixture_check_empty_aux(&host, context.Get()) != 0) return 1;

    if (outer && rsf_d3d11_host_fixture_check_inner(host.user, outer->inner.Get()) != 0) return 1;
    auto color = texture(device.Get(), DXGI_FORMAT_R11G11B10_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto motion = texture(device.Get(), DXGI_FORMAT_R16G16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto depth = texture(device.Get(), DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
    auto output = texture(device.Get(), DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    if (!color || !motion || !depth || !output) return fail("allocate native-format fixture textures");
    ComPtr<ID3D11RenderTargetView> color_view, motion_view, output_view;
    D3D11_DEPTH_STENCIL_VIEW_DESC ds{}; ds.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; ds.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    ComPtr<ID3D11DepthStencilView> depth_view;
    if (FAILED(device->CreateRenderTargetView(color.Get(), nullptr, &color_view)) ||
        FAILED(device->CreateRenderTargetView(motion.Get(), nullptr, &motion_view)) ||
        FAILED(device->CreateRenderTargetView(output.Get(), nullptr, &output_view)) ||
        FAILED(device->CreateDepthStencilView(depth.Get(), &ds, &depth_view))) return fail("create native-format fixture resource views");
    ComPtr<ID3D11InfoQueue> diagnostics; (void)device.As(&diagnostics);
    std::printf("D3D11 debug queue available=%u\n", diagnostics ? 1u : 0u);
    if (diagnostics) diagnostics->ClearStoredMessages();
    rsf_overlay_api overlay{sizeof(overlay), RSF_OVERLAY_API_VERSION};
    if (rsf_get_overlay_api(&overlay) != RSF_OK) return fail("query production overlay API");
    const DirectX::PackedVector::XMFLOAT3PK native_pixel(0.25f, 0.5f, 0.75f), outside_marker(3.0f, 0.0f, 3.0f);
    const rsf_d3d11_fixture_resources resources{
        &host, device.Get(), context.Get(), chain.Get(), color.Get(), motion.Get(), depth.Get(), output.Get(),
        color_view.Get(), motion_view.Get(), depth_view.Get(), output_view.Get(), width, height,
        diagnostics.Get(), []() noexcept { DestroyWindow(window); },
    };
    struct NativeCalls {
        const rsf_d3d11_present_api* presentation;
        const OuterChain* outer;
        const rsf_overlay_api* overlay;
    };
    NativeCalls native_calls{&presentation, outer, &overlay};
    const rsf_d3d11_fixture_fg_calls calls{
        &native_calls,
        [](void* receiver, void** out) noexcept -> int32_t {
            try {
                ComPtr<ID3D11Texture2D> texture;
                const auto result = static_cast<IDXGISwapChain*>(receiver)->GetBuffer(0, IID_PPV_ARGS(&texture));
                if (SUCCEEDED(result)) *out = texture.Detach();
                return result;
            } catch (...) { return E_FAIL; }
        },
        [](void* receiver) noexcept -> int32_t {
            try { return static_cast<IDXGISwapChain*>(receiver)->Present(0, 0); }
            catch (...) { return E_FAIL; }
        },
        [](void* user) noexcept -> int32_t {
            try {
                const auto& api = *static_cast<const NativeCalls*>(user)->presentation;
                return api.provider() && api.session();
            } catch (...) { return -1; }
        },
        [](void* user) noexcept -> uint32_t {
            try { return static_cast<const NativeCalls*>(user)->presentation->backend(); }
            catch (...) { return UINT32_MAX; }
        },
        outer ? +[](void* user, uint32_t* pre, uint32_t* post, uint32_t* dispatched) noexcept {
            const auto& chain = *static_cast<const NativeCalls*>(user)->outer;
            *pre = chain.pre; *post = chain.post; *dispatched = chain.dispatched;
        } : nullptr,
        []() noexcept -> int32_t {
            try {
                MSG message{};
                while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                return RSF_OK;
            } catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](void* user, rsf_render_settings* settings) noexcept -> int32_t {
            try { return static_cast<const NativeCalls*>(user)->overlay->settings_get(settings); }
            catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](void* user, const rsf_render_settings* settings) noexcept -> int32_t {
            try { return static_cast<const NativeCalls*>(user)->overlay->settings_apply(settings); }
            catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](void* user, int32_t* result) noexcept -> int32_t {
            try {
                *result = static_cast<const NativeCalls*>(user)->presentation->switch_result();
                return RSF_OK;
            } catch (...) { return RSF_ERROR_NOT_READY; }
        },
        [](void* user, uint64_t* generation) noexcept -> int32_t {
            try {
                *generation = static_cast<const NativeCalls*>(user)->presentation->generation();
                return RSF_OK;
            } catch (...) { return RSF_ERROR_NOT_READY; }
        },
    };
    return rsf_d3d11_host_fixture_rendered_fg(&resources, &calls, uint32_t(scoped), first_present,
        native_pixel.v, outside_marker.v);
}
