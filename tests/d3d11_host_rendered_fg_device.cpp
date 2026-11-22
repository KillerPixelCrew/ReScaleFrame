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
#include <cmath>
#include <algorithm>
#include <cstdio>
#include <thread>
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
void identity(float* matrix) { matrix[0] = matrix[5] = matrix[10] = matrix[15] = 1; }
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
void cpu(const rsf_game_host_services& host, uint64_t source, uint32_t stage) {
    rsf_d3d11_host_fixture_cpu(&host, source, stage, uint32_t(stage == RSF_GAME_CPU_FRAME_END));
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
        rsf_d3d11_chain_dispatch typed{}; typed.struct_size = sizeof(typed); typed.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        typed.receiver = chain.Get(); typed.method = RSF_D3D11_DISPATCH_RELEASE;
        const auto held_references = chain->AddRef();
        if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled ||
            typed.result.references != held_references-1) return fail("unadmitted Release consumes exactly one reference");
        for (uint32_t method = RSF_D3D11_DISPATCH_PRESENT; method <= RSF_D3D11_DISPATCH_RESIZE_BUFFERS1; ++method) {
            typed.method = method; typed.handled = 0;
            if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled ||
                typed.result.hresult != DXGI_ERROR_INVALID_CALL) return fail("known unadmitted method cannot fall through");
        }
        rsf_d3d11_present_dispatch refused{sizeof(refused),RSF_D3D11_HOST_ABI_VERSION,chain.Get(),0,0,0,S_OK};
        if (rsf_d3d11_host_dispatch_present(host.user,&refused) != RSF_OK || !refused.handled ||
            refused.native_result != DXGI_ERROR_INVALID_CALL) return fail("unadmitted facade remains handled with failure");
        refused.flags = DXGI_PRESENT_TEST;
        if (rsf_d3d11_host_dispatch_present(host.user,&refused) != RSF_OK || !refused.handled ||
            refused.native_result != DXGI_ERROR_INVALID_CALL) return fail("unadmitted TEST cannot fall through to native code");
        auto* inner = chain.Detach();
        outer = new OuterChain(inner,host.user); inner->Release(); chain.Attach(outer);
        void* original_output = chain.Get();
        const rsf_d3d11_native_chain outputs{sizeof(outputs),RSF_D3D11_HOST_ABI_VERSION,device.Get(),context.Get(),&original_output};
        if (rsf_d3d11_host_creation_end(host.user,&scope,&outputs) != RSF_OK || original_output != chain.Get() ||
            !presentation.is_owner(chain.Get()) || !presentation.is_owner(outer->inner.Get())) return fail("canonical outer admission preserves output");
        rsf_d3d11_present_dispatch alias{sizeof(alias),RSF_D3D11_HOST_ABI_VERSION,outer,0,0,1,123};
        if (rsf_d3d11_host_dispatch_present(host.user,&alias) != RSF_OK || alias.handled || alias.native_result != 123)
            return fail("canonical outer alias cannot bypass its own Present work");
        alias.receiver = reinterpret_cast<void*>(uintptr_t(1)); alias.handled = 1;
        if (rsf_d3d11_host_dispatch_present(host.user,&alias) != RSF_OK || alias.handled || alias.native_result != 123)
            return fail("unknown receiver is not dereferenced with a live owner");
        typed.receiver = outer;
        const auto outer_refs = outer->refs;
        for (uint32_t method = RSF_D3D11_DISPATCH_RELEASE; method <= RSF_D3D11_DISPATCH_RESIZE_BUFFERS1; ++method) {
            typed.method = method; typed.result.references = 123;
            if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || typed.handled ||
                typed.result.references != 123 || outer->refs != outer_refs)
                return fail("typed dispatch preserves every outer wrapper method and reference");
        }
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
    rsf_d3d11_auxiliary_scope empty_cleanup{sizeof(empty_cleanup),RSF_D3D11_HOST_ABI_VERSION,host.session_id,1,1};
    if (rsf_d3d11_host_restore_auxiliaries(host.user,&empty_cleanup)!=RSF_OK ||
        rsf_d3d11_host_restore_auxiliaries(host.user,&empty_cleanup)!=RSF_OK)
        return fail("verified owner acknowledges empty cleanup before preparation and on repeat");
    auto wrong_session=empty_cleanup; ++wrong_session.session_id;
    if (rsf_d3d11_host_restore_auxiliaries(host.user,&wrong_session)!=RSF_ERROR_NOT_READY)
        return fail("empty cleanup refuses a foreign session");
    int32_t foreign_thread_result=RSF_OK;
    std::thread wrong_thread([&] { foreign_thread_result=rsf_d3d11_host_restore_auxiliaries(host.user,&empty_cleanup); });
    wrong_thread.join();
    if (foreign_thread_result!=RSF_ERROR_NOT_READY) return fail("empty cleanup refuses a foreign graphics thread");
    rsf_d3d11_auxiliary_pass failed_before_prepare{sizeof(failed_before_prepare),RSF_D3D11_HOST_ABI_VERSION,nullptr,0,nullptr};
    if (rsf_d3d11_host_upscale_auxiliaries(host.user,context.Get(),&failed_before_prepare)!=RSF_ERROR_INVALID_ARGUMENT ||
        rsf_d3d11_host_restore_auxiliaries(host.user,&empty_cleanup)!=RSF_OK)
        return fail("failure before auxiliary preparation acknowledges no retained lease");

    if (outer) {
        rsf_d3d11_chain_dispatch typed{}; typed.struct_size = sizeof(typed); typed.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        typed.receiver = outer->inner.Get(); typed.method = RSF_D3D11_DISPATCH_PRESENT1;
        DXGI_PRESENT_PARAMETERS dirty{}; dirty.DirtyRectsCount = 1;
        typed.args.present1.parameters = &dirty;
        if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled ||
            typed.result.hresult != DXGI_ERROR_INVALID_CALL) return fail("Present1 preserves dirty-region refusal");
        typed.args = {}; typed.method = RSF_D3D11_DISPATCH_RESIZE_BUFFERS;
        typed.args.resize_buffers.buffer_count = 99;
        if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled ||
            typed.result.hresult != DXGI_ERROR_INVALID_CALL) return fail("ResizeBuffers preserves invalid-count refusal");
        typed.args = {}; typed.method = RSF_D3D11_DISPATCH_RESIZE_BUFFERS1;
        const uint32_t node_mask = 1; typed.args.resize_buffers1.node_masks = &node_mask;
        if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled ||
            typed.result.hresult != DXGI_ERROR_INVALID_CALL) return fail("ResizeBuffers1 preserves native-array refusal");
        typed.args = {}; typed.method = RSF_D3D11_DISPATCH_SET_FULLSCREEN_STATE;
        if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled || FAILED(typed.result.hresult))
            return fail("SetFullscreenState uses the actual physical owner");
        typed.method = RSF_D3D11_DISPATCH_SET_COLOR_SPACE1;
        typed.args.color_space.color_space = DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709;
        if (rsf_d3d11_host_dispatch_chain(host.user,&typed) != RSF_OK || !typed.handled || FAILED(typed.result.hresult))
            return fail("SetColorSpace1 uses the actual physical owner");
    }
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
    UINT active_width = width, active_height = height;
    bool native_reinsertion = false;
    const DirectX::PackedVector::XMFLOAT3PK native_pixel(0.25f, 0.5f, 0.75f), outside_marker(3.0f, 0.0f, 3.0f);
    std::vector<uint32_t> native_pixels(size_t(width) * height);
    uint64_t source = 0, captured = 0, sr_evaluated = 0, app_presents = 0;
    bool failed = false, unavailable = false, occluded = first_present == DXGI_STATUS_OCCLUDED;
    const auto verify = [&](bool condition, const char* label) {
        if (!condition) { std::printf("FAIL: %s\n", label); failed = true; }
    };
    bool refuse_inputs_late = false, refuse_depth_late = false, exercise_auxiliary_cleanup = false;
    auto frame = [&](bool reconstruct, bool primary, bool reset, int expect_capture) {
        ++source;
        cpu(host, source, RSF_GAME_CPU_FRAME_BEGIN);
        cpu(host, source, RSF_GAME_CPU_SIMULATION_END);
        const float scene_color[]{0.25f, 0.5f, 0.75f, 1}, zero[4]{};
        if (native_reinsertion) {
            std::fill(native_pixels.begin(), native_pixels.end(), outside_marker.v);
            for (UINT row = 0; row < active_height; ++row)
                std::fill_n(native_pixels.begin() + size_t(row) * width, active_width, native_pixel.v);
            context->UpdateSubresource(color.Get(), 0, nullptr, native_pixels.data(), width * UINT(sizeof(uint32_t)), 0);
        } else context->ClearRenderTargetView(color_view.Get(), scene_color);
        context->ClearRenderTargetView(motion_view.Get(), zero);
        context->ClearDepthStencilView(depth_view.Get(), D3D11_CLEAR_DEPTH, 0.5f, 0);
        rsf_game_render_pass pass{}; pass.struct_size = sizeof(pass);
        pass.session_id = host.session_id; pass.source_frame_id = pass.submission_id = pass.scope_id = source;
        pass.family_key = pass.view_key = pass.history_key = pass.viewport_key = pass.window_key = pass.rhi_viewport_key = 1;
        pass.native_frame = source; pass.screen = RSF_SCREEN_GAMEPLAY;
        pass.flags = RSF_GAME_RENDER_AFTER_SIMULATION | (primary ? RSF_GAME_RENDER_PRIMARY : RSF_GAME_RENDER_NO_FG);
        if (reset) pass.flags |= RSF_GAME_RENDER_RESET;
        pass.swapchain = chain.Get(); pass.color_input = color.Get(); pass.color_output = native_reinsertion ? color.Get() : output.Get();
        pass.depth = depth.Get(); pass.motion = motion.Get(); pass.camera_valid = pass.motion_camera_included = 1;
        pass.motion_to_uv[0] = pass.motion_to_uv[1] = -1;
        auto& camera = pass.camera; camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        identity(camera.view_to_clip); identity(camera.clip_to_view); identity(camera.world_to_view);
        identity(camera.view_to_world); identity(camera.clip_to_previous_clip); identity(pass.previous_clip_to_clip);
        camera.view_to_clip[0] = float(height) / float(width); camera.clip_to_view[0] = float(width) / float(height);
        camera.near_plane = 0.1f; camera.far_plane = 1000; camera.vertical_fov_radians = 1.2f;
        camera.frame_time_seconds = 1.0f / 60; camera.depth_inverted = 1;
        camera.render_width = active_width; camera.render_height = active_height;
        camera.output_width = width; camera.output_height = height;
        pass.render_rect[2] = int32_t(active_width); pass.render_rect[3] = int32_t(active_height);
        pass.output_rect[2] = int32_t(width); pass.output_rect[3] = int32_t(height);
        // FRAME precedes every source-owned GPU input and SR callback.
        pass.role = RSF_GAME_RENDER_FRAME; host.render_pass(host.user, context.Get(), &pass, 1);
        pass.role = RSF_GAME_RENDER_SUBMISSION; host.render_pass(host.user, context.Get(), &pass, 1);
        if (scoped) {
            auto& domains=pass.depth_domains;
            domains.struct_size=sizeof(domains); domains.version=RSF_DEPTH_DOMAINS_VERSION; domains.domain_count=1;
            auto& domain=domains.domains[0]; domain.classify_max=domain.viewport_max_depth=1; domain.endpoint_flags=3;
            domain.viewport_xywh[2]=float(active_width); domain.viewport_xywh[3]=float(active_height);
            identity(domain.source_clip_to_target_clip);
        }
        pass.role = RSF_GAME_RENDER_FG_INPUTS;
        host.render_pass(host.user, context.Get(), &pass, 1);
        host.render_pass(host.user, context.Get(), &pass, 0);
        const bool held = rsf_native_fg_inputs_ready(source) != 0;
        if (held) ++captured;
        if (expect_capture > 0 && !held) {
            if (!presentation.provider() || !presentation.session()) unavailable = true;
            else { std::puts("FAIL: requested native FG inputs were not retained"); failed = true; }
        }
        if (expect_capture == 0 && held) { std::puts("FAIL: FG Off still retained pure FG inputs"); failed = true; }
        // Match the native adapter: retain inputs before SR can mutate them.
        auto sr_pass = pass; sr_pass.role = RSF_GAME_RENDER_SR;
        sr_pass.flags |= RSF_GAME_RENDER_NO_FG; // SR metadata does not grant FG eligibility.
        if (scoped) sr_pass.depth_domains.domains[0].source_clip_to_target_clip[14]=.125f;
        rsf_d3d11_auxiliary_surface auxiliary{sizeof(auxiliary),RSF_D3D11_AUXILIARY_COLOUR,motion.Get(),
            {0,0,float(active_width),float(active_height)},{0,0,int32_t(width),int32_t(height)},{0,0}};
        auxiliary.flags=RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
        const rsf_d3d11_auxiliary_pass auxiliary_request{sizeof(auxiliary_request),RSF_D3D11_HOST_ABI_VERSION,&sr_pass,1,&auxiliary};
        const auto sr_result = exercise_auxiliary_cleanup ?
            rsf_d3d11_host_upscale_auxiliaries(host.user,context.Get(),&auxiliary_request) :
            host.process_upscale(host.user, context.Get(), &sr_pass);
        if (exercise_auxiliary_cleanup) {
            const rsf_d3d11_auxiliary_scope cleanup{sizeof(cleanup),RSF_D3D11_HOST_ABI_VERSION,host.session_id,source,1};
            verify(sr_result==RSF_OK,"warmed auxiliary transaction commits before cleanup checks");
            auto mismatch=cleanup; ++mismatch.native_frame;
            verify(rsf_d3d11_host_restore_auxiliaries(host.user,&mismatch)==RSF_ERROR_NOT_READY,
                "mismatched pending auxiliary scope refuses without releasing ownership");
            mismatch=cleanup; ++mismatch.view_key;
            verify(rsf_d3d11_host_restore_auxiliaries(host.user,&mismatch)==RSF_ERROR_NOT_READY,"pending cleanup refuses wrong view");
            mismatch=cleanup; ++mismatch.session_id;
            verify(rsf_d3d11_host_restore_auxiliaries(host.user,&mismatch)==RSF_ERROR_NOT_READY,"pending cleanup refuses foreign session");
            int32_t thread_result=RSF_OK;
            std::thread other([&] { thread_result=rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup); }); other.join();
            verify(thread_result==RSF_ERROR_NOT_READY,"pending cleanup refuses foreign thread");
            verify(rsf_d3d11_host_upscale_auxiliaries(host.user,context.Get(),&auxiliary_request)==RSF_ERROR_BUSY,
                "refused cleanup leaves the original pending transaction owned");
            verify(rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_OK &&
                rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_OK,"successful cleanup and empty repeat acknowledge OK");
            D3D11_TEXTURE2D_DESC staging_desc{}; output->GetDesc(&staging_desc);
            staging_desc.Usage=D3D11_USAGE_STAGING; staging_desc.BindFlags=staging_desc.MiscFlags=0;
            staging_desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
            ComPtr<ID3D11Texture2D> invalid_mirror;
            verify(SUCCEEDED(device->CreateTexture2D(&staging_desc,nullptr,&invalid_mirror)),"create unsupported readable mirror for rollback");
            auto rollback_pass=sr_pass; rollback_pass.color_output_readable=invalid_mirror.Get();
            auto invalid_request=auxiliary_request; invalid_request.pass=&rollback_pass;
            verify(rsf_d3d11_host_upscale_auxiliaries(host.user,context.Get(),&invalid_request)!=RSF_OK,
                "second destination failure rolls back partially prepared auxiliary transaction");
            verify(rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_OK &&
                rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_OK,"rolled-back empty cleanup and repeat acknowledge OK");
        }
        if (reconstruct && sr_result == RSF_OK) ++sr_evaluated;
        if (reconstruct && native_reinsertion) verify(sr_result == RSF_OK, "warmed reduced SR must reinsert into native colour allocation");
        if (refuse_depth_late) {
            auto malformed=pass; ++malformed.depth_domains.version;
            host.render_pass(host.user,context.Get(),&malformed,1);
            host.render_pass(host.user,context.Get(),&malformed,0);
        }
        if (refuse_inputs_late) {
            auto refused = pass; refused.role = RSF_GAME_RENDER_FG_INPUTS; refused.flags |= RSF_GAME_RENDER_NO_FG;
            host.render_pass(host.user,context.Get(),&refused,1);
            host.render_pass(host.user,context.Get(),&refused,0);
            // A subsequent duplicate eligible lease cannot undo the genuine refusal.
            pass.role = RSF_GAME_RENDER_FG_INPUTS;
            host.render_pass(host.user,context.Get(),&pass,1);
            host.render_pass(host.user,context.Get(),&pass,0);
        }
        ComPtr<ID3D11Texture2D> backbuffer;
        if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&backbuffer)))) { verify(false, "GetBuffer for final scene"); return; }
        ComPtr<ID3D11RenderTargetView> backbuffer_view;
        if (FAILED(device->CreateRenderTargetView(backbuffer.Get(), nullptr, &backbuffer_view))) { verify(false, "CreateRenderTargetView for final scene"); return; }
        context->ClearRenderTargetView(backbuffer_view.Get(), scene_color);
        pass.role = RSF_GAME_RENDER_FINAL_SCENE; pass.scene_surface = backbuffer.Get();
        host.render_pass(host.user, context.Get(), &pass, 1);
        host.render_pass(host.user, context.Get(), &pass, 0);
        pass.role = RSF_GAME_RENDER_SUBMISSION; host.render_pass(host.user, context.Get(), &pass, 0);
        pass.role = RSF_GAME_RENDER_WINDOW; pass.scene_surface = nullptr;
        host.render_pass(host.user, context.Get(), &pass, 1);
        const auto result = chain->Present(0, 0); ++app_presents;
        if (outer) {
            rsf_native_present_record observed{}; observed.struct_size = sizeof(observed);
            verify(rsf_native_window_last_present(&observed) && observed.matched && observed.source_frame_id == source,
                "actual inner Present matches the outer WINDOW canonical identity");
            verify(outer->pre == app_presents + 1 && outer->post == outer->pre && outer->dispatched == outer->pre,
                "outer pre/post work survives every source and generation switch");
        }
        if (FAILED(result)) { std::printf("FAIL: Present HRESULT=0x%08lx\n", static_cast<unsigned long>(result)); failed = true; }
        occluded |= result == DXGI_STATUS_OCCLUDED;
        host.render_pass(host.user, context.Get(), &pass, 0);
        pass.role = RSF_GAME_RENDER_FRAME; host.render_pass(host.user, context.Get(), &pass, 0);
        cpu(host, source, RSF_GAME_CPU_FRAME_END);
        rsf_native_cpu_frame cpu_frame{}; cpu_frame.struct_size = sizeof(cpu_frame);
        const auto cpu_ok = rsf_native_cpu_read(host.session_id, source, &cpu_frame);
        if (!cpu_ok || cpu_frame.pacing_qpc || cpu_frame.input_ready_qpc || cpu_frame.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE]) {
            std::printf("CPU timing failure: source=%llu query=%d pacing_qpc=%llu input_ready_qpc=%llu input_sample_qpc=%llu\n",
                static_cast<unsigned long long>(source), cpu_ok, static_cast<unsigned long long>(cpu_frame.pacing_qpc),
                static_cast<unsigned long long>(cpu_frame.input_ready_qpc),
                static_cast<unsigned long long>(cpu_frame.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE]));
            verify(cpu_ok != 0, "source CPU snapshot query");
            verify(!cpu_frame.pacing_qpc, "source CPU pacing QPC must remain zero");
            verify(!cpu_frame.input_ready_qpc, "source CPU input-ready QPC must remain zero");
            verify(!cpu_frame.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE], "source CPU physical input sample must remain unknown");
        }
        rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg);
        if (!rsf_native_fg_status_get(&fg)) { verify(false, "per-source FG status query"); return; }
        std::printf("source=%llu SR=%d held=%u backend=%u reason=%u result=%d tagged=%llu present_callbacks=%llu generated_callbacks=%llu statistics_flags=0x%x vendor_generated_presented=%llu\n",
            static_cast<unsigned long long>(source), sr_result, held ? 1u : 0u, presentation.backend(), fg.reason,
            fg.last_result, static_cast<unsigned long long>(fg.tagged_frames),
            static_cast<unsigned long long>(fg.vendor.present_callbacks), static_cast<unsigned long long>(fg.vendor.generated_callbacks),
            fg.vendor.valid_statistics, static_cast<unsigned long long>(fg.vendor.generated_presented));
        std::printf("source_status=%llu provenance=%u input_qpc=%llu source_qpc=%llu frequency=%llu requested_mode=%u generated=%u reflex=%u limit_us=%u effective_mode=%u effective_reflex=%u pacing_owner=%u paced=%llu\n",
            static_cast<unsigned long long>(fg.source_frame_id), fg.source_provenance,
            static_cast<unsigned long long>(fg.input_qpc), static_cast<unsigned long long>(fg.source_qpc),
            static_cast<unsigned long long>(fg.source_qpc_frequency), fg.requested.mode, fg.requested.generated_frames,
            fg.requested.reflex_mode, fg.requested.frame_limit_us, fg.vendor.effective_mode, fg.vendor.effective_reflex,
            fg.vendor.pacing_owner, static_cast<unsigned long long>(fg.vendor.latency_paced_frames));
        verify(!fg.input_qpc, "FG status physical input QPC must remain unknown");
        verify(!fg.vendor.latency_paced_frames, "provider latency paced count must remain zero");
        verify(fg.vendor.pacing_owner == RSF_PACING_NONE, "provider pacing owner must be NONE");
        verify(fg.vendor.effective_reflex == RSF_REFLEX_OFF, "provider effective Reflex must be Off");
        verify(fg.requested.reflex_mode == RSF_REFLEX_OFF, "requested Reflex must be Off");
        verify(!fg.requested.frame_limit_us, "requested limiter must be zero");
        if (fg.source_frame_id == source && fg.tagged_frames) {
            verify(fg.source_provenance == RSF_FRAME_SOURCE_RENDERED, "tagged current source must declare rendered provenance");
            verify(fg.source_qpc != 0, "tagged current source must carry native source QPC");
            verify(fg.source_qpc_frequency != 0, "tagged current source must carry source QPC frequency");
        }
        MSG message{}; while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&message); DispatchMessageW(&message); }
    };
    // Presentation was bound above without claiming scene/SR admission. Bootstrap
    // is one complete source with FG/SR Off, not a hidden warmup or successful SR source.
    std::puts("Phase: one complete source-owned bootstrap scene, SR/FG Off");
    frame(false, true, true, 0);
    const uint64_t bootstrap_sources = source;
    verify(bootstrap_sources == 1 && app_presents == 1 && !captured && !sr_evaluated,
        "bootstrap must complete exactly one application Present without capture or SR evaluation");
    if (failed) return fail("complete source-owned bootstrap admission");
    rsf_render_settings settings{}; settings.struct_size = sizeof(settings); settings.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    if (overlay.settings_get(&settings) != RSF_OK) return fail("read initial warmup settings");
    settings.sr_enabled = 1; settings.sr_backend = RSF_SR_DLSS; settings.sr_quality = 1;
    settings.fg_enabled = 0; settings.fg_backend = RSF_FG_BACKEND_FSR3; settings.generated_frames = 1;
    settings.latency = RSF_REFLEX_OFF; settings.frame_limit_us = 0;
    if (overlay.settings_apply(&settings) != RSF_OK) return fail("apply reduced DLSS warmup with FG Off");
    active_width = 640; active_height = 360; native_reinsertion = true;
    std::puts("Phase: bounded 10-source DLSS Quality warmup, render640x360 in960x540 native colour allocation, FG Off");
    const auto warmup_evaluated_before = sr_evaluated;
    for (unsigned i = 0; i < 10 && !failed; ++i) {
        exercise_auxiliary_cleanup=scoped && i==9;
        frame(true,true,i==0,0);
    }
    exercise_auxiliary_cleanup=false;
    verify(sr_evaluated == warmup_evaluated_before + 10, "all bounded reduced warmup sources must evaluate SR");
    // Inspect an output pixel outside the active input rectangle: reinsertion must
    // overwrite the marker in the same R11G11B10 native allocation, not a side target.
    D3D11_TEXTURE2D_DESC native_desc{}; color->GetDesc(&native_desc);
    native_desc.BindFlags = 0; native_desc.Usage = D3D11_USAGE_STAGING; native_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> native_readback;
    if (FAILED(device->CreateTexture2D(&native_desc, nullptr, &native_readback))) return fail("create native reinsertion readback");
    context->CopyResource(native_readback.Get(), color.Get());
    D3D11_MAPPED_SUBRESOURCE native_mapped{};
    if (FAILED(context->Map(native_readback.Get(), 0, D3D11_MAP_READ, 0, &native_mapped))) return fail("map native reinsertion readback");
    const auto outside_pixel = reinterpret_cast<const uint32_t*>(static_cast<const unsigned char*>(native_mapped.pData) + native_mapped.RowPitch * (height - 2))[width - 2];
    context->Unmap(native_readback.Get(), 0);
    std::printf("Native reinsertion outside-active pixel=0x%08x original_marker=0x%08x\n", outside_pixel, outside_marker.v);
    verify(outside_pixel != outside_marker.v, "DLSS reinsertion must overwrite native allocation outside reduced input rectangle");
    rsf_native_fg_status warmed{}; warmed.struct_size = sizeof(warmed);
    verify(rsf_native_fg_status_get(&warmed) != 0, "read warmed FG-Off status");
    verify(warmed.requested.mode == RSF_FG_OFF && !warmed.tagged_frames, "warmup must remain FG Off and untagged");
    const auto presents_before_late_enable = app_presents;
    const auto captured_before_late_enable = captured;
    const auto generation_before_late_enable = presentation.generation();
    settings.fg_enabled = 1;
    if (overlay.settings_apply(&settings) != RSF_OK) return fail("enable FSR3 through public overlay after warmed reduced DLSS");
    std::puts("Phase: bounded 10-source late FSR3 activation over warmed reduced in-place DLSS");
    for (unsigned i = 0; i < 10 && !failed; ++i) frame(true, true, false, i < 2 ? -1 : 1);
    rsf_native_fg_status late_enabled{}; late_enabled.struct_size = sizeof(late_enabled);
    verify(rsf_native_fg_status_get(&late_enabled) != 0, "read post-switch reduced FG status");
    std::printf("Late-enable receipt: backend=%u switch_result=%d generation=%llu previous_generation=%llu captures=%llu application_calls=%llu tags=%llu generated_callbacks=%llu statistics_flags=0x%x vendor_generated_presented=%llu\n",
        presentation.backend(), presentation.switch_result(), static_cast<unsigned long long>(presentation.generation()),
        static_cast<unsigned long long>(generation_before_late_enable), static_cast<unsigned long long>(captured - captured_before_late_enable),
        static_cast<unsigned long long>(app_presents - presents_before_late_enable), static_cast<unsigned long long>(late_enabled.tagged_frames),
        static_cast<unsigned long long>(late_enabled.vendor.generated_callbacks), late_enabled.vendor.valid_statistics,
        static_cast<unsigned long long>(late_enabled.vendor.generated_presented));
    verify(app_presents == presents_before_late_enable + 10, "late-enable phase must complete ten application Presents");
    if (presentation.backend() != RSF_FG_BACKEND_FSR3 || !late_enabled.vendor.generated_callbacks) unavailable = true;
    else {
        verify(presentation.switch_result() == RSF_BACKEND_OK, "late FSR switch must succeed");
        verify(presentation.generation() > generation_before_late_enable, "late FSR switch must advance provider generation");
        verify(captured > captured_before_late_enable && late_enabled.tagged_frames > warmed.tagged_frames,
            "late activation must capture and tag reduced native inputs");
    }
    const bool late_generated = late_enabled.tagged_frames > warmed.tagged_frames && late_enabled.vendor.generated_callbacks != 0;
    // Preserve the original native-resolution SR-Off/SR-On and refusal phases.
    settings.sr_enabled = 0; settings.sr_quality = 0;
    if (overlay.settings_apply(&settings) != RSF_OK) return fail("start original SR-Off generation phase");
    active_width = width; active_height = height; native_reinsertion = false;
    // More than the six-slot transfer ring; first source explicitly invalidates history.
    for (unsigned i = 0; i < 12 && !failed; ++i) frame(false, true, i == 0, i == 0 ? -1 : 1);
    rsf_native_fg_status off_sr{}; off_sr.struct_size = sizeof(off_sr);
    if (!rsf_native_fg_status_get(&off_sr)) return fail("query SR-Off FG status");
    const bool off_sr_generated = off_sr.tagged_frames > late_enabled.tagged_frames &&
        off_sr.vendor.generated_callbacks > late_enabled.vendor.generated_callbacks;
    if (off_sr_generated) verify(presentation.backend() == RSF_FG_BACKEND_FSR3, "generated SR-Off phase must use FSR3");
    std::printf("SR-Off receipt: tagged=%llu generated_callbacks=%llu provenance=%u input_qpc=%llu\n",
        static_cast<unsigned long long>(off_sr.tagged_frames),
        static_cast<unsigned long long>(off_sr.vendor.generated_callbacks), off_sr.source_provenance,
        static_cast<unsigned long long>(off_sr.input_qpc));
    if (overlay.settings_get(&settings) != RSF_OK) return fail("read shared rendering settings");
    settings.sr_enabled = 1; settings.sr_quality = 0;
    if (overlay.settings_apply(&settings) != RSF_OK) return fail("apply shared rendering settings");
    for (unsigned i = 0; i < 12 && !failed; ++i) frame(true, true, i == 0, true);
    // Explicit reset followed by continuity, then a non-primary source must not tag.
    rsf_native_fg_status before_reset{}; before_reset.struct_size = sizeof(before_reset);
    if (!rsf_native_fg_status_get(&before_reset)) return fail("query before-reset FG status");
    if (!failed) frame(true, true, true, true);
    rsf_native_fg_status after_reset{}; after_reset.struct_size = sizeof(after_reset);
    verify(rsf_native_fg_status_get(&after_reset) != 0, "after-reset FG status query");
    std::printf("Reset tags: before=%llu after=%llu\n", static_cast<unsigned long long>(before_reset.tagged_frames), static_cast<unsigned long long>(after_reset.tagged_frames));
    verify(after_reset.tagged_frames == before_reset.tagged_frames, "explicit reset must not increment tag count");
    if (!failed) frame(true, true, false, true);
    rsf_native_fg_status before_refusal{}; before_refusal.struct_size = sizeof(before_refusal);
    if (!rsf_native_fg_status_get(&before_refusal)) return fail("query before-late-refusal FG status");
    refuse_inputs_late = true;
    if (!failed) frame(true,true,false,true);
    refuse_inputs_late = false;
    rsf_native_fg_status after_refusal{}; after_refusal.struct_size = sizeof(after_refusal);
    verify(rsf_native_fg_status_get(&after_refusal) != 0, "after-late-refusal FG status query");
    verify(after_refusal.tagged_frames == before_refusal.tagged_frames && after_refusal.reason == RSF_NATIVE_FG_REASON_RESET,
        "genuine late FG refusal remains sticky despite eligible duplicate and WINDOW");
    if (!failed) frame(true,true,false,true);
    if (scoped) {
        rsf_native_fg_status before_depth{}; before_depth.struct_size=sizeof(before_depth);
        verify(rsf_native_fg_status_get(&before_depth)!=0,"query before malformed repeated depth lease");
        refuse_depth_late=true;
        if (!failed) frame(true,true,false,true);
        refuse_depth_late=false;
        rsf_native_fg_status after_depth{}; after_depth.struct_size=sizeof(after_depth);
        verify(rsf_native_fg_status_get(&after_depth)!=0 && after_depth.tagged_frames==before_depth.tagged_frames &&
            after_depth.reason==RSF_NATIVE_FG_REASON_RESET,
            "refused repeated depth descriptor cannot authorize retained same-source inputs");
        if (!failed) frame(true,true,false,true);
    }
    rsf_native_fg_status before_invalid{}; before_invalid.struct_size = sizeof(before_invalid);
    if (!rsf_native_fg_status_get(&before_invalid)) return fail("query before-nonprimary FG status");
    if (!failed) frame(true, false, false, -1);
    rsf_native_fg_status after_invalid{}; after_invalid.struct_size = sizeof(after_invalid);
    verify(rsf_native_fg_status_get(&after_invalid) != 0, "after-nonprimary FG status query");
    std::printf("Nonprimary tags: before=%llu after=%llu\n", static_cast<unsigned long long>(before_invalid.tagged_frames), static_cast<unsigned long long>(after_invalid.tagged_frames));
    verify(after_invalid.tagged_frames == before_invalid.tagged_frames, "nonprimary source must not increment tag count");
    settings.fg_enabled = 0; settings.sr_enabled = 0;
    if (overlay.settings_apply(&settings) != RSF_OK) return fail("apply shared rendering settings");
    for (unsigned i = 0; i < 3 && !failed; ++i) frame(false, true, false, false);
    rsf_native_fg_status stopped{}; stopped.struct_size = sizeof(stopped);
    rsf_native_latency_status latency{}; latency.struct_size = sizeof(latency); latency.version = 1;
    const auto stopped_ok = rsf_native_fg_status_get(&stopped);
    const auto latency_ok = rsf_native_latency_status_get(&latency);
    std::printf("Final FG: query=%d requested=%u effective=%u tagged=%llu expected_tags=%llu statistics_flags=0x%x present_callbacks=%llu generated_callbacks=%llu\n",
        stopped_ok, stopped.requested.mode, stopped.vendor.effective_mode,
        static_cast<unsigned long long>(stopped.tagged_frames), static_cast<unsigned long long>(after_invalid.tagged_frames),
        stopped.vendor.valid_statistics, static_cast<unsigned long long>(stopped.vendor.present_callbacks),
        static_cast<unsigned long long>(stopped.vendor.generated_callbacks));
    std::printf("Final latency: query=%d version=%u owner=%u available=%u paced=%llu markers=%llu effective=%u PCL_available=%u report_available=%u\n",
        latency_ok, latency.version, latency.owner, latency.available, static_cast<unsigned long long>(latency.paced_frames),
        static_cast<unsigned long long>(latency.marker_calls), latency.effective_mode, latency.pcl_available, latency.report_available);
    verify(stopped_ok != 0, "final FG status query");
    verify(stopped.requested.mode == RSF_FG_OFF, "final FG request must be Off");
    verify(stopped.tagged_frames == after_invalid.tagged_frames, "FG Off must not increment tag count");
    verify(latency_ok != 0, "final latency status query with version 1");
    verify(!latency.paced_frames, "final latency paced count must be zero");
    verify(!latency.marker_calls, "final latency marker count must be zero");
    verify(latency.effective_mode == RSF_REFLEX_OFF, "final effective latency must be Off");
    verify(!latency.pcl_available, "rendered-source latency PCL availability must remain zero");
    verify(!latency.report_available, "rendered-source physical latency report availability must remain zero");
    if (diagnostics) for (UINT64 i = 0; i < diagnostics->GetNumStoredMessagesAllowedByRetrievalFilter(); ++i) {
        SIZE_T size{}; (void)diagnostics->GetMessage(i, nullptr, &size); std::vector<unsigned char> storage(size);
        auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
        if (SUCCEEDED(diagnostics->GetMessage(i, message, &size)) && message->Severity <= D3D11_MESSAGE_SEVERITY_ERROR) {
            std::printf("D3D11 error: %s\n", message->pDescription); failed = true;
        }
    }
    (void)rsf_d3d11_host_set_active(host.user, 0);
    DestroyWindow(window);
    std::printf("Receipts: sources=%llu bootstrap_sources=%llu test_sources=%llu captured=%llu SR_evaluated=%llu application_Present_calls=%llu SR_Off_generated=%u occluded=%u\n",
        static_cast<unsigned long long>(source), static_cast<unsigned long long>(bootstrap_sources),
        static_cast<unsigned long long>(source - bootstrap_sources), static_cast<unsigned long long>(captured),
        static_cast<unsigned long long>(sr_evaluated), static_cast<unsigned long long>(app_presents),
        off_sr_generated ? 1u : 0u, occluded ? 1u : 0u);
    if (failed) return fail("one or more labeled fixture checks failed");
    if (!late_generated || !off_sr_generated || unavailable) {
        std::puts("INCOMPLETE: missing late-enable or SR-Off callback receipt; no physical display/generation success claimed");
        return 77;
    }
    if (after_reset.tagged_frames <= off_sr.tagged_frames || after_reset.vendor.generated_callbacks <= off_sr.vendor.generated_callbacks) { std::puts("FAIL: SR-On phase never tagged generation inputs"); return fail("SR-On phase must advance both tags and generated callbacks"); }
    if (!sr_evaluated) { std::puts("FAIL: SR-On phase never evaluated"); return fail("SR-On phase must evaluate reconstruction"); }
    std::puts("Rendered-source FSR generated callback regression passed; physical scanout and latency remain unmeasured");
    return 0;
}
