/* SPDX-License-Identifier: MIT */
#ifndef RSF_TEST_CENSUS_FACTS_H
#define RSF_TEST_CENSUS_FACTS_H
#include <stdint.h>

typedef struct rsf_test_census_functional_facts {
    uint64_t dispatch_calls;
    uint64_t anchor_rva;
    uint32_t image_size;
    uint32_t had_dxgi;
} rsf_test_census_functional_facts;

#endif
