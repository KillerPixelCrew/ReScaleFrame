// Dump a texture whose contents we chose, and check the decode against values computed here.
// Under Wine this runs on DXVK, which is the same D3D11 the game sees on this machine.

#include <rescaleframe/texture_dump.h>

#include <d3d11.h>

#include <cstdint>
#include <cstdio>
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

void check_near(float actual, float expected, float tolerance, const char* message)
{
    const float difference = actual > expected ? actual - expected : expected - actual;
    if (difference > tolerance) {
        std::fprintf(stderr, "%s (got %.4f, expected %.4f)\n", message, double(actual),
                     double(expected));
        passed = false;
    }
}

// Collects the progress lines. In the game these go to the loader's log, where they exist to name
// the resource and the step a dump died on, so a test that never reads them would not notice the
// sink going quiet.
std::vector<std::string> log_lines;

void collect(void* user, const char* message)
{
    check(user == &log_lines, "The sink must be handed back the pointer it was given.");
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

// The encoding under test, written out the way Common.ush states it.
uint16_t encode_velocity(float value)
{
    const float encoded = value * (0.499f * 0.5f) + 32767.0f / 65535.0f;
    return static_cast<uint16_t>(encoded * 65535.0f + 0.5f);
}

} // namespace

int main(int argc, char* argv[])
{
    if (argc != 2) {
        std::fprintf(stderr, "usage: texture_dump <output directory>\n");
        return 2;
    }

    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL obtained{};
    HRESULT created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, wanted,
                                        1, D3D11_SDK_VERSION, &device, &obtained, &context);
    if (FAILED(created)) {
        created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted, 1,
                                    D3D11_SDK_VERSION, &device, &obtained, &context);
    }
    if (FAILED(created) || !device || !context) {
        // No device at all means the environment cannot host this test, which is not a failure
        // of the code under test.
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }

    // Three known pixels: no motion, positive motion, negative motion, plus an unwritten one.
    const UINT width = 4, height = 1;
    std::vector<uint16_t> pixels(width * height * 2);
    pixels[0] = 0;                       // unwritten sentinel
    pixels[1] = 0;
    pixels[2] = encode_velocity(0.0f);   // written, no motion
    pixels[3] = encode_velocity(0.0f);
    pixels[4] = encode_velocity(0.5f);   // positive
    pixels[5] = encode_velocity(0.25f);
    pixels[6] = encode_velocity(-0.5f);  // negative
    pixels[7] = encode_velocity(-1.0f);

    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R16G16_UNORM;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;

    D3D11_SUBRESOURCE_DATA initial{};
    initial.pSysMem = pixels.data();
    initial.SysMemPitch = width * 4;

    ID3D11Texture2D* texture = nullptr;
    check(SUCCEEDED(device->CreateTexture2D(&desc, &initial, &texture)) && texture,
          "The source texture must be created.");
    if (!texture) {
        return 1;
    }

    char prefix[1024];
    std::snprintf(prefix, sizeof(prefix), "%s\\velocity", argv[1]);

    rsf_texture_dump_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
    options.output_prefix_utf8 = prefix;
    options.view = RSF_DUMP_VIEW_VELOCITY;
    options.log = collect;
    options.log_user = &log_lines;

    rsf_texture_dump_report report{};
    report.struct_size = sizeof(report);

    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION + 1u;
    check(rsf_dump_texture(device, context, texture, &options, &report) ==
              RSF_TEXTURE_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;

    check(rsf_dump_texture(nullptr, context, texture, &options, &report) ==
              RSF_TEXTURE_ERROR_INVALID_ARGUMENT,
          "A missing device must be rejected.");

    const rsf_dump_texture_result result =
        rsf_dump_texture(device, context, texture, &options, &report);
    check(result == RSF_TEXTURE_OK, "Dumping a velocity texture must succeed.");

    if (result == RSF_TEXTURE_OK) {
        check(report.width == width && report.height == height,
              "The report must describe the source dimensions.");
        // One pixel in four was left at the clear value.
        check_near(report.fraction_unwritten, 0.25f, 0.001f,
                   "The unwritten share must match the sentinel pixels.");
        // Decoded extremes must match what was encoded, within 16 bit quantisation.
        check_near(report.min_x, -0.5f, 0.01f, "Smallest decoded x must match what was encoded.");
        check_near(report.max_x, 0.5f, 0.01f, "Largest decoded x must match what was encoded.");
        check_near(report.min_y, -1.0f, 0.01f, "Smallest decoded y must match what was encoded.");
        check_near(report.max_y, 0.25f, 0.01f, "Largest decoded y must match what was encoded.");

        // Each step is announced before it runs, which is the only reason a crashing dump says
        // where it was.
        check(logged("creating staging copy"), "The staging step must be announced.");
        check(logged("mapping"), "The mapping step must be announced.");
        check(logged("writing"), "The write step must be announced.");
        check(logged("done"), "Completion must be announced.");

        char image[1024];
        std::snprintf(image, sizeof(image), "%s.tga", prefix);
        std::FILE* stream = std::fopen(image, "rb");
        check(stream != nullptr, "A Targa file must be written.");
        if (stream) {
            uint8_t header[18];
            check(std::fread(header, 1, sizeof(header), stream) == sizeof(header) &&
                      header[2] == 2 && header[16] == 32,
                  "The Targa header must describe an uncompressed 32 bit image.");
            std::fclose(stream);
        }
    }

    // An unsupported format must be refused rather than decoded as something it is not.
    D3D11_TEXTURE2D_DESC other = desc;
    other.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    std::vector<uint8_t> bytes(width * height * 4, 0);
    D3D11_SUBRESOURCE_DATA otherInitial{};
    otherInitial.pSysMem = bytes.data();
    otherInitial.SysMemPitch = width * 4;
    ID3D11Texture2D* unsupported = nullptr;
    if (SUCCEEDED(device->CreateTexture2D(&other, &otherInitial, &unsupported)) && unsupported) {
        check(rsf_dump_texture(device, context, unsupported, &options, &report) ==
                  RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT,
              "An unsupported format must be refused, not guessed at.");
        unsupported->Release();
    }

    // A mip chain has more subresources than the single-subresource staging copy, which makes
    // CopyResource invalid. The top level has to be named explicitly instead, and a game's targets
    // are not all flat.
    D3D11_TEXTURE2D_DESC mipped{};
    mipped.Width = 8;
    mipped.Height = 8;
    mipped.MipLevels = 4;
    mipped.ArraySize = 1;
    mipped.Format = DXGI_FORMAT_R16G16_UNORM;
    mipped.SampleDesc.Count = 1;
    mipped.Usage = D3D11_USAGE_DEFAULT;
    mipped.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET;
    ID3D11Texture2D* with_mips = nullptr;
    if (SUCCEEDED(device->CreateTexture2D(&mipped, nullptr, &with_mips)) && with_mips) {
        char mip_prefix[1024];
        std::snprintf(mip_prefix, sizeof(mip_prefix), "%s\\mipped", argv[1]);
        options.output_prefix_utf8 = mip_prefix;
        check(rsf_dump_texture(device, context, with_mips, &options, &report) == RSF_TEXTURE_OK,
              "A texture with mip levels must dump its top level rather than fail.");
        check(report.width == 8u && report.height == 8u,
              "The dump must describe the top mip level.");
        with_mips->Release();
        options.output_prefix_utf8 = prefix;
    }

    // Copying between resources of different devices is invalid and takes the process down. It
    // has to be refused before the copy, not diagnosed after it.
    ID3D11Device* other_device = nullptr;
    ID3D11DeviceContext* other_context = nullptr;
    if (SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted, 1,
                                    D3D11_SDK_VERSION, &other_device, &obtained, &other_context)) &&
        other_device) {
        ID3D11Texture2D* foreign = nullptr;
        if (SUCCEEDED(other_device->CreateTexture2D(&desc, &initial, &foreign)) && foreign) {
            check(rsf_dump_texture(device, context, foreign, &options, &report) ==
                      RSF_TEXTURE_ERROR_FOREIGN_DEVICE,
                  "A texture from another device must be refused before anything is copied.");
            foreign->Release();
        }
        if (other_context) {
            other_context->Release();
        }
        other_device->Release();
    }

    texture->Release();
    context->Release();
    device->Release();
    return passed ? 0 : 1;
}
