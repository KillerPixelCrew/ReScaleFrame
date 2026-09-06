// Encode motion the way Unreal does, decode it on the GPU, and check the numbers that come back.
//
// This is the pass every backend needs, so its arithmetic has to be right rather than plausible.
// The values here are computed from the engine's own constants and compared against what the
// shader produced, including the two cases that are easy to get wrong: the clear value, which must
// come back as the sentinel rather than as a large negative motion, and a decoded zero, which must
// stay a real zero motion and not be mistaken for the clear value.
//
// Under Wine this runs on DXVK, which is the same D3D11 the game sees on this machine.

#include <rescaleframe/motion_decode.h>

#include <windows.h>

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
        std::fprintf(stderr, "%s (got %.5f, expected %.5f)\n", message, double(actual),
                     double(expected));
        passed = false;
    }
}

void collect(void* user, const char* message)
{
    (void)user;
    std::fprintf(stderr, "[decode] %s\n", message);
    std::fflush(stderr);
}

// The engine's constants, written out rather than referenced, so a change to one of them shows up
// here as a disagreement instead of moving both sides together.
constexpr float encode_scale = 0.499f * 0.5f;
constexpr float encode_bias = 32767.0f / 65535.0f;

uint16_t encode(float velocity)
{
    return static_cast<uint16_t>((velocity * encode_scale + encode_bias) * 65535.0f + 0.5f);
}

float half_to_float(uint16_t value)
{
    const uint32_t sign = (value & 0x8000u) << 16;
    uint32_t exponent = (value >> 10) & 0x1fu;
    uint32_t mantissa = value & 0x3ffu;
    if (exponent == 0) {
        if (mantissa != 0) {
            exponent = 1;
            while ((mantissa & 0x400u) == 0) {
                mantissa <<= 1;
                --exponent;
            }
            mantissa &= 0x3ffu;
            exponent += 112;
        }
    } else if (exponent == 31) {
        exponent = 255;
    } else {
        exponent += 112;
    }
    const uint32_t bits = sign | (exponent << 23) | (mantissa << 13);
    float result;
    std::memcpy(&result, &bits, sizeof(result));
    return result;
}

} // namespace

