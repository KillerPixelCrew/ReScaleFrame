/* SPDX-License-Identifier: GPL-3.0-only */
/* Entropy measurement. No platform dependency, so it is testable anywhere. */

#include <rescaleframe/module_dump.h>

#include <math.h>

double rsf_shannon_entropy(const void* data, size_t size)
{
    if (!data || size == 0) {
        return 0.0;
    }

    size_t counts[256] = {0};
    const unsigned char* bytes = (const unsigned char*)data;
    for (size_t index = 0; index < size; ++index) {
        ++counts[bytes[index]];
    }

    const double total = (double)size;
    double entropy = 0.0;
    for (size_t value = 0; value < 256; ++value) {
        if (counts[value] == 0) {
            continue;
        }
        const double probability = (double)counts[value] / total;
        entropy -= probability * log2(probability);
    }
    return entropy;
}

double rsf_sampled_entropy(const void* data, size_t size, size_t window, size_t samples)
{
    if (!data || size == 0 || window == 0 || samples == 0) {
        return 0.0;
    }
    if (size <= window) {
        return rsf_shannon_entropy(data, size);
    }

    const unsigned char* bytes = (const unsigned char*)data;
    const size_t span = size - window;
    double total = 0.0;
    size_t taken = 0;
    for (size_t index = 0; index < samples; ++index) {
        /* Spread the windows evenly across the range, including both ends, so a section that is
           only partly decrypted cannot hide behind a single lucky sample. */
        const size_t offset = samples == 1 ? 0 : (span * index) / (samples - 1);
        total += rsf_shannon_entropy(bytes + offset, window);
        ++taken;
    }
    return total / (double)taken;
}
