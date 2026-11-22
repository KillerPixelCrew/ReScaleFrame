// SPDX-License-Identifier: MIT
#include <stdint.h>
#include <windows.h>

extern BOOL rsf_test_thin_fixture_attach(DWORD event, uint8_t marker);

BOOL WINAPI DllMain(HINSTANCE instance, DWORD event, LPVOID reserved)
{
    (void)instance;
    (void)reserved;
#ifdef THIN_FIXTURE_RUNTIME
    return rsf_test_thin_fixture_attach(event, 'R');
#else
    return rsf_test_thin_fixture_attach(event, 'A');
#endif
}

#ifndef THIN_FIXTURE_RUNTIME
extern uint32_t WINAPI rsf_test_thin_fixture_start(const wchar_t* configuration_path);

__declspec(dllexport) uint32_t WINAPI ThinFixtureStart(const wchar_t* configuration_path)
{
    return rsf_test_thin_fixture_start(configuration_path);
}
#endif
