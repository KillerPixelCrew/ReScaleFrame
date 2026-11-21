// SPDX-License-Identifier: MIT
#include "camera_probe.h"
#include "asi_platform.h"
#include <MinHook.h>
#include <intrin.h>

extern "C" {
bool rsf_rdr2_camera_install(const wchar_t*, const rsf::rdr2::DiagnosticHook*, int32_t (*)(uintptr_t)) noexcept;
bool rsf_rdr2_camera_reference(void*, const wchar_t*) noexcept;
void rsf_rdr2_camera_request() noexcept;
void rsf_rdr2_camera_capture(uint32_t provider, const void* context, const void* parameters,
    const void* caller, uintptr_t (*thread_slots)()) noexcept;
}

namespace {
using Upscale = bool (*)(void*, void*, bool);
Upscale fsr_original{}, dlss_original{};

uintptr_t thread_slots() noexcept { return __readgsqword(0x58); }

bool fsr_hook(void* context, void* parameters, bool option)
{
    rsf_rdr2_camera_capture(2, context, parameters, _ReturnAddress(), thread_slots);
    return fsr_original(context, parameters, option);
}

bool dlss_hook(void* context, void* parameters, bool option)
{
    rsf_rdr2_camera_capture(1, context, parameters, _ReturnAddress(), thread_slots);
    return dlss_original(context, parameters, option);
}
}

bool rsf_rdr2_install_camera_probe(const wchar_t* directory) noexcept
{
    const rsf::rdr2::DiagnosticHook hooks[] = {
        {reinterpret_cast<const void*>(&fsr_hook), reinterpret_cast<void**>(&fsr_original)},
        {reinterpret_cast<const void*>(&dlss_hook), reinterpret_cast<void**>(&dlss_original)},
    };
    return rsf_rdr2_camera_install(directory, hooks, [](uintptr_t target) noexcept -> int32_t {
        return MH_EnableHook(reinterpret_cast<void*>(target));
    });
}

bool rsf_rdr2_install_camera_reference(void* instance, const wchar_t* library) noexcept
{
    return rsf_rdr2_camera_reference(instance, library);
}

void rsf_rdr2_request_camera_capture() noexcept
{
    rsf_rdr2_camera_request();
}
