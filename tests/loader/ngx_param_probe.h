/* SPDX-License-Identifier: MIT */
/* C declarations for the NVSDK_NGX_Parameter overload probe in vendor_census_ngx_platform.cpp.
   The compiler picks the virtual slot of each overload, so the calls stay in the C++ leaf. */
#ifndef RSF_TEST_NGX_PARAM_PROBE_H
#define RSF_TEST_NGX_PARAM_PROBE_H

#ifdef __cplusplus
extern "C" {
#endif

/* Number of overload slots the census relies on that the compiler disagrees with. */
int rsf_test_ngx_check_slots(void);

/* Set one value through the overload the C++ compiler selects for the argument type. */
void rsf_test_ngx_set_u32(void* parameters, const char* name, unsigned int value);
void rsf_test_ngx_set_i32(void* parameters, const char* name, int value);
void rsf_test_ngx_set_f32(void* parameters, const char* name, float value);
void rsf_test_ngx_set_resource12(void* parameters, const char* name, void* resource);

#ifdef __cplusplus
}
#endif
#endif