int main()
{
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    const D3D_FEATURE_LEVEL wanted[] = {D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL obtained{};
    HRESULT created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, wanted, 1,
                                        D3D11_SDK_VERSION, &device, &obtained, &context);
    if (FAILED(created)) {
        created = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, wanted, 1,
                                    D3D11_SDK_VERSION, &device, &obtained, &context);
    }
    if (FAILED(created) || !device || !context) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }

    // Four pixels: the clear value, zero motion, and motion each way.
    const UINT width = 4, height = 1;
    const float wanted_x[4] = {0.0f, 0.0f, 0.5f, -0.5f};
    const float wanted_y[4] = {0.0f, 0.0f, 0.25f, -1.0f};
    std::vector<uint16_t> pixels(width * height * 2);
    pixels[0] = 0;  // the clear value: nothing wrote this pixel
    pixels[1] = 0;
    for (int index = 1; index < 4; ++index) {
        pixels[index * 2 + 0] = encode(wanted_x[index]);
        pixels[index * 2 + 1] = encode(wanted_y[index]);
    }

    D3D11_TEXTURE2D_DESC source{};
    source.Width = width;
    source.Height = height;
    source.MipLevels = 1;
    source.ArraySize = 1;
    source.Format = DXGI_FORMAT_R16G16_UNORM;
    source.SampleDesc.Count = 1;
    source.Usage = D3D11_USAGE_DEFAULT;
    source.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA initial{};
    initial.pSysMem = pixels.data();
    initial.SysMemPitch = width * 4;
    ID3D11Texture2D* motion = nullptr;
    check(SUCCEEDED(device->CreateTexture2D(&source, &initial, &motion)) && motion,
          "The encoded source texture must be created.");
    if (!motion) {
        return 1;
    }

    rsf_motion_decode_setup setup{};
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_MOTION_DECODE_ABI_VERSION;
    setup.width = width;
    setup.height = height;
    setup.log = collect;

    rsf_motion_decode* pass = nullptr;
    setup.abi_version = RSF_MOTION_DECODE_ABI_VERSION + 1u;
    check(rsf_motion_decode_create(device, &setup, &pass) == RSF_MOTION_DECODE_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    setup.abi_version = RSF_MOTION_DECODE_ABI_VERSION;

    setup.width = 0;
    check(rsf_motion_decode_create(device, &setup, &pass) ==
              RSF_MOTION_DECODE_ERROR_INVALID_ARGUMENT,
          "A zero sized pass must be rejected.");
    setup.width = width;

    const rsf_motion_decode_result made = rsf_motion_decode_create(device, &setup, &pass);
    if (made == RSF_MOTION_DECODE_ERROR_SHADER_FAILED) {
        // No HLSL compiler here. That is an environment without one, not a broken pass.
        std::fprintf(stderr, "no shader compiler available, skipping the decode\n");
        motion->Release();
        context->Release();
        device->Release();
        return passed ? 0 : 1;
    }
    check(made == RSF_MOTION_DECODE_OK && pass != nullptr, "Creating the pass must succeed.");
    if (!pass) {
        return 1;
    }

    rsf_motion_decode_params params{};
    params.struct_size = sizeof(params);
    params.scale_x = 1.0f / encode_scale;
    params.scale_y = 1.0f / encode_scale;
    params.bias_x = encode_bias;
    params.bias_y = encode_bias;
    params.output_scale_x = 1.0f;
    params.output_scale_y = 1.0f;
    params.invalid_value = -1000.0f;
    params.zero_means_unwritten = 1;

    // A source of the wrong size has to be refused rather than dispatched over.
    D3D11_TEXTURE2D_DESC wrong = source;
    wrong.Width = width * 2;
    std::vector<uint16_t> more(width * 2 * height * 2, 0);
    D3D11_SUBRESOURCE_DATA more_initial{};
    more_initial.pSysMem = more.data();
    more_initial.SysMemPitch = width * 2 * 4;
    ID3D11Texture2D* mismatched = nullptr;
    if (SUCCEEDED(device->CreateTexture2D(&wrong, &more_initial, &mismatched)) && mismatched) {
        check(rsf_motion_decode_run(pass, context, mismatched, &params) ==
                  RSF_MOTION_DECODE_ERROR_SOURCE_MISMATCH,
              "A source of the wrong size must be refused.");
        mismatched->Release();
    }

    check(rsf_motion_decode_run(pass, context, motion, &params) == RSF_MOTION_DECODE_OK,
          "Running the decode must succeed.");

    // Read the decoded target back.
    auto* decoded = static_cast<ID3D11Texture2D*>(rsf_motion_decode_texture(pass));
    check(decoded != nullptr, "The pass must expose its decoded target.");
    D3D11_TEXTURE2D_DESC staging{};
    decoded->GetDesc(&staging);
    check(staging.Format == DXGI_FORMAT_R16G16_FLOAT,
          "The default output format must be the half precision pair backends expect.");
    staging.Usage = D3D11_USAGE_STAGING;
    staging.BindFlags = 0;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    staging.MiscFlags = 0;
    ID3D11Texture2D* readable = nullptr;
    check(SUCCEEDED(device->CreateTexture2D(&staging, nullptr, &readable)) && readable,
          "A staging copy of the decoded target must be created.");
    if (!readable) {
        return 1;
    }
    context->CopyResource(readable, decoded);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    check(SUCCEEDED(context->Map(readable, 0, D3D11_MAP_READ, 0, &mapped)),
          "The staging copy must map.");

    if (mapped.pData) {
        const auto* row = static_cast<const uint16_t*>(mapped.pData);
        const float sentinel_x = half_to_float(row[0]);
        const float sentinel_y = half_to_float(row[1]);
        check_near(sentinel_x, -1000.0f, 1.0f,
                   "The clear value must decode to the sentinel, not to a large motion.");
        check_near(sentinel_y, -1000.0f, 1.0f, "Both channels of the sentinel must be written.");

        // Zero motion, encoded and decoded, has to come back as zero. That it cannot be told apart
        // from the sentinel afterwards is exactly why the sentinel is re-established above.
        check_near(half_to_float(row[2]), 0.0f, 0.01f, "Encoded zero motion must decode to zero.");
        check_near(half_to_float(row[3]), 0.0f, 0.01f, "Encoded zero motion must decode to zero.");

        for (int index = 2; index < 4; ++index) {
            check_near(half_to_float(row[index * 2 + 0]), wanted_x[index], 0.01f,
                       "Decoded x must match what was encoded.");
            check_near(half_to_float(row[index * 2 + 1]), wanted_y[index], 0.01f,
                       "Decoded y must match what was encoded.");
        }
        context->Unmap(readable, 0);
    }

    // The output scale is where a convention flip belongs, so it has to actually apply.
    params.output_scale_y = -1.0f;
    check(rsf_motion_decode_run(pass, context, motion, &params) == RSF_MOTION_DECODE_OK,
          "Running with a flipped axis must succeed.");
    context->CopyResource(readable, decoded);
    if (SUCCEEDED(context->Map(readable, 0, D3D11_MAP_READ, 0, &mapped)) && mapped.pData) {
        const auto* row = static_cast<const uint16_t*>(mapped.pData);
        check_near(half_to_float(row[5]), -wanted_y[2], 0.01f,
                   "A negative output scale must flip that axis.");
        check_near(half_to_float(row[4]), wanted_x[2], 0.01f,
                   "Flipping one axis must leave the other alone.");
        context->Unmap(readable, 0);
    }

    readable->Release();
    rsf_motion_decode_destroy(pass);
    motion->Release();
    context->Release();
    device->Release();
    return passed ? 0 : 1;
}
