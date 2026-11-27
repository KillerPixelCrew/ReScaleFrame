/* SPDX-License-Identifier: MIT */
#ifndef RSF_TEST_CENSUS_FACTS_H
#define RSF_TEST_CENSUS_FACTS_H
#include <stdint.h>
#include <rescaleframe/vendor_census.h>

typedef struct rsf_test_census_functional_facts {
    uint64_t dispatch_calls;
    const void* anchor;
    uint32_t had_dxgi;
} rsf_test_census_functional_facts;

typedef struct rsf_test_census_native {
    rsf_census_result (*install)(const rsf_census_options*);
    rsf_census_result (*flush)(void);
    rsf_census_result (*counters)(rsf_census_counters*);
    rsf_census_result (*shutdown)(void);
    int (*check_vtable)(void);
    int (*functional)(rsf_test_census_functional_facts*);
    int (*after_shutdown)(void);
    int (*jitter)(int index);
    int (*vulkan)(void);
    int (*ffx)(void);
    void (*dispatch_twice)(int* results);
} rsf_test_census_native;

#endif
