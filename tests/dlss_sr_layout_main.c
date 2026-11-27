/* SPDX-License-Identifier: MIT */
/* Runs the Rust comparison of the Streamline records with the vendor C++ declarations that
   dlss_sr_layout_facts.cpp reports. */
#include <stddef.h>
#include <stdint.h>

size_t rsf_test_dlss_sr_default(uint32_t kind, void* out, size_t capacity);
size_t rsf_test_dlss_sr_values(uint64_t* out, size_t capacity);

typedef struct rsf_dlss_sr_layout_facts {
    size_t (*default_object)(uint32_t, void*, size_t);
    size_t (*values)(uint64_t*, size_t);
} rsf_dlss_sr_layout_facts;

int rsf_fixture_dlss_sr_layout(const rsf_dlss_sr_layout_facts* facts);

int main(void)
{
    const rsf_dlss_sr_layout_facts facts = {rsf_test_dlss_sr_default, rsf_test_dlss_sr_values};
    return rsf_fixture_dlss_sr_layout(&facts);
}
