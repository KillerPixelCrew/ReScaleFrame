// Install the observer, then behave like a game: create a device, allocate targets, present.
// The observer must notice the matching target, ignore the others, and leave rendering working.

#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/texture_dump.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdio>
#include <cstdint>
#include <cstring>
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

// What the creation callbacks saw. The observer reports pipeline objects as the game builds them,
// which is what turns a pointer into a name for the rest of the run; these record that the
// translation out of D3D11's descriptors is faithful, since everything downstream trusts it.
struct SeenLayout {
    void* layout;
    uint32_t copied;
    uint32_t count;
    rsf_observer_layout_element elements[RSF_OBSERVER_MAX_LAYOUT_ELEMENTS];
};
std::vector<SeenLayout> seen_layouts;

struct SeenShader {
    void* shader;
    uint32_t stage;
    uint32_t bytes;
    bool had_bytecode;
};
std::vector<SeenShader> seen_shaders;

struct SeenTexture {
    void* texture;
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t mip_levels;
    uint32_t array_size;
    uint32_t sample_count;
    uint32_t bind_flags;
};
std::vector<SeenTexture> seen_textures;

void on_layout(void* user, void* layout, const rsf_observer_layout_element* elements,
               uint32_t copied, uint32_t count)
{
    (void)user;
    SeenLayout record{};
    record.layout = layout;
    record.copied = copied;
    record.count = count;
    for (uint32_t index = 0; index < copied && index < RSF_OBSERVER_MAX_LAYOUT_ELEMENTS; ++index) {
        record.elements[index] = elements[index];
    }
    seen_layouts.push_back(record);
}

void on_shader(void* user, void* shader, uint32_t stage, const void* bytecode, uint32_t bytes)
{
    (void)user;
    seen_shaders.push_back({shader, stage, bytes, bytecode != nullptr});
}

void on_texture(void* user, void* texture, uint32_t width, uint32_t height, uint32_t format,
                uint32_t mip_levels, uint32_t array_size, uint32_t sample_count,
                uint32_t bind_flags, uint32_t misc_flags)
{
    (void)user;
    (void)misc_flags;
    seen_textures.push_back(
        {texture, width, height, format, mip_levels, array_size, sample_count, bind_flags});
}

using compile_fn = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

// Loaded rather than linked, as elsewhere in this tree, so nothing here needs the compiler import
// library to build.
compile_fn load_compiler()
{
    const HMODULE module =
        LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        return nullptr;
    }
    return reinterpret_cast<compile_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
}

