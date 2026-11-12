#include <windows.h>
#include <unknwn.h>
#include <cstring>
#include "thin_startup.h"
#include "thin_library.h"

namespace {
INIT_ONCE system_once = INIT_ONCE_STATIC_INIT;
INIT_ONCE startup_once = INIT_ONCE_STATIC_INIT;
BOOL CALLBACK start_configured(PINIT_ONCE, PVOID, PVOID*)
{
    thin_startup();
    // A refusal is final for this process; later creates only forward.
    return TRUE;
}
HMODULE system_module = nullptr;
BOOL CALLBACK load_system(PINIT_ONCE, PVOID, PVOID*)
{
    wchar_t path[MAX_PATH];
    const UINT length = GetSystemDirectoryW(path, MAX_PATH);
    constexpr wchar_t suffix[] = L"\\dinput8.dll";
    if (length && length + _countof(suffix) <= _countof(path)) {
        std::memcpy(path + length, suffix, sizeof(suffix));
        // A bare filename can resolve to this proxy, even with SEARCH_SYSTEM32.
        system_module = thin_load_library(path, LOAD_LIBRARY_SEARCH_SYSTEM32);
    }
    return TRUE;
}
template<typename T> T system_proc(const char* name)
{
    InitOnceExecuteOnce(&system_once, load_system, nullptr, nullptr);
    FARPROC address = system_module ? GetProcAddress(system_module, name) : nullptr;
    T result = nullptr;
    static_assert(sizeof(result) == sizeof(address));
    std::memcpy(&result, &address, sizeof(result));
    return result;
}
}
extern "C" HRESULT WINAPI DirectInput8Create(HINSTANCE instance, DWORD version, REFIID iid,
    LPVOID* output, LPUNKNOWN outer)
{
    InitOnceExecuteOnce(&startup_once, start_configured, nullptr, nullptr);
    using Function = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto function = system_proc<Function>("DirectInput8Create");
    return function ? function(instance, version, iid, output, outer) : E_FAIL;
}
extern "C" HRESULT WINAPI DllCanUnloadNow()
{
    const auto function = system_proc<HRESULT (WINAPI*)()>("DllCanUnloadNow");
    return function ? function() : E_FAIL;
}
extern "C" HRESULT WINAPI DllGetClassObject(REFCLSID clsid, REFIID iid, LPVOID* output)
{
    using Function = HRESULT (WINAPI*)(REFCLSID, REFIID, LPVOID*);
    const auto function = system_proc<Function>("DllGetClassObject");
    return function ? function(clsid, iid, output) : E_FAIL;
}
extern "C" HRESULT WINAPI DllRegisterServer()
{
    const auto function = system_proc<HRESULT (WINAPI*)()>("DllRegisterServer");
    return function ? function() : E_FAIL;
}
extern "C" HRESULT WINAPI DllUnregisterServer()
{
    const auto function = system_proc<HRESULT (WINAPI*)()>("DllUnregisterServer");
    return function ? function() : E_FAIL;
}
extern "C" const void* WINAPI GetdfDIJoystick()
{
    const auto function = system_proc<const void* (WINAPI*)()>("GetdfDIJoystick");
    return function ? function() : nullptr;
}
