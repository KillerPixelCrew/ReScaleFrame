/* SPDX-License-Identifier: MIT */
#include <rescaleframe/ac7_view.h>

int rsf_ac7_view_probe(const char* directory);

int main(int argc, char** argv)
{
    if (argc == 2) return rsf_ac7_view_probe(argv[1]);
    rsf_ac7_view view = { .struct_size = (uint32_t)sizeof(view) };
    return rsf_ac7_view_read(NULL, RSF_AC7_VIEW_BUFFER_BYTES, RSF_AC7_VIEW_ABI_VERSION,
                            &view) == RSF_AC7_VIEW_ERROR_INVALID_ARGUMENT ? 0 : 1;
}
