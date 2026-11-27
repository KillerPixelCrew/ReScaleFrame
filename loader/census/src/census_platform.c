/* SPDX-License-Identifier: MIT */
/* The C leaves of the Rust census, gathered into the one table rsf_census_install hands over. */
#include <stdio.h>

#include "MinHook.h"
#include "census_internal.h"
#include "d3d_platform.h"
#include "fsr_platform.h"
#include "misc_platform.h"
#include "wrapper_platform.h"

_Static_assert(RSF_FN_COUNT < 1024, "the hook table indexes functions with 10 bits");

const rsf_census_hook_desc rsf_census_functions[RSF_FN_COUNT] = {
#define X(id, api, name, mods) {#id, api, name, mods},
    RSF_CENSUS_FUNCTIONS(X)
#undef X
};

static const uint16_t d3d_entries[] = {
#define RSF_CENSUS_D3D_ID(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_D3D_ENTRIES(RSF_CENSUS_D3D_ID)
#undef RSF_CENSUS_D3D_ID
};
_Static_assert(sizeof(d3d_entries) / sizeof(d3d_entries[0]) == RSF_D3D_COUNT,
               "one function id per D3D wrapper slot");

static const uint16_t vk_entries[] = {
#define RSF_CENSUS_VK_MAP(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_VK_ENTRIES(RSF_CENSUS_VK_MAP)
#undef RSF_CENSUS_VK_MAP
};
_Static_assert(sizeof(vk_entries) / sizeof(vk_entries[0]) == RSF_CENSUS_VK_COUNT,
               "one function id per Vulkan wrapper");

static const uint16_t ngx_entries[] = {
#define RSF_CENSUS_NGX_ID(name, api, symbol, modules) (uint16_t)RSF_FN_##name,
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D12, NULL, NULL)
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, D3D11, NULL, NULL)
    RSF_NGX_FUNCS(RSF_CENSUS_NGX_ID, VULKAN, NULL, NULL)
#undef RSF_CENSUS_NGX_ID
};
_Static_assert(sizeof(ngx_entries) / sizeof(ngx_entries[0]) == RSF_CENSUS_NGX_ENTRIES,
               "one function id per NGX wrapper");

static const uint16_t fsr_entries[] = {
#define RSF_CENSUS_FSR_ID(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_FSR_ENTRIES(RSF_CENSUS_FSR_ID)
#undef RSF_CENSUS_FSR_ID
};
_Static_assert(sizeof(fsr_entries) / sizeof(fsr_entries[0]) == RSF_FSR_COUNT,
               "one function id per FSR and XeSS wrapper");

static const uint16_t misc_entries[] = {
#define RSF_CENSUS_MISC_ID(name) (uint16_t)RSF_FN_##name,
    RSF_CENSUS_MISC_ENTRIES(RSF_CENSUS_MISC_ID)
#undef RSF_CENSUS_MISC_ID
};
_Static_assert(sizeof(misc_entries) / sizeof(misc_entries[0]) == RSF_MISC_COUNT,
               "one function id per miscellaneous wrapper");

static int hook_initialize(void) { return MH_Initialize(); }
static int hook_create_api(const uint16_t* module, const char* name, void* detour, void** original)
{
    return MH_CreateHookApi((const wchar_t*)module, name, detour, original);
}
static int hook_create(void* target, void* detour, void** original)
{
    return MH_CreateHook(target, detour, original);
}
static void hook_remove(void* target) { MH_RemoveHook(target); }
static int hook_enable(void* target) { return MH_QueueEnableHook(target); }
static void hook_apply(void) { MH_ApplyQueued(); }
static void hook_disable(void) { MH_DisableHook(MH_ALL_HOOKS); }
static const void* hook_current_teb(void) { return NtCurrentTeb(); }

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

static int format_decimal(char* text, size_t size, double value)
{
    return snprintf(text, size, "%.9g", value);
}

static const rsf_census_platform platform = {
    .entries = rsf_census_functions,
    .count = (uint16_t)RSF_FN_COUNT,
    .event_entry = (uint16_t)RSF_FN_CensusEvent,
    .graphics_roots = {(uint16_t)RSF_FN_CreateDXGIFactory, (uint16_t)RSF_FN_CreateDXGIFactory1,
                       (uint16_t)RSF_FN_CreateDXGIFactory2, (uint16_t)RSF_FN_D3D12CreateDevice},
    .d3d_entries = d3d_entries,
    .vk_entries = vk_entries,
    .ngx_entries = ngx_entries,
    .fsr_entries = fsr_entries,
    .misc_entries = misc_entries,
    .initialize = hook_initialize,
    .create_api = hook_create_api,
    .create = hook_create,
    .remove = hook_remove,
    .enable = hook_enable,
    .apply = hook_apply,
    .disable = hook_disable,
    .current_teb = hook_current_teb,
    .read = rsf_census_safe_read,
    .read_string = rsf_census_read_arg_string,
    .read_wstring = read_wide,
    .read_name = rsf_census_safe_read_cstr,
    .slot = rsf_census_vtable_slot,
    .parameter = read_ngx_parameter,
    .frame_index = read_streamline_frame,
    .write_ptr = write_streamline_output,
    .query = rsf_census_query_interface,
    .release = rsf_census_release_interface,
    .queue_desc = rsf_census_queue_desc,
    .swapchain_desc = rsf_census_swapchain_desc,
    .resource12_desc = rsf_census_resource12_desc,
    .resource11_dimension = rsf_census_resource11_dimension,
    .resource11_desc = rsf_census_resource11_desc,
    .decimal = format_decimal,
};

rsf_census_result rsf_census_install(const rsf_census_options* options)
{
    return rsf_census_install_impl(options, &platform);
}