/* SPDX-License-Identifier: MIT */
#include <stdbool.h>
#include <wchar.h>
#include <rescaleframe/vendor_census.h>

bool rsf_rdr2_install_capture_fixture(const wchar_t* directory);
bool rsf_rdr2_fixture_ngx_flags(void* parameters);
bool rsf_rdr2_fixture_ngx_inputs(void* parameters, void* color, void* depth, void* motion, void* output);
int rsf_rdr2_capture_fixture_main(bool (*setup)(const wchar_t*),
    rsf_census_result (*start)(const rsf_census_options*), rsf_census_result (*stop)(void),
    bool (*flags)(void*), bool (*inputs)(void*, void*, void*, void*, void*));

int main(void)
{
    return rsf_rdr2_capture_fixture_main(rsf_rdr2_install_capture_fixture, rsf_census_install,
                                        rsf_census_shutdown, rsf_rdr2_fixture_ngx_flags,
                                        rsf_rdr2_fixture_ngx_inputs);
}
