/* SPDX-License-Identifier: MIT */
#include <rescaleframe/ac7_view.h>
#include <stdio.h>

/* The view probe's capture reader (tests/ac7_view_probe.rs). Keep the launcher argument in the
   Windows C runtime's narrow path encoding. */
int rsf_ac7_view_capture_read(const char* directory, int index, unsigned char* data)
{
    char path[1024];
    snprintf(path, sizeof(path), "%s\\capture%02d_cb4096.bin", directory, index);
    FILE* stream = fopen(path, "rb");
    if (!stream) return 0;
    const size_t got = fread(data, 1, RSF_AC7_VIEW_BUFFER_BYTES, stream);
    fclose(stream);
    return got == RSF_AC7_VIEW_BUFFER_BYTES;
}
