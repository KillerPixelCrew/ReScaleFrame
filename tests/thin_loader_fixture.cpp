#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cwchar>

namespace {
void record(char event)
{
    wchar_t path[32768];
    if (!GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_EVENTS", path, _countof(path))) return;
    HANDLE file = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
        nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        WriteFile(file, &event, 1, &written, nullptr);
        CloseHandle(file);
    }
}
}
BOOL WINAPI DllMain(HINSTANCE, DWORD reason, LPVOID)
{
    if (reason == DLL_PROCESS_ATTACH) {
        wchar_t check[8];
        if (GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_CHECK_ERROR_MODE", check, _countof(check))) {
            constexpr DWORD flags = SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX;
            if ((GetThreadErrorMode() & flags) != flags) record('H');
        }
#ifdef THIN_FIXTURE_RUNTIME
        record('R');
#else
        record('A');
#endif
    }
    return TRUE;
}
#ifndef THIN_FIXTURE_RUNTIME
extern "C" __declspec(dllexport) uint32_t __stdcall ThinFixtureStart(const wchar_t* configuration_path)
{
    wchar_t expected[32768];
    if (!GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_CONFIG", expected, _countof(expected)) ||
        !configuration_path || std::wcscmp(expected, configuration_path) != 0) {
        record('X');
        return 1;
    }
    wchar_t pointer[64];
    if (GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_OUTPUT", pointer, _countof(pointer))) {
        const auto address = static_cast<uintptr_t>(std::wcstoull(pointer, nullptr, 16));
        if (*reinterpret_cast<void* volatile*>(address) != nullptr) {
            record('O'); // Real DirectInputCreate has already modified the caller output.
            return 2;
        }
    }
    wchar_t thread[32];
    if (GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_THREAD", thread, _countof(thread)) &&
        GetCurrentThreadId() != std::wcstoul(thread, nullptr, 10)) {
        record('T');
        return 3;
    }
    // Make a premature asynchronous return reliably visible to the caller.
    wchar_t mode[32];
    if (GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_THREAD_MODE", mode, _countof(mode)) &&
        GetThreadErrorMode() != std::wcstoul(mode, nullptr, 10)) {
        record('M');
        return 4;
    }
    if (GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_PROCESS_MODE", mode, _countof(mode)) &&
        GetErrorMode() != std::wcstoul(mode, nullptr, 10)) {
        record('P');
        return 5;
    }
    Sleep(80);
    record('E');
    wchar_t refusal[8];
    return GetEnvironmentVariableW(L"RSF_THIN_FIXTURE_REFUSE", refusal, _countof(refusal)) ? 7u : 0u;
}
#endif
