/* SPDX-License-Identifier: GPL-3.0-only */
/* SHA-256 (FIPS 180-4). Self-contained so the sidecar needs no crypto provider. */

#include "module_dump_internal.h"

#include <string.h>

static const uint32_t k[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u,
    0xab1c5ed5u, 0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu,
    0x9bdc06a7u, 0xc19bf174u, 0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu,
    0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau, 0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u, 0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu,
    0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u, 0xa2bfe8a1u, 0xa81a664bu,
    0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u, 0x19a4c116u,
    0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u,
    0xc67178f2u};

#define ROTR(value, bits) (((value) >> (bits)) | ((value) << (32 - (bits))))

static void compress(uint32_t state[8], const unsigned char block[64])
{
    uint32_t w[64];
    for (int index = 0; index < 16; ++index) {
        w[index] = ((uint32_t)block[index * 4] << 24) | ((uint32_t)block[index * 4 + 1] << 16) |
                   ((uint32_t)block[index * 4 + 2] << 8) | (uint32_t)block[index * 4 + 3];
    }
    for (int index = 16; index < 64; ++index) {
        const uint32_t s0 = ROTR(w[index - 15], 7) ^ ROTR(w[index - 15], 18) ^ (w[index - 15] >> 3);
        const uint32_t s1 = ROTR(w[index - 2], 17) ^ ROTR(w[index - 2], 19) ^ (w[index - 2] >> 10);
        w[index] = w[index - 16] + s0 + w[index - 7] + s1;
    }
    uint32_t a = state[0], b = state[1], c = state[2], d = state[3];
    uint32_t e = state[4], f = state[5], g = state[6], h = state[7];
    for (int index = 0; index < 64; ++index) {
        const uint32_t s1 = ROTR(e, 6) ^ ROTR(e, 11) ^ ROTR(e, 25);
        const uint32_t choose = (e & f) ^ (~e & g);
        const uint32_t t1 = h + s1 + choose + k[index] + w[index];
        const uint32_t s0 = ROTR(a, 2) ^ ROTR(a, 13) ^ ROTR(a, 22);
        const uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
        const uint32_t t2 = s0 + majority;
        h = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }
    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    state[4] += e;
    state[5] += f;
    state[6] += g;
    state[7] += h;
}

void rsf_sha256_hex(const void* data, size_t size, char out[65])
{
    uint32_t state[8] = {0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
                         0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u};
    const unsigned char* bytes = (const unsigned char*)data;
    size_t offset = 0;
    for (; offset + 64 <= size; offset += 64) {
        compress(state, bytes + offset);
    }

    unsigned char tail[128];
    const size_t rest = size - offset;
    memset(tail, 0, sizeof(tail));
    if (rest) {
        memcpy(tail, bytes + offset, rest);
    }
    tail[rest] = 0x80;
    const size_t tail_size = rest < 56 ? 64 : 128;
    const uint64_t bits = (uint64_t)size * 8u;
    for (int index = 0; index < 8; ++index) {
        tail[tail_size - 1 - (size_t)index] = (unsigned char)(bits >> (8 * index));
    }
    compress(state, tail);
    if (tail_size == 128) {
        compress(state, tail + 64);
    }

    static const char digits[] = "0123456789abcdef";
    for (int index = 0; index < 8; ++index) {
        for (int shift = 0; shift < 4; ++shift) {
            const unsigned char value = (unsigned char)(state[index] >> (24 - 8 * shift));
            out[index * 8 + shift * 2] = digits[value >> 4];
            out[index * 8 + shift * 2 + 1] = digits[value & 15];
        }
    }
    out[64] = '\0';
}
