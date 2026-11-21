// SPDX-License-Identifier: MIT
#include "native_settings.h"
#include "asi_platform.h"
#include <MinHook.h>

extern "C" {
bool rsf_rdr2_settings_install(const wchar_t* path, const rsf::rdr2::DiagnosticHook* hooks,
    int32_t (*enable)(uintptr_t)) noexcept;
void rsf_rdr2_settings_row(const void* destination, const void* context, const void* metadata) noexcept;
void rsf_rdr2_settings_value(const void* path, const void* source, const void* type,
    const void* destination, int32_t flags) noexcept;
}

namespace {
using Bind = void (*)(void*, const void*, const void*);
using Read = void (*)(const void*, void*, const void*, void*, int32_t);
Bind bind_original{};
Read read_original{};

void bind(void* destination, const void* context, const void* metadata)
{
    rsf_rdr2_settings_row(destination, context, metadata);
    bind_original(destination, context, metadata);
}

void read(const void* path, void* source, const void* type, void* destination, int32_t flags)
{
    read_original(path, source, type, destination, flags);
    rsf_rdr2_settings_value(path, source, type, destination, flags);
}
}

bool rsf_rdr2_native_settings_probe(const wchar_t* path) noexcept
{
    const rsf::rdr2::DiagnosticHook hooks[] = {
        {reinterpret_cast<const void*>(&bind), reinterpret_cast<void**>(&bind_original)},
        {reinterpret_cast<const void*>(&read), reinterpret_cast<void**>(&read_original)},
    };
    return rsf_rdr2_settings_install(path, hooks, [](uintptr_t target) noexcept -> int32_t {
        return MH_EnableHook(reinterpret_cast<void*>(target));
    });
}
