// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/texture_dump.h>

#include <d3d11.h>

#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

// Unreal 4.18 stores velocity as In * (0.499 * 0.5) + 32767/65535 and reserves a raw zero as the
// clear value meaning nothing wrote velocity at that pixel. See Common.ush.
constexpr float velocity_scale = 0.499f * 0.5f;
constexpr float velocity_bias = 32767.0f / 65535.0f;

struct Sample {
    float x;
    float y;
    // Third channel, for the colour formats. Zero for the two channel motion formats, which is
    // what the velocity views expect and what they had before this existed.
    float z;
    bool written;
};

// One of the small unsigned floats R11G11B10 packs: five exponent bits, `mantissa_bits` of
// mantissa, bias 15, no sign. Written as arithmetic rather than as bit assembly because there is no
// standard type to assemble into.
float small_float_to_float(uint32_t value, uint32_t mantissa_bits)
{
    const uint32_t mantissa_mask = (1u << mantissa_bits) - 1u;
    const uint32_t mantissa = value & mantissa_mask;
    const uint32_t exponent = value >> mantissa_bits;
    const float scale = float(1u << mantissa_bits);
    if (exponent == 0) {
        // Subnormal, which is where the small values in a dark scene live.
        return std::ldexp(float(mantissa) / scale, -14);
    }
    if (exponent == 31) {
        // Infinity or not a number. Neither is meaningful in an image, and a huge value would
        // dominate the range this dump reports, so it is clamped to something visible instead.
        return mantissa == 0 ? 65504.0f : 0.0f;
    }
    return std::ldexp(1.0f + float(mantissa) / scale, int(exponent) - 15);
}

