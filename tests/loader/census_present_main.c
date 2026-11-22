/* SPDX-License-Identifier: MIT */
#include <rescaleframe/vendor_census.h>

int rsf_test_census_present(int, char**, rsf_census_result (*)(const rsf_census_options*),
                            rsf_census_result (*)(void));

int main(int argc, char** argv)
{
    return rsf_test_census_present(argc, argv, rsf_census_install, rsf_census_flush);
}
