/* SPDX-License-Identifier: GPL-3.0-only */
/* Can a D3D11 texture be opened by a D3D12 device here?
 *
 * The whole presentation bridge rests on yes. Every frame generation SDK this project targets is
 * D3D12 only and the game is D3D11, so the frame has to cross devices, and it crosses through a
 * shared NT handle or it does not cross.
 *
 * On Windows the answer is yes and this is a regression test. Under Wine it is a question: DXVK and
 * vkd3d-proton are separate translations of separate APIs onto Vulkan, with no obligation to agree
 * about memory. So a refusal here exits 77 with the HRESULT printed rather than failing, because it
 * is a fact about the environment rather than a defect in the code. What must never happen is this
 * quietly passing while the bridge cannot work, so every step says what it got.
 */
#include <rescaleframe/shared_surface.h>

#include <windows.h>

#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi1_4.h>

#include <cstdio>

namespace {

bool passed = true;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        passed = false;
    }
}

void stage(const char* what)
{
    std::fprintf(stderr, "[stage] %s\n", what);
    std::fflush(stderr);
}

void collect(void* user, const char* message)
{
    (void)user;
    std::fprintf(stderr, "  %s\n", message);
    std::fflush(stderr);
}

} // namespace

int main()
{
    stage("creating a D3D11 device");
    ID3D11Device* device11 = nullptr;
    ID3D11DeviceContext* context = nullptr;
    const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL obtained{};
    HRESULT made = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, wanted, 2,
                                     D3D11_SDK_VERSION, &device11, &obtained, &context);
    if (FAILED(made)) {
        made = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted, 2,
                                 D3D11_SDK_VERSION, &device11, &obtained, &context);
    }
    if (FAILED(made) || !device11) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 77;
    }

    stage("creating a D3D12 device on the same adapter");
    // The same adapter, found by LUID rather than by taking the first one. Sharing across adapters
    // is not something D3D12 will do, and a machine with two of them is the common case now.
    IDXGIDevice* dxgi_device = nullptr;
    IDXGIAdapter* adapter = nullptr;
    if (SUCCEEDED(device11->QueryInterface(__uuidof(IDXGIDevice),
                                           reinterpret_cast<void**>(&dxgi_device))) &&
        dxgi_device) {
        dxgi_device->GetAdapter(&adapter);
        dxgi_device->Release();
    }

    ID3D12Device* device12 = nullptr;
    made = D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, __uuidof(ID3D12Device),
                             reinterpret_cast<void**>(&device12));
    if (adapter) {
        adapter->Release();
    }
    if (FAILED(made) || !device12) {
        std::fprintf(stderr,
                     "no D3D12 device on this adapter (hr 0x%08lx), skipping: the bridge cannot be "
                     "measured here\n",
                     (unsigned long)made);
        if (context) {
            context->Release();
        }
        device11->Release();
        return 77;
    }

    stage("the D3D11 side alone");
    {
        // Without a D3D12 device: does this runtime produce an NT handle at all? Separating the two
        // means a failure names which half refused, which is the difference between "DXVK cannot
        // export" and "vkd3d cannot import".
        rsf_shared_surface_setup setup{};
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
        setup.width = 64;
        setup.height = 32;
        setup.format = DXGI_FORMAT_R8G8B8A8_UNORM;
        setup.render_target = 1;
        setup.log = collect;

        rsf_shared_surface* surface = nullptr;
        const rsf_shared_result result = rsf_shared_surface_create(device11, nullptr, &setup,
                                                                   &surface);
        if (result != RSF_SHARED_OK) {
            std::fprintf(stderr,
                         "this runtime does not export shared NT handles (result %d), skipping\n",
                         (int)result);
            device12->Release();
            if (context) {
                context->Release();
            }
            device11->Release();
            return 77;
        }
        check(rsf_shared_surface_d3d11(surface) != nullptr, "The texture must exist.");
        check(rsf_shared_surface_target(surface) != nullptr,
              "And a render target view when one was asked for: the layer and the HUD-less copy "
              "are both written on the D3D11 side before anything reads them on the other.");
        check(rsf_shared_surface_d3d12(surface) == nullptr,
              "With no D3D12 device there must be no opened resource, rather than a null nobody "
              "checked.");
        rsf_shared_surface_destroy(surface);
    }

    stage("across both devices");
    {
        rsf_shared_surface_setup setup{};
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
        setup.width = 64;
        setup.height = 32;
        setup.format = DXGI_FORMAT_R8G8B8A8_UNORM;
        setup.render_target = 1;
        setup.log = collect;

        rsf_shared_surface* surface = nullptr;
        const rsf_shared_result result =
            rsf_shared_surface_create(device11, device12, &setup, &surface);
        if (result == RSF_SHARED_ERROR_OPEN_FAILED) {
            std::fprintf(stderr,
                         "the D3D11 runtime exports handles and the D3D12 runtime will not open "
                         "them. That is the measurement this fixture exists to take, and it means "
                         "the bridge cannot run here. Skipping.\n");
            device12->Release();
            if (context) {
                context->Release();
            }
            device11->Release();
            return 77;
        }
        check(result == RSF_SHARED_OK, "Sharing must succeed once both runtimes agree.");
        if (result == RSF_SHARED_OK) {
            check(rsf_shared_surface_d3d12(surface) != nullptr,
                  "The opened resource must exist: this is the frame crossing devices, which is "
                  "the whole bridge.");
            auto* opened = static_cast<ID3D12Resource*>(rsf_shared_surface_d3d12(surface));
            const D3D12_RESOURCE_DESC description = opened->GetDesc();
            check(description.Width == 64 && description.Height == 32,
                  "And must describe the same surface on both sides, or the two devices are "
                  "looking at the same memory with different ideas of its shape.");
            check(description.Format == DXGI_FORMAT_R8G8B8A8_UNORM, "Including its format.");
        }
        rsf_shared_surface_destroy(surface);
    }

    stage("a fence both devices can see");
    {
        rsf_shared_fence* fence = nullptr;
        const rsf_shared_result result =
            rsf_shared_fence_create(device11, device12, collect, nullptr, &fence);
        if (result != RSF_SHARED_OK) {
            std::fprintf(stderr,
                         "a shared fence could not be made (result %d). Surfaces without a fence "
                         "are memory two devices race over, so the bridge cannot run here. "
                         "Skipping.\n",
                         (int)result);
            device12->Release();
            if (context) {
                context->Release();
            }
            device11->Release();
            return 77;
        }
        check(rsf_shared_fence_d3d11(fence) != nullptr && rsf_shared_fence_d3d12(fence) != nullptr,
              "Both views of the fence must exist.");

        // The round trip that the per-present protocol depends on: D3D11 signals, D3D12 sees it.
        // Without this the surfaces are shared memory with no ordering, which is worse than no
        // sharing because it fails intermittently rather than at once.
        auto* fence12 = static_cast<ID3D12Fence*>(rsf_shared_fence_d3d12(fence));
        auto* fence11 = static_cast<ID3D11Fence*>(rsf_shared_fence_d3d11(fence));
        ID3D11DeviceContext4* context4 = nullptr;
        if (context && SUCCEEDED(context->QueryInterface(__uuidof(ID3D11DeviceContext4),
                                                         reinterpret_cast<void**>(&context4))) &&
            context4) {
            check(SUCCEEDED(context4->Signal(fence11, 7u)),
                  "The D3D11 side must be able to signal.");
            context->Flush();
            // Waited for rather than asserted immediately: a signal is a queue operation and
            // reading the value straight away races the queue rather than testing it.
            for (int attempt = 0; attempt < 100 && fence12->GetCompletedValue() < 7u; ++attempt) {
                Sleep(10);
            }
            check(fence12->GetCompletedValue() >= 7u,
                  "And the D3D12 side must see it. This is the ordering every frame of the bridge "
                  "depends on; without it the two devices share memory and race over it.");
            context4->Release();
        } else {
            std::fprintf(stderr, "  no ID3D11DeviceContext4, so the signal round trip is untested\n");
        }
        rsf_shared_fence_destroy(fence);
    }

    stage("arguments are checked");
    {
        rsf_shared_surface_setup setup{};
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
        setup.width = 64;
        setup.height = 32;
        setup.format = DXGI_FORMAT_R8G8B8A8_TYPELESS;
        rsf_shared_surface* surface = nullptr;
        check(rsf_shared_surface_create(device11, device12, &setup, &surface) ==
                  RSF_SHARED_ERROR_INVALID_ARGUMENT,
              "A typeless format must be refused: the other runtime cannot ask what was meant, and "
              "guessing turns a colour buffer into a depth buffer without saying so.");
        setup.format = DXGI_FORMAT_R8G8B8A8_UNORM;
        setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION + 1u;
        check(rsf_shared_surface_create(device11, device12, &setup, &surface) ==
                  RSF_SHARED_ERROR_ABI_MISMATCH,
              "An ABI mismatch must be refused.");
        setup.abi_version = RSF_SHARED_SURFACE_ABI_VERSION;
        setup.width = 0;
        check(rsf_shared_surface_create(device11, device12, &setup, &surface) ==
                  RSF_SHARED_ERROR_INVALID_ARGUMENT,
              "A surface with no extent must be refused.");
        check(rsf_shared_surface_create(nullptr, device12, &setup, &surface) ==
                  RSF_SHARED_ERROR_INVALID_ARGUMENT,
              "A null device must be refused.");
        rsf_shared_surface_destroy(nullptr);
        rsf_shared_fence_destroy(nullptr);
    }

    device12->Release();
    if (context) {
        context->Release();
    }
    device11->Release();

    std::fprintf(stderr, "%s\n",
                 passed ? "shared_surface: all checks passed" : "shared_surface: FAILED");
    return passed ? 0 : 1;
}
