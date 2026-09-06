// Install the observer, then behave like a game: create a device, allocate targets, present.
// The observer must notice the matching target, ignore the others, and leave rendering working.

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/texture_dump.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>
#include <cstdint>
#include <string>
#include <vector>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

// This test drives a graphics runtime and patches vtables, so a hang is a realistic failure.
// Announcing each stage means a stall says where it stalled instead of nothing at all.
void stage(const char* what)
{
    std::fprintf(stderr, "[stage] %s\n", what);
    std::fflush(stderr);
}

// The dump runs inside a present, where a failure is a crashed game and nothing else. Its progress
// lines are the only account of how far it got, so the sink is part of what this test covers.
std::vector<std::string> log_lines;

void collect(void* user, const char* message)
{
    (void)user;
    log_lines.emplace_back(message);
}

bool logged(const char* fragment)
{
    for (const std::string& line : log_lines) {
        if (line.find(fragment) != std::string::npos) {
            return true;
        }
    }
    return false;
}

HWND make_window()
{
    WNDCLASSEXW window_class{};
    window_class.cbSize = sizeof(window_class);
    window_class.lpfnWndProc = DefWindowProcW;
    window_class.hInstance = GetModuleHandleW(nullptr);
    window_class.lpszClassName = L"RsfObserverTest";
    RegisterClassExW(&window_class);
    return CreateWindowExW(0, window_class.lpszClassName, L"", WS_OVERLAPPEDWINDOW, 0, 0, 64, 64,
                           nullptr, nullptr, window_class.hInstance, nullptr);
}

} // namespace