// A vertex shader whose input signature is Slate's, so a real input layout can be created against
// it. D3D11 validates a layout against a shader signature, which is why this has to compile rather
// than being a made up blob. The semantics do not matter; the declaration does.
const char* const layout_probe_source =
    "struct VSIn {\n"
    "  float4 texcoords : ATTRIBUTE0;\n"
    "  float2 material  : ATTRIBUTE1;\n"
    "  float2 position  : ATTRIBUTE2;\n"
    "  float4 color     : ATTRIBUTE3;\n"
    "  uint2  pixelsize : ATTRIBUTE4;\n"
    "};\n"
    "float4 VSMain(VSIn input) : SV_Position {\n"
    "  return float4(input.position, 0, 1) + input.texcoords + float4(input.color.rgb, 0)\n"
    "       + float4(input.material, 0, 0) + float4(input.pixelsize.x, input.pixelsize.y, 0, 0);\n"
    "}\n"
    "float4 PSMain() : SV_Target { return float4(1, 1, 1, 1); }\n";

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
    // Unreal's encoding, so the decode written beside each dump is the one a backend would get.
    options.decode_motion = 1;
    options.motion_scale = 1.0f / (0.499f * 0.5f);
    options.motion_bias = 32767.0f / 65535.0f;
    options.motion_invalid_value = -1000.0f;
    options.on_layout = on_layout;
    options.on_shader = on_shader;
    options.on_texture = on_texture;

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

    stage("creation callbacks");
    {
        // Every texture, not only the ones the dump filter keeps. The filter serves dumping; what a
        // texture is for is a question the caller answers, and it cannot answer it about a texture
        // it was never told about. `too_small` and `wrong_format` are both filtered out above and
        // both must still have been reported.
        auto find_texture = [](void* texture) -> const SeenTexture* {
            for (const SeenTexture& seen : seen_textures) {
                if (seen.texture == texture) {
                    return &seen;
                }
            }
            return nullptr;
        };
        const SeenTexture* reported = find_texture(match);
        check(reported != nullptr, "A created texture must be reported.");
        check(find_texture(wrong_format) != nullptr,
              "A texture the dump filter rejects on format must still be reported.");
        check(find_texture(too_small) != nullptr,
              "A texture the dump filter rejects on size must still be reported.");
        if (reported) {
            check(reported->width == 256 && reported->height == 128,
                  "The reported size must be the descriptor's.");
            check(reported->format == static_cast<uint32_t>(DXGI_FORMAT_R16G16_UNORM),
                  "The reported format must be the descriptor's.");
            check(reported->mip_levels == 1 && reported->array_size == 1 &&
                      reported->sample_count == 1,
                  "Mip, array and sample counts must survive the translation: all three are part "
                  "of what identifies a widget target.");
            check((reported->bind_flags & RSF_OBSERVER_BIND_RENDER_TARGET) != 0 &&
                      (reported->bind_flags & RSF_OBSERVER_BIND_SHADER_RESOURCE) != 0,
                  "The bind flags this header names must be the D3D11 values.");
        }

        const compile_fn compile = load_compiler();
        if (!compile) {
            std::fprintf(stderr, "d3dcompiler_47.dll is not available, skipping the layout and "
                                 "shader callbacks\n");
        } else {
            ID3DBlob* vertex_code = nullptr;
            ID3DBlob* pixel_code = nullptr;
            ID3DBlob* errors = nullptr;
            HRESULT compiled =
                compile(layout_probe_source, std::strlen(layout_probe_source), "observer_test",
                        nullptr, nullptr, "VSMain", "vs_5_0", 0, 0, &vertex_code, &errors);
            if (errors) {
                errors->Release();
                errors = nullptr;
            }
            check(SUCCEEDED(compiled) && vertex_code, "The probe vertex shader must compile.");
            compiled = compile(layout_probe_source, std::strlen(layout_probe_source),
                               "observer_test", nullptr, nullptr, "PSMain", "ps_5_0", 0, 0,
                               &pixel_code, &errors);
            if (errors) {
                errors->Release();
            }
            check(SUCCEEDED(compiled) && pixel_code, "The probe pixel shader must compile.");

            if (vertex_code && pixel_code) {
                // Slate's declaration, from 4.18.3. The point is not that the observer knows what
                // Slate is, which it must not, but that the five fields that identify a
                // declaration survive the trip out of D3D11 intact.
                const D3D11_INPUT_ELEMENT_DESC slate[] = {
                    {"ATTRIBUTE", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0,
                     D3D11_INPUT_PER_VERTEX_DATA, 0},
                    {"ATTRIBUTE", 1, DXGI_FORMAT_R32G32_FLOAT, 0, 16, D3D11_INPUT_PER_VERTEX_DATA,
                     0},
                    {"ATTRIBUTE", 2, DXGI_FORMAT_R32G32_FLOAT, 0, 24, D3D11_INPUT_PER_VERTEX_DATA,
                     0},
                    {"ATTRIBUTE", 3, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 32,
                     D3D11_INPUT_PER_VERTEX_DATA, 0},
                    {"ATTRIBUTE", 4, DXGI_FORMAT_R16G16_UINT, 0, 36, D3D11_INPUT_PER_VERTEX_DATA,
                     0},
                };
                ID3D11InputLayout* layout = nullptr;
                const HRESULT made = device->CreateInputLayout(
                    slate, 5, vertex_code->GetBufferPointer(), vertex_code->GetBufferSize(),
                    &layout);
                check(SUCCEEDED(made) && layout, "The probe input layout must be created.");
                if (layout) {
                    bool found = false;
                    for (const SeenLayout& seen : seen_layouts) {
                        if (seen.layout != layout) {
                            continue;
                        }
                        found = true;
                        check(seen.count == 5 && seen.copied == 5,
                              "All five elements must be reported.");
                        check(seen.elements[4].semantic_index == 4 &&
                                  seen.elements[4].format ==
                                      static_cast<uint32_t>(DXGI_FORMAT_R16G16_UINT) &&
                                  seen.elements[4].input_slot == 0 &&
                                  seen.elements[4].byte_offset == 36 &&
                                  seen.elements[4].per_instance == 0,
                              "The last element must arrive with its index, format, slot, offset "
                              "and step class intact.");
                        check(seen.elements[0].byte_offset == 0 && seen.elements[1].byte_offset == 16,
                              "Offsets must not be renumbered.");
                    }
                    check(found, "Creating an input layout must reach the callback.");
                    layout->Release();
                }

                ID3D11PixelShader* pixel = nullptr;
                device->CreatePixelShader(pixel_code->GetBufferPointer(),
                                          pixel_code->GetBufferSize(), nullptr, &pixel);
                check(pixel != nullptr, "The probe pixel shader must be created.");
                if (pixel) {
                    bool found = false;
                    for (const SeenShader& seen : seen_shaders) {
                        if (seen.shader != pixel) {
                            continue;
                        }
                        found = true;
                        check(seen.stage == RSF_OBSERVER_STAGE_PIXEL,
                              "A pixel shader must be reported as one.");
                        check(seen.had_bytecode && seen.bytes == pixel_code->GetBufferSize(),
                              "The bytecode must arrive whole, because hashing it is the caller's "
                              "job and a truncated blob hashes to a plausible wrong answer.");
                    }
                    check(found, "Creating a pixel shader must reach the callback.");
                }

                // Created here rather than assumed to happen during device setup: whether a
                // runtime builds shaders of its own is the runtime's business, and an assertion
                // about it tests the driver instead of this code.
                //
                // The pixel shader above is deliberately still alive. Releasing it first made this
                // run fail, because the runtime handed the vertex shader the address the pixel
                // shader had just vacated and the lookup below found the stale record. That is not
                // a quirk of the test: it is the reason nothing downstream may key a registry on a
                // pointer without evicting on reuse, and it happened within a few lines of one
                // another rather than over a long session.
                ID3D11VertexShader* vertex = nullptr;
                device->CreateVertexShader(vertex_code->GetBufferPointer(),
                                           vertex_code->GetBufferSize(), nullptr, &vertex);
                check(vertex != nullptr, "The probe vertex shader must be created.");
                if (vertex) {
                    bool found = false;
                    for (const SeenShader& seen : seen_shaders) {
                        if (seen.shader != vertex) {
                            continue;
                        }
                        found = true;
                        check(seen.stage == RSF_OBSERVER_STAGE_VERTEX,
                              "A vertex shader must be reported as one: the two stages share a "
                              "callback and only this field separates them.");
                        check(seen.bytes == vertex_code->GetBufferSize(),
                              "The vertex bytecode must arrive whole.");
                    }
                    check(found, "Creating a vertex shader must reach the callback.");
                    vertex->Release();
                }
                if (pixel) {
                    pixel->Release();
                }
            }
            if (vertex_code) {
                vertex_code->Release();
            }
            if (pixel_code) {
                pixel_code->Release();
            }
        }
    }

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

    // The decode has to reach the game's own targets, not only a test's made up values, so the
    // dump path runs it and writes the result beside the raw one.
    char decoded_path[1024];
    std::snprintf(decoded_path, sizeof(decoded_path), "%s\\observed_0_decoded.tga", argv[1]);
    if (std::FILE* stream = std::fopen(decoded_path, "rb")) {
        std::fclose(stream);
    } else if (logged("motion decode unavailable")) {
        std::fprintf(stderr, "no shader compiler here, decode skipped\n");
    } else {
        check(false, "A decoded motion dump must be written beside the raw one.");
    }

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
