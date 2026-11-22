/* SPDX-License-Identifier: MIT */
/* Native declarations and guarded vendor calls for the Rust census. */
#include <stdio.h>

#include "census_internal.h"
#include "fsr_platform.h"
#include "misc_platform.h"
#include "ngx_platform.h"
#include "wrapper_platform.h"

const rsf_census_hook_desc rsf_census_functions[RSF_FN_COUNT] = {
#define X(id, api, name, mods) {#id, api, name, mods},
    RSF_CENSUS_FUNCTIONS(X)
#undef X
};

static int format_decimal(char* text, size_t size, double value)
{
    return snprintf(text, size, "%.9g", value);
}

static bool read_wide(const void* address, uint16_t* text, size_t capacity)
{
    return rsf_census_read_arg_wstring(address, (wchar_t*)text, capacity);
}

static bool read_ngx_parameter(const void* parameters, size_t slot, const char* name, void* output,
                               int32_t* result)
{
    typedef int32_t (*Getter)(const void*, const char*, void*);
    Getter get = (Getter)rsf_census_vtable_slot(parameters, slot);
    if (!get) return false;
    RSF_FOREIGN_TRY {
        *result = get(parameters, name, output);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

static bool read_streamline_frame(const void* token, uint32_t* value)
{
    typedef uint32_t (*Getter)(const void*);
    Getter get = (Getter)rsf_census_vtable_slot(token, 0);
    if (!get) return false;
    RSF_FOREIGN_TRY {
        *value = get(token);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

static void write_streamline_output(void** output, void* value)
{
    RSF_FOREIGN_TRY {
        *output = value;
    } RSF_FAULT_CATCH {
    }
}

static void describe_resource12(rsf_census_json* state, const char* key, const void* resource)
{
    rsf_census_describe_d3d_resource(state, key, resource, true);
}

static const uint16_t vk_entries[] = {
#define RSF_CENSUS_VK_MAP(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_MAP)
#undef RSF_CENSUS_VK_MAP
};
_Static_assert(sizeof(vk_entries) / sizeof(vk_entries[0]) == RSF_CENSUS_VK_COUNT,
               "one function id per Vulkan wrapper");
static const rsf_census_vk_platform vk_platform = {rsf_census_safe_read,
                                                   rsf_census_read_arg_string};

static const uint16_t ngx_entries[] = {
#define RSF_CENSUS_NGX_ID(name, api, symbol, modules) (uint16_t)RSF_FN_##name,
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D12, NULL, NULL)
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D11, NULL, NULL)
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, VULKAN, NULL, NULL)
#undef RSF_CENSUS_NGX_ID
};
_Static_assert(sizeof(ngx_entries) / sizeof(ngx_entries[0]) == RSF_CENSUS_NGX_ENTRIES,
               "one function id per NGX wrapper");
static const rsf_census_ngx_platform ngx_platform = {
    rsf_census_safe_read,
    rsf_census_read_arg_string,
    read_wide,
    read_ngx_parameter,
    rsf_census_describe_d3d_resource,
    format_decimal,
};

static const uint16_t misc_entries[] = {
#define RSF_CENSUS_MISC_ID(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_MISC_ENTRIES(RSF_CENSUS_MISC_ID)
#undef RSF_CENSUS_MISC_ID
};
_Static_assert(sizeof(misc_entries) / sizeof(misc_entries[0]) == RSF_MISC_COUNT,
               "one function id per miscellaneous wrapper");
static const rsf_census_misc_platform misc_platform = {
    rsf_census_safe_read,
    rsf_census_read_arg_string,
    read_wide,
    read_streamline_frame,
    write_streamline_output,
    describe_resource12,
    format_decimal,
};

static const uint16_t fsr_entries[] = {
#define RSF_CENSUS_FSR_ID(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_FSR_ENTRIES(RSF_CENSUS_FSR_ID)
#undef RSF_CENSUS_FSR_ID
};
_Static_assert(sizeof(fsr_entries) / sizeof(fsr_entries[0]) == RSF_FSR_COUNT,
               "one function id per FSR and XeSS wrapper");
static const rsf_census_fsr_platform fsr_platform = {
    rsf_census_safe_read,
    rsf_census_vtable_slot,
    describe_resource12,
    format_decimal,
};

static const rsf_census_vendor_platform vendor_platform = {
    vk_entries,   &vk_platform,   ngx_entries,  &ngx_platform,
    fsr_entries,  &fsr_platform,  misc_entries, &misc_platform,
};

rsf_census_result rsf_census_install(const rsf_census_options* options)
{
    static const rsf_census_core_platform platform = {
        rsf_census_functions,
        (uint16_t)RSF_FN_COUNT,
        (uint16_t)RSF_FN_CensusEvent,
        &rsf_census_hook_platform_table,
        &vendor_platform,
    };
    return rsf_census_install_impl(options, &platform);
}
