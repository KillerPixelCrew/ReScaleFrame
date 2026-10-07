// SPDX-License-Identifier: GPL-3.0-only
#include "contact_shadow.h"
#include <windows.h>
#include <wincrypt.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>

// Exact-variant DXBC transform at native shader creation. Container sizes, tokens and checksum
// are validated before edits; an engine trailer is preserved outside the rebuilt container.
// Evidence: docs/research/ac7-lighting-shadow-20261004.md and ue418-hook-map.md.
namespace {
uint32_t word(const uint8_t* bytes, size_t offset)
{
    uint32_t value = 0; std::memcpy(&value, bytes + offset, sizeof(value)); return value;
}
void store(uint8_t* bytes, size_t offset, uint32_t value)
{ std::memcpy(bytes + offset, &value, sizeof(value)); }
// Fingerprint the complete original container before interpreting its build-specific token offsets.
bool fingerprint(const uint8_t* bytes, size_t size)
{
    constexpr std::array<uint8_t, 32> expected{
        0x60,0xa2,0xca,0x61,0xe5,0xd8,0x3b,0x85,0x3f,0x69,0x2e,0xe8,0xc0,0xbc,0x35,0x80,
        0xea,0x46,0xb8,0xe3,0x3d,0x72,0xc6,0x44,0x94,0x9e,0x5f,0x37,0xc1,0x06,0x0a,0x12};
    HMODULE module = LoadLibraryExW(L"crypt32.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return false;
    auto hash = reinterpret_cast<decltype(&CryptHashCertificate2)>(
        reinterpret_cast<void*>(GetProcAddress(module, "CryptHashCertificate2")));
    std::array<uint8_t, 32> actual{}; DWORD count = DWORD(actual.size());
    const bool ok = hash && hash(L"SHA256", 0, nullptr, bytes, DWORD(size), actual.data(), &count) &&
        count == actual.size() && actual == expected;
    FreeLibrary(module); return ok;
}
uint32_t rotate(uint32_t value, uint32_t bits)
{ return (value << bits) | (value >> (32 - bits)); }
void compress(std::array<uint32_t, 4>& state, const uint8_t* block)
{
    // Standard MD5 compression; only the DXBC final-block framing differs.
    static const auto constants = [] {
        std::array<uint32_t, 64> result{};
        for (size_t i = 0; i < result.size(); ++i)
            result[i] = uint32_t(std::floor(std::fabs(std::sin(double(i + 1))) * 4294967296.0));
        return result;
    }();
    constexpr uint32_t shifts[4][4]{{7,12,17,22},{5,9,14,20},{4,11,16,23},{6,10,15,21}};
    uint32_t a = state[0], b = state[1], c = state[2], d = state[3];
    for (uint32_t i = 0; i < 64; ++i) {
        uint32_t f = 0, index = 0;
        if (i < 16) { f = (b & c) | (~b & d); index = i; }
        else if (i < 32) { f = (d & b) | (~d & c); index = (5 * i + 1) % 16; }
        else if (i < 48) { f = b ^ c ^ d; index = (3 * i + 5) % 16; }
        else { f = c ^ (b | ~d); index = (7 * i) % 16; }
        const uint32_t next = b + rotate(a + f + constants[i] + word(block, index * 4), shifts[i / 16][i % 4]);
        a = d; d = c; c = b; b = next;
    }
    state[0] += a; state[1] += b; state[2] += c; state[3] += d;
}
std::array<uint32_t, 4> checksum(const uint8_t* bytes, size_t size)
{
    // DXBC hashes the container from byte 20. Its terminal block stores the bit
    // count before a short remainder, or in a new block after a long remainder.
    const auto* data = bytes + 20; const size_t count = size - 20;
    std::array<uint32_t, 4> state{0x67452301u,0xefcdab89u,0x98badcfeu,0x10325476u};
    const size_t whole = count & ~size_t(63);
    for (size_t i = 0; i < whole; i += 64) compress(state, data + i);
    std::array<uint8_t, 64> tail{}; const size_t remainder = count - whole;
    const auto bits = uint32_t(count * 8);
    if (remainder >= 56) {
        std::memcpy(tail.data(), data + whole, remainder); tail[remainder] = 0x80;
        compress(state, tail.data()); tail = {}; store(tail.data(), 0, bits);
    } else {
        store(tail.data(), 0, bits);
        std::memcpy(tail.data() + 4, data + whole, remainder); tail[remainder + 4] = 0x80;
    }
    store(tail.data(), 60, (bits >> 2) | 1u); compress(state, tail.data()); return state;
}
}

rsf_ac7_contact_shadow_result rsf_ac7_contact_shadow_correct(
    const void* bytes, size_t size, std::vector<uint8_t>& output) noexcept
{
    output.clear();
    constexpr size_t original_size = 10456, shader_offset = 208, shader_size = 10240;
    const auto* input = static_cast<const uint8_t*>(bytes);
    if (!input || size < original_size || size > 8 * 1024 * 1024 ||
        std::memcmp(input, "DXBC", 4) || word(input, 24) != original_size ||
        !fingerprint(input, original_size)) return rsf_ac7_contact_shadow_result::unmatched;
    try {
        // The full fingerprint guards all operands/signatures. Explicit layout checks
        // also keep container/token growth local and prevent processing another variant.
        if (word(input, 20) != 1 || word(input, 28) != 3 || word(input, 32) != 44 ||
            word(input, 36) != 156 || word(input, 40) != shader_offset ||
            std::memcmp(input + 44, "ISGN", 4) || word(input, 48) != 104 ||
            std::memcmp(input + 156, "OSGN", 4) || word(input, 160) != 44 ||
            std::memcmp(input + shader_offset, "SHEX", 4) || word(input, shader_offset + 4) != shader_size)
            return rsf_ac7_contact_shadow_result::refused;
        const auto old_hash = checksum(input, original_size);
        if (std::memcmp(input + 4, old_hash.data(), 16)) return rsf_ac7_contact_shadow_result::refused;
        constexpr size_t tokens = shader_offset + 8, phase_token = 469, temps_token = 66, start_token = 996;
        constexpr std::array<uint32_t, 10> noise{0x0a00000fu,0x00100012u,1u,0x00101046u,2u,
            0x00004002u,0x3d897143u,0x3bbf4590u,0u,0u};
        // mov r10.z, r8.z: the ray's starting depth, read by every one of the eight samples.
        constexpr std::array<uint32_t, 5> ray_start{0x05000036u,0x00100042u,0x0000000au,0x0010002au,0x00000008u};
        if (word(input, tokens) != 0x50 || word(input, tokens + 4) != shader_size / 4 ||
            word(input, tokens + temps_token * 4) != 0x02000068 ||
            word(input, tokens + (temps_token + 1) * 4) != 14 ||
            std::memcmp(input + tokens + phase_token * 4, noise.data(), sizeof(noise)) ||
            std::memcmp(input + tokens + start_token * 4, ray_start.data(), sizeof(ray_start)))
            return rsf_ac7_contact_shadow_result::refused;
        // 1. Noise phase. Temporary r14 holds the current View's uint phase, then the pixel
        //    offset UE's InterleavedGradientNoise applies; the dot product reads it.
        constexpr std::array<uint32_t, 18> phase{
            0x06000056u,0x00100012u,14u,0x0020802au,0u,137u,
            0x0c000032u,0x00100032u,14u,0x00100006u,14u,0x00004002u,
            0x4202a8f6u,0x413d0a3du,0u,0u,0x00101046u,2u};
        // 2. Point-sampled scene depth can falsely occlude a grazing-angle ray by half a texel
        //    per axis. Estimate that bias from four neighbours, using the smaller side per axis
        //    to avoid silhouette inflation, and add it to the starting depth.
        //    Uses new temporaries r15..r17:
        //      add r15, v0.xyxy, cb0[128].zwzw / add r16, v0.xyxy, -cb0[128].zwzw
        //      mov r15.y, v0.y / mov r16.y, v0.y / mov r15.z, v0.x / mov r16.z, v0.x
        //      sample_l r17.x..w at (u+du,v) (u-du,v) (u,v+dv) (u,v-dv) from t4 with s4, LOD 0
        //      sample_l r16.w at (u,v), the centre texel
        //      add r17, r17, -r16.wwww
        //      min r15.x, |r17.x|, |r17.y| / min r15.y, |r17.z|, |r17.w|
        //      max r16.x, |r17.x|, |r17.y| / max r16.y, |r17.z|, |r17.w|
        //      movc r15.xy, r15.xyxx, r15.xyxx, r16.xyxx   (a clamped edge sample reads the
        //          pixel itself and gives zero; the other side is used there)
        //      add r15.x, r15.x, r15.y / mul r15.x, r15.x, 0.5
        //    and the ray start becomes add r10.z, r8.z, r15.x.
        constexpr std::array<uint32_t, 169> bias{
            0x08000000u,0x001000f2u,0x0000000fu,0x00101446u,0x00000000u,0x00208ee6u,
            0x00000000u,0x00000080u,0x09000000u,0x001000f2u,0x00000010u,0x00101446u,
            0x00000000u,0x80208ee6u,0x00000041u,0x00000000u,0x00000080u,0x05000036u,
            0x00100022u,0x0000000fu,0x0010101au,0x00000000u,0x05000036u,0x00100022u,
            0x00000010u,0x0010101au,0x00000000u,0x05000036u,0x00100042u,0x0000000fu,
            0x0010100au,0x00000000u,0x05000036u,0x00100042u,0x00000010u,0x0010100au,
            0x00000000u,0x8d000048u,0x800000c2u,0x00155543u,0x00100012u,0x00000011u,
            0x00100046u,0x0000000fu,0x00107e46u,0x00000004u,0x00106000u,0x00000004u,
            0x00004001u,0x00000000u,0x8d000048u,0x800000c2u,0x00155543u,0x00100022u,
            0x00000011u,0x00100046u,0x00000010u,0x00107e16u,0x00000004u,0x00106000u,
            0x00000004u,0x00004001u,0x00000000u,0x8d000048u,0x800000c2u,0x00155543u,
            0x00100042u,0x00000011u,0x00100ae6u,0x0000000fu,0x00107c96u,0x00000004u,
            0x00106000u,0x00000004u,0x00004001u,0x00000000u,0x8d000048u,0x800000c2u,
            0x00155543u,0x00100082u,0x00000011u,0x00100ae6u,0x00000010u,0x00107396u,
            0x00000004u,0x00106000u,0x00000004u,0x00004001u,0x00000000u,0x8d000048u,
            0x800000c2u,0x00155543u,0x00100082u,0x00000010u,0x00101046u,0x00000000u,
            0x00107396u,0x00000004u,0x00106000u,0x00000004u,0x00004001u,0x00000000u,
            0x08000000u,0x001000f2u,0x00000011u,0x00100e46u,0x00000011u,0x80100ff6u,
            0x00000041u,0x00000010u,0x09000033u,0x00100012u,0x0000000fu,0x8010000au,
            0x00000081u,0x00000011u,0x8010001au,0x00000081u,0x00000011u,0x09000033u,
            0x00100022u,0x0000000fu,0x8010002au,0x00000081u,0x00000011u,0x8010003au,
            0x00000081u,0x00000011u,0x09000034u,0x00100012u,0x00000010u,0x8010000au,
            0x00000081u,0x00000011u,0x8010001au,0x00000081u,0x00000011u,0x09000034u,
            0x00100022u,0x00000010u,0x8010002au,0x00000081u,0x00000011u,0x8010003au,
            0x00000081u,0x00000011u,0x09000037u,0x00100032u,0x0000000fu,0x00100046u,
            0x0000000fu,0x00100046u,0x0000000fu,0x00100046u,0x00000010u,0x07000000u,
            0x00100012u,0x0000000fu,0x0010000au,0x0000000fu,0x0010001au,0x0000000fu,
            0x07000038u,0x00100012u,0x0000000fu,0x0010000au,0x0000000fu,0x00004001u,
            0x3f000000u
        };
        constexpr std::array<uint32_t, 7> biased_start{
            0x07000000u,0x00100042u,0x0000000au,0x0010002au,0x00000008u,0x0010000au,
            0x0000000fu
        };
        constexpr size_t phase_split = tokens + phase_token * 4, start_split = tokens + start_token * 4;
        constexpr size_t growth = sizeof(phase) + sizeof(bias) + sizeof(biased_start) - sizeof(ray_start);
        output.resize(size + growth);
        uint8_t* out = output.data();
        auto put = [&out](const void* data, size_t bytes) { std::memcpy(out, data, bytes); out += bytes; };
        put(input, phase_split);
        put(phase.data(), sizeof(phase));
        put(input + phase_split, start_split - phase_split);
        put(bias.data(), sizeof(bias));
        put(biased_start.data(), sizeof(biased_start));
        put(input + start_split + sizeof(ray_start), size - start_split - sizeof(ray_start));
        store(output.data(), 24, uint32_t(original_size + growth));
        store(output.data(), shader_offset + 4, uint32_t(shader_size + growth));
        store(output.data(), tokens + 4, uint32_t((shader_size + growth) / 4));
        store(output.data(), tokens + (temps_token + 1) * 4, 18);
        // The noise dot product now reads r14.xy instead of v2.xy.
        const size_t noise_at = phase_split + sizeof(phase);
        store(output.data(), noise_at + 3 * 4, 0x00100046u);
        store(output.data(), noise_at + 4 * 4, 14);
        const auto updated_hash = checksum(output.data(), original_size + growth);
        std::memcpy(output.data() + 4, updated_hash.data(), 16);
        return rsf_ac7_contact_shadow_result::patched;
    } catch (...) { output.clear(); return rsf_ac7_contact_shadow_result::refused; }
}

uint32_t rsf_ac7_contact_shadow_crc(const void* bytes, size_t size) noexcept
{
    // CRC-32C of the DXBC container alone, as D3D11 receives it and as captures identify it.
    const auto* data = static_cast<const uint8_t*>(bytes);
    if (!data || size < 28) return 0;
    const uint32_t length = word(data, 24);
    if (length > size) return 0;
    uint32_t crc = 0xFFFFFFFFu;
    for (uint32_t i = 0; i < length; ++i) {
        crc ^= data[i];
        for (int bit = 0; bit < 8; ++bit) crc = (crc >> 1) ^ (0x82F63B78u & (0u - (crc & 1u)));
    }
    return ~crc;
}