int main(int argc, char* argv[])
{
    if (argc != 2) {
        std::fprintf(stderr, "usage: d3d11_observer <output directory>\n");
        return 2;
    }

    stage("validating arguments");
    rsf_observer_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_OBSERVER_ABI_VERSION;
    options.format = DXGI_FORMAT_R16G16_UNORM;
    options.minimum_width = 64;
    options.capacity = 8;
    options.constant_buffer_min_bytes = 2640;  // stock 4.18 view uniform size
    options.constant_buffer_max_bytes = 2640;
    options.log = collect;

    options.abi_version = RSF_OBSERVER_ABI_VERSION + 1u;
    check(rsf_observer_install(&options) == RSF_OBSERVER_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    options.abi_version = RSF_OBSERVER_ABI_VERSION;

    options.struct_size = 0;
    check(rsf_observer_install(&options) == RSF_OBSERVER_ERROR_INVALID_ARGUMENT,
          "A short options structure must be rejected.");
    options.struct_size = sizeof(options);

    stage("installing observer (creates a dummy device and swap chain)");
    const rsf_observer_result installed = rsf_observer_install(&options);
    if (installed == RSF_OBSERVER_ERROR_NO_DEVICE) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }
    check(installed == RSF_OBSERVER_OK, "Installing the observer must succeed.");
    check(rsf_observer_install(&options) == RSF_OBSERVER_ERROR_ALREADY_INSTALLED,
          "Installing twice must be refused.");

    // From here on, act like the game: a fresh device the observer never saw created.
    stage("creating test window");
    HWND window = make_window();
    check(window != nullptr, "The test window must be created.");

    DXGI_SWAP_CHAIN_DESC swap{};
    swap.BufferCount = 1;
    swap.BufferDesc.Width = 256;
    swap.BufferDesc.Height = 128;
    swap.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    swap.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    swap.OutputWindow = window;
    swap.SampleDesc.Count = 1;
    swap.Windowed = TRUE;

    stage("creating test device and swap chain");
    IDXGISwapChain* swapchain = nullptr;
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL obtained{};
    HRESULT created = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0,
                                                    wanted, 1, D3D11_SDK_VERSION, &swap,
                                                    &swapchain, &device, &obtained, &context);
    if (FAILED(created)) {
        created = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted,
                                                1, D3D11_SDK_VERSION, &swap, &swapchain, &device,
                                                &obtained, &context);
    }
    check(SUCCEEDED(created) && device && swapchain, "The test device must be created.");
    if (FAILED(created) || !device) {
        return 1;
    }

    auto make_target = [&](DXGI_FORMAT format, UINT width, UINT height) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = width;
        desc.Height = height;
        desc.MipLevels = 1;
        desc.ArraySize = 1;
        desc.Format = format;
        desc.SampleDesc.Count = 1;
        desc.Usage = D3D11_USAGE_DEFAULT;
        desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
        ID3D11Texture2D* texture = nullptr;
        device->CreateTexture2D(&desc, nullptr, &texture);
        return texture;
    };

    // One match, and three that must be ignored for three different reasons.
    stage("creating targets");
    ID3D11Texture2D* match = make_target(DXGI_FORMAT_R16G16_UNORM, 256, 128);
    ID3D11Texture2D* wrong_format = make_target(DXGI_FORMAT_R8G8B8A8_UNORM, 256, 128);
    ID3D11Texture2D* too_small = make_target(DXGI_FORMAT_R16G16_UNORM, 32, 16);
    check(match && wrong_format && too_small, "The test targets must be created.");

    D3D11_TEXTURE2D_DESC staging{};
    staging.Width = 256;
    staging.Height = 128;
    staging.MipLevels = 1;
    staging.ArraySize = 1;
    staging.Format = DXGI_FORMAT_R16G16_UNORM;
    staging.SampleDesc.Count = 1;
    staging.Usage = D3D11_USAGE_STAGING;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ID3D11Texture2D* not_a_target = nullptr;
    device->CreateTexture2D(&staging, nullptr, &not_a_target);

    // A constant buffer of the size the view uniform data occupies, with a recognisable payload,
    // plus one of a different size that must be ignored.
    stage("creating constant buffers");
    std::vector<float> constants(2640 / sizeof(float), 0.0f);
    constants[0] = 1.5f;
    constants[1] = -2.25f;
    constants[(2640 / sizeof(float)) - 1] = 9.75f;

    auto make_constant_buffer = [&](UINT bytes, const void* payload) {
        D3D11_BUFFER_DESC desc{};
        desc.ByteWidth = bytes;
        desc.Usage = D3D11_USAGE_DEFAULT;
        desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        D3D11_SUBRESOURCE_DATA initial{};
        initial.pSysMem = payload;
        ID3D11Buffer* buffer = nullptr;
        device->CreateBuffer(&desc, payload ? &initial : nullptr, &buffer);
        return buffer;
    };

    std::vector<float> decoy(512 / sizeof(float), 3.0f);
    ID3D11Buffer* wrong_size = make_constant_buffer(512, decoy.data());
    ID3D11Buffer* view_like = make_constant_buffer(2640, constants.data());
    check(view_like != nullptr, "The view sized constant buffer must be created.");

    stage("presenting");
    check(SUCCEEDED(swapchain->Present(0, 0)), "Presenting must still work with hooks installed.");
    check(SUCCEEDED(swapchain->Present(0, 0)), "Presenting must keep working.");

    stage("reading status");
    rsf_observer_status status{};
    status.struct_size = sizeof(status);
    check(rsf_observer_get_status(&status) == RSF_OBSERVER_OK, "Status must be readable.");
    check(status.installed == 1u, "Status must report the observer as installed.");
    check(status.frames_presented >= 2u, "Both presents must be counted.");
    check(status.have_device == 1u, "A device must have been acquired.");
    check(status.textures_created >= 4u, "Every created texture must be counted.");
    // The filter is the point: one of the four qualifies.
    check(status.textures_matched == 1u, "Exactly the matching target must be retained.");
    check(status.present_width == 256u && status.present_height == 128u,
          "The presented size must be recorded from the swap chain.");

    char prefix[1024];
    std::snprintf(prefix, sizeof(prefix), "%s\\observed", argv[1]);
    stage("requesting dump");
    check(rsf_observer_request_dump(prefix, RSF_DUMP_VIEW_VELOCITY) == RSF_OBSERVER_OK,
          "Requesting a dump must succeed.");
    // The request is carried out inside a present, so one more present performs it.
    check(SUCCEEDED(swapchain->Present(0, 0)), "Presenting must carry out the dump.");

    rsf_observer_status after{};
    after.struct_size = sizeof(after);
    check(rsf_observer_get_status(&after) == RSF_OBSERVER_OK, "Status must be readable.");
    check(after.dumps_completed == 1u, "The dump must have run inside the present.");
    check(after.textures_written == 1u, "The one retained target must be written.");
    check(after.constant_bytes_written == 2640u, "The retained constant buffer must be written.");

    // What the loader's log has to contain for a crashed dump to be diagnosable at all: the count
    // it started with, the resource it was on, and the step it had reached.
    check(logged("dump begin: 1 textures"), "The dump must announce what it is about to do.");
    check(logged("texture 1 of 1"), "Each texture must be named before it is touched.");
    check(logged("256x128"), "The descriptor must be logged before the copy.");
    check(logged("constant buffer of 2640 bytes"), "Each constant buffer must be named.");
    check(logged("dump end: 1 textures"), "Completion must be distinguishable from a crash.");

    char constants_path[1024];
    std::snprintf(constants_path, sizeof(constants_path), "%s\\observed_cb2640.bin", argv[1]);
    if (std::FILE* stream = std::fopen(constants_path, "rb")) {
        std::vector<float> read_back(2640 / sizeof(float), 0.0f);
        const size_t got = std::fread(read_back.data(), 1, 2640, stream);
        std::fclose(stream);
        check(got == 2640, "The dumped file must hold the whole buffer.");
        check(read_back[0] == 1.5f && read_back[1] == -2.25f &&
                  read_back[(2640 / sizeof(float)) - 1] == 9.75f,
              "The dumped contents must match what was uploaded.");
    } else {
        check(false, "The constant buffer dump must exist.");
    }

    char sizes_path[1024];
    std::snprintf(sizes_path, sizeof(sizes_path), "%s\\sizes.csv", argv[1]);
    check(rsf_observer_write_buffer_sizes(sizes_path) == RSF_OBSERVER_OK,
          "Writing the size histogram must succeed.");
    check(after.distinct_buffer_sizes >= 2u,
          "Both constant buffer sizes must appear in the histogram.");

    stage("uninstalling");
    check(rsf_observer_uninstall() == RSF_OBSERVER_OK, "Uninstalling must succeed.");
    check(rsf_observer_uninstall() == RSF_OBSERVER_ERROR_NOT_READY,
          "Uninstalling twice must be refused.");
    // Presenting after the vtable is restored proves the entries were put back intact.
    check(SUCCEEDED(swapchain->Present(0, 0)), "Presenting must work after uninstalling.");

    if (view_like) view_like->Release();
    if (wrong_size) wrong_size->Release();
    if (not_a_target) not_a_target->Release();
    if (too_small) too_small->Release();
    if (wrong_format) wrong_format->Release();
    if (match) match->Release();
    if (context) context->Release();
    swapchain->Release();
    device->Release();
    DestroyWindow(window);
    return passed ? 0 : 1;
}
