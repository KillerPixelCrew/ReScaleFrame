// SPDX-License-Identifier: MIT
// C entry for the shared post-pass read, so a C carrier uses the settings store and the public
// sharpening range rules instead of its own copies. Internal; not exported from the Runtime.
#include "render_settings_store.h"

extern "C" void rsf_settings_store_read_sharpening(const wchar_t* ini, const wchar_t* section,
    rsf_sharpening_options* output)
{
    if (!output) return;
    try {
        *output = rsf::render_settings::read_sharpening(ini ? ini : L"", section ? section : L"");
    } catch (...) {
        *output = rsf_sharpening_defaults();
    }
}
