/* SPDX-License-Identifier: MIT */
#include "ac7_capture_io.h"
#include <stdio.h>

/* Preserve the carrier's narrow CRT path and short-write/close checks. */
bool rsf_ac7_capture_write_file(const char* path, const unsigned char* data, size_t bytes)
{
    FILE* file = fopen(path, "wb");
    if (!file) return false;
    const bool ok = bytes == 0 || fwrite(data, 1, bytes, file) == bytes;
    return fclose(file) == 0 && ok;
}