// Half precision to float, written out rather than pulled in, because this file is compiled by two
// toolchains and neither is guaranteed a conversion intrinsic.
float half_to_float(uint16_t value)
{
    const uint32_t sign = (value & 0x8000u) << 16;
    uint32_t exponent = (value >> 10) & 0x1fu;
    uint32_t mantissa = value & 0x3ffu;
    if (exponent == 0) {
        if (mantissa == 0) {
            exponent = 0;
        } else {
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

// Only the formats the project actually needs to look at. Anything else is refused rather than
// guessed at, because a wrong decode looks plausible and would mislead.
bool decode(DXGI_FORMAT format, const uint8_t* pixel, Sample& out)
{
    switch (format) {
    case DXGI_FORMAT_R16G16_UNORM: {
        uint16_t raw[2];
        std::memcpy(raw, pixel, sizeof(raw));
        out.written = raw[0] != 0;
        out.x = raw[0] / 65535.0f;
        out.y = raw[1] / 65535.0f;
        return true;
    }
    case DXGI_FORMAT_R16G16_FLOAT: {
        uint16_t raw[2];
        std::memcpy(raw, pixel, sizeof(raw));
        auto half_to_float = [](uint16_t value) {
            const uint32_t sign = (value & 0x8000u) << 16;
            uint32_t exponent = (value >> 10) & 0x1fu;
            uint32_t mantissa = value & 0x3ffu;
            if (exponent == 0) {
                if (mantissa == 0) {
                    exponent = 0;
                } else {
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
        };
        out.x = half_to_float(raw[0]);
        out.y = half_to_float(raw[1]);
        out.written = raw[0] != 0 || raw[1] != 0;
        return true;
    }
    case DXGI_FORMAT_R32G32_FLOAT: {
        float raw[2];
        std::memcpy(raw, pixel, sizeof(raw));
        out.x = raw[0];
        out.y = raw[1];
        out.written = raw[0] != 0.0f || raw[1] != 0.0f;
        return true;
    }
    case DXGI_FORMAT_R11G11B10_FLOAT: {
        // What this game renders its scene colour into, so this is the input side of a comparison
        // with an upscaled result. Three unsigned floats packed into a word: red and green with
        // five exponent bits and six mantissa bits, blue with five of each, all biased by 15 and
        // with no sign bit, which is why the usual half conversion cannot be reused.
        uint32_t raw = 0;
        std::memcpy(&raw, pixel, sizeof(raw));
        out.x = small_float_to_float((raw >> 0) & 0x7ffu, 6);
        out.y = small_float_to_float((raw >> 11) & 0x7ffu, 6);
        out.z = small_float_to_float((raw >> 22) & 0x3ffu, 5);
        out.written = raw != 0;
        return true;
    }
    case DXGI_FORMAT_R16G16B16A16_FLOAT: {
        // The format a reconstruction writes its result in, so this is what makes an upscaled
        // frame something that can be looked at rather than only counted. Alpha is dropped: this
        // exists to show an image, not to preserve one.
        uint16_t raw[4];
        std::memcpy(raw, pixel, sizeof(raw));
        out.x = half_to_float(raw[0]);
        out.y = half_to_float(raw[1]);
        out.z = half_to_float(raw[2]);
        out.written = raw[0] != 0 || raw[1] != 0 || raw[2] != 0;
        return true;
    }
    default:
        return false;
    }
}

uint32_t bytes_per_pixel(DXGI_FORMAT format)
{
    switch (format) {
    case DXGI_FORMAT_R16G16_UNORM:
    case DXGI_FORMAT_R16G16_FLOAT:
        return 4;
    case DXGI_FORMAT_R32G32_FLOAT:
        return 8;
    case DXGI_FORMAT_R16G16B16A16_FLOAT:
        return 8;
    case DXGI_FORMAT_R11G11B10_FLOAT:
        return 4;
    default:
        return 0;
    }
}

uint8_t clamp_byte(float value)
{
    if (value <= 0.0f) {
        return 0;
    }
    if (value >= 255.0f) {
        return 255;
    }
    return static_cast<uint8_t>(value + 0.5f);
}

// Announce a step before taking it. Formatted here rather than in the sink so the sink can stay a
// plain string callback and cross a DLL boundary without a varargs contract.
void say(const rsf_texture_dump_options& options, const char* format, ...)
{
    if (!options.log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    options.log(options.log_user, message);
}

bool write_targa(const std::string& path, uint32_t width, uint32_t height,
                 const std::vector<uint8_t>& bgra)
{
    std::FILE* stream = std::fopen(path.c_str(), "wb");
    if (!stream) {
        return false;
    }
    // Uncompressed true colour, origin at top left. The simplest format anything can open.
    uint8_t header[18] = {};
    header[2] = 2;
    header[12] = static_cast<uint8_t>(width & 0xff);
    header[13] = static_cast<uint8_t>((width >> 8) & 0xff);
    header[14] = static_cast<uint8_t>(height & 0xff);
    header[15] = static_cast<uint8_t>((height >> 8) & 0xff);
    header[16] = 32;
    header[17] = 0x28;
    const bool ok = std::fwrite(header, 1, sizeof(header), stream) == sizeof(header) &&
                    std::fwrite(bgra.data(), 1, bgra.size(), stream) == bgra.size();
    std::fclose(stream);
    return ok;
}

} // namespace

extern "C" rsf_dump_texture_result rsf_dump_texture(void* device_pointer, void* context_pointer,
                                                    void* texture_pointer,
                                                    const rsf_texture_dump_options* options,
                                                    rsf_texture_dump_report* report)
{
    if (!device_pointer || !context_pointer || !texture_pointer || !options ||
        options->struct_size < sizeof(rsf_texture_dump_options) || !options->output_prefix_utf8) {
        return RSF_TEXTURE_ERROR_INVALID_ARGUMENT;
    }
    if (options->abi_version != RSF_TEXTURE_DUMP_ABI_VERSION) {
        return RSF_TEXTURE_ERROR_ABI_MISMATCH;
    }

    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    auto* texture = static_cast<ID3D11Texture2D*>(texture_pointer);

    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    say(*options, "dump %s: %ux%u format %u mips %u slices %u samples %u",
        options->output_prefix_utf8, desc.Width, desc.Height, unsigned(desc.Format), desc.MipLevels,
        desc.ArraySize, desc.SampleDesc.Count);

    const uint32_t stride = bytes_per_pixel(desc.Format);
    if (stride == 0) {
        return RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT;
    }

    // Copying between resources of different devices is invalid and takes the process down rather
    // than failing a call. A runtime with more than one device is not exotic: the observer creates
    // a throwaway one to reach the vtables, and any overlay in the process may create its own.
    ID3D11Device* owner = nullptr;
    texture->GetDevice(&owner);
    const bool same_device = owner == device;
    if (owner) {
        owner->Release();
    }
    if (!same_device) {
        say(*options, "dump: refused, texture belongs to another device");
        return RSF_TEXTURE_ERROR_FOREIGN_DEVICE;
    }

    // A staging copy leaves the game's own resource and binding state untouched.
    D3D11_TEXTURE2D_DESC staging = desc;
    staging.Usage = D3D11_USAGE_STAGING;
    staging.BindFlags = 0;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    staging.MiscFlags = 0;
    staging.MipLevels = 1;
    staging.ArraySize = 1;
    staging.SampleDesc.Count = 1;
    staging.SampleDesc.Quality = 0;

    say(*options, "dump: creating staging copy");
    ID3D11Texture2D* readable = nullptr;
    if (FAILED(device->CreateTexture2D(&staging, nullptr, &readable)) || !readable) {
        return RSF_TEXTURE_ERROR_STAGING_FAILED;
    }
    // CopyResource requires both resources to have the same subresource count, and the staging
    // copy deliberately has one. Anything with mips or slices therefore has to name the top one
    // explicitly instead.
    const bool single_subresource = desc.MipLevels <= 1 && desc.ArraySize <= 1;
    say(*options, "dump: copying (%s)",
        desc.SampleDesc.Count > 1 ? "resolve"
                                  : (single_subresource ? "whole resource" : "top subresource"));
    if (desc.SampleDesc.Count > 1) {
        context->ResolveSubresource(readable, 0, texture, 0, desc.Format);
    } else if (single_subresource) {
        context->CopyResource(readable, texture);
    } else {
        context->CopySubresourceRegion(readable, 0, 0, 0, 0, texture, 0, nullptr);
    }

    say(*options, "dump: mapping");
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(readable, 0, D3D11_MAP_READ, 0, &mapped))) {
        readable->Release();
        return RSF_TEXTURE_ERROR_MAP_FAILED;
    }
    say(*options, "dump: decoding %u rows", desc.Height);

    const float scale = options->scale != 0.0f ? options->scale : 1.0f;
    std::vector<uint8_t> image(static_cast<size_t>(desc.Width) * desc.Height * 4u);
    float min_x = 1e30f, max_x = -1e30f, min_y = 1e30f, max_y = -1e30f;
    size_t unwritten = 0;

    for (uint32_t y = 0; y < desc.Height; ++y) {
        const auto* row = static_cast<const uint8_t*>(mapped.pData) + size_t(y) * mapped.RowPitch;
        for (uint32_t x = 0; x < desc.Width; ++x) {
            Sample sample{};
            if (!decode(desc.Format, row + size_t(x) * stride, sample)) {
                continue;
            }
            float red = sample.x;
            float green = sample.y;
            if (options->view == RSF_DUMP_VIEW_DECODED_MOTION) {
                // Already decoded, so the only thing to recognise is the sentinel a decode pass
                // wrote where the source held its clear value.
                if (red <= RSF_DUMP_DECODED_SENTINEL_THRESHOLD) {
                    ++unwritten;
                    uint8_t* pixel = &image[(size_t(y) * desc.Width + x) * 4u];
                    pixel[0] = 220;  // blue, the same marker the velocity view uses
                    pixel[1] = 0;
                    pixel[2] = 0;
                    pixel[3] = 255;
                    continue;
                }
            }
            if (options->view == RSF_DUMP_VIEW_VELOCITY) {
                if (!sample.written) {
                    ++unwritten;
                    // Unwritten pixels are marked rather than drawn as motion, so the sentinel is
                    // visible instead of reading as a large negative velocity. Blue, because the
                    // motion channels are red and green and a marker sharing them reads as
                    // motion that is not there.
                    uint8_t* pixel = &image[(size_t(y) * desc.Width + x) * 4u];
                    pixel[0] = 220;  // blue
                    pixel[1] = 0;
                    pixel[2] = 0;
                    pixel[3] = 255;
                    continue;
                }
                red = (sample.x - velocity_bias) / velocity_scale;
                green = (sample.y - velocity_bias) / velocity_scale;
            }
            if (red < min_x) min_x = red;
            if (red > max_x) max_x = red;
            if (green < min_y) min_y = green;
            if (green > max_y) max_y = green;

            uint8_t* pixel = &image[(size_t(y) * desc.Width + x) * 4u];
            if (options->view == RSF_DUMP_VIEW_VELOCITY ||
                options->view == RSF_DUMP_VIEW_DECODED_MOTION) {
                // Zero motion sits at mid grey so sign is readable at a glance.
                pixel[2] = clamp_byte(128.0f + red * scale * 127.0f);
                pixel[1] = clamp_byte(128.0f + green * scale * 127.0f);
                pixel[0] = 128;
            } else {
                // Scene colour is linear and can exceed one, so this clips rather than tonemaps.
                // Good enough to see whether an upscaled frame is the scene at all, which is the
                // question being asked, and not a judgement of its brightness.
                pixel[2] = clamp_byte(red * scale * 255.0f);
                pixel[1] = clamp_byte(green * scale * 255.0f);
                pixel[0] = clamp_byte(sample.z * scale * 255.0f);
            }
            pixel[3] = 255;
        }
    }

    context->Unmap(readable, 0);
    readable->Release();

    // Nothing contributed a value, so the running extremes are still their starting sentinels.
    // Reporting those as a range would read as an enormous motion rather than as no data.
    if (min_x > max_x) {
        min_x = max_x = min_y = max_y = 0.0f;
    }

    const std::string prefix = options->output_prefix_utf8;
    say(*options, "dump: writing %s.tga", prefix.c_str());
    if (!write_targa(prefix + ".tga", desc.Width, desc.Height, image)) {
        return RSF_TEXTURE_ERROR_WRITE_FAILED;
    }

    const size_t total = size_t(desc.Width) * desc.Height;
    const float fraction = total ? float(unwritten) / float(total) : 0.0f;
    if (std::FILE* stream = std::fopen((prefix + ".json").c_str(), "wb")) {
        std::fprintf(stream,
                     "{\n  \"width\": %u,\n  \"height\": %u,\n  \"format\": %u,\n"
                     "  \"view\": %u,\n  \"fraction_unwritten\": %.4f,\n"
                     "  \"x_range\": [%.4f, %.4f],\n  \"y_range\": [%.4f, %.4f]\n}\n",
                     desc.Width, desc.Height, unsigned(desc.Format), options->view,
                     double(fraction), double(min_x), double(max_x), double(min_y), double(max_y));
        std::fclose(stream);
    }

    if (report && report->struct_size >= sizeof(rsf_texture_dump_report)) {
        report->width = desc.Width;
        report->height = desc.Height;
        report->format = static_cast<uint32_t>(desc.Format);
        report->bytes_written = static_cast<uint32_t>(image.size());
        report->fraction_unwritten = fraction;
        report->min_x = min_x;
        report->max_x = max_x;
        report->min_y = min_y;
        report->max_y = max_y;
    }
    say(*options, "dump: done, %.1f%% unwritten", double(fraction) * 100.0);
    return RSF_TEXTURE_OK;
}
